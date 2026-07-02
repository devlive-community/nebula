//! 分片上传:大文件分块上传,支持子资源签名(`uploads` / `uploadId` / `partNumber`)。
//!
//! 低层四步:[`OssClient::initiate_multipart_upload`] → 多次
//! [`OssClient::upload_part`] → [`OssClient::complete_multipart_upload`],出错时
//! [`OssClient::abort_multipart_upload`]。高层 [`OssClient::upload_multipart`] 把整块
//! 数据按 `part_size` 切分并自动编排(失败自动 abort)。

use bytes::Bytes;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, DATE, ETAG};
use reqwest::{Method, Request};
use serde::Deserialize;

use crate::client::OssClient;
use crate::error::{OssError, Result};
use crate::object::{check_status, now_gmt};
use crate::sign;

/// OSS 分片下限:除最后一片外,每片至少 100 KiB。
pub const MIN_PART_SIZE: usize = 100 * 1024;

/// 一次分片请求的输入。用结构体收拢以避免过多参数。
struct PartRequest<'a> {
    method: Method,
    key: &'a str,
    subresources: &'a [(&'a str, Option<&'a str>)],
    content_type: Option<&'a str>,
    content_md5: Option<&'a str>,
    body: Option<Bytes>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InitiateResult {
    upload_id: String,
}

impl OssClient {
    /// 初始化一次分片上传,返回 `UploadId`。
    pub async fn initiate_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: Option<&str>,
    ) -> Result<String> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::POST,
                key,
                subresources: &[("uploads", None)],
                content_type,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(cloud_core::CoreError::from)?;
        let parsed: InitiateResult = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(cloud_core::CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed.upload_id)
    }

    /// 上传一个分片(`part_number` 从 1 开始),返回该片的 ETag。
    pub async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: u32,
        body: impl Into<Bytes>,
    ) -> Result<String> {
        let date = now_gmt();
        let part = part_number.to_string();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key,
                subresources: &[("partNumber", Some(&part)), ("uploadId", Some(upload_id))],
                content_type: None,
                content_md5: None,
                body: Some(body.into()),
            },
            &date,
        )?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let etag = resp
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                OssError::Core(cloud_core::CoreError::InvalidResponse(
                    "upload part response missing ETag".into(),
                ))
            })?;
        Ok(etag)
    }

    /// 完成分片上传。`parts` 为 `(part_number, etag)` 列表,内部会按序号排序。
    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let date = now_gmt();
        let body = complete_body(parts);
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::POST,
                key,
                subresources: &[("uploadId", Some(upload_id))],
                content_type: Some("application/xml"),
                content_md5: None,
                body: Some(Bytes::from(body)),
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 中止一次分片上传,释放已上传的分片。
    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<()> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::DELETE,
                key,
                subresources: &[("uploadId", Some(upload_id))],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 高层封装:把整块数据按 `part_size` 切分并完成分片上传;任一步失败自动 abort。
    ///
    /// `part_size` 会被抬到不小于 [`MIN_PART_SIZE`]。至少上传一个分片(空数据也会
    /// 产生一个空的最后分片)。
    pub async fn upload_multipart(
        &self,
        bucket: &str,
        key: &str,
        data: impl Into<Bytes>,
        part_size: usize,
        content_type: Option<&str>,
    ) -> Result<()> {
        let data: Bytes = data.into();
        let part_size = part_size.max(MIN_PART_SIZE);
        let upload_id = self
            .initiate_multipart_upload(bucket, key, content_type)
            .await?;

        let outcome = self
            .upload_all_parts(bucket, key, &upload_id, &data, part_size)
            .await;
        match outcome {
            Ok(parts) => {
                let completed = self
                    .complete_multipart_upload(bucket, key, &upload_id, &parts)
                    .await;
                if completed.is_err() {
                    let _ = self.abort_multipart_upload(bucket, key, &upload_id).await;
                }
                completed
            }
            Err(err) => {
                let _ = self.abort_multipart_upload(bucket, key, &upload_id).await;
                Err(err)
            }
        }
    }

    /// 顺序上传所有分片,返回 `(part_number, etag)` 列表。
    async fn upload_all_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        data: &Bytes,
        part_size: usize,
    ) -> Result<Vec<(u32, String)>> {
        let mut parts = Vec::new();
        let mut offset = 0usize;
        let mut part_number = 1u32;
        loop {
            let end = (offset + part_size).min(data.len());
            let chunk = data.slice(offset..end);
            let etag = self
                .upload_part(bucket, key, upload_id, part_number, chunk)
                .await?;
            parts.push((part_number, etag));
            offset = end;
            part_number += 1;
            if offset >= data.len() {
                break;
            }
        }
        Ok(parts)
    }

    /// 组装并签名一次分片相关请求(子资源计入 CanonicalizedResource)。
    fn build_part_request(
        &self,
        bucket: &str,
        req: PartRequest<'_>,
        date: &str,
    ) -> Result<Request> {
        let canonical = sign::canonicalized_resource(bucket, req.key, req.subresources);
        let sts = sign::string_to_sign(
            req.method.as_str(),
            req.content_md5.unwrap_or(""),
            req.content_type.unwrap_or(""),
            date,
            "",
            &canonical,
        );
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);

        // URL 的查询串直接用子资源(顺序不影响服务端)。
        let query = req
            .subresources
            .iter()
            .map(|(k, v)| match v {
                Some(v) => format!("{k}={v}"),
                None => (*k).to_string(),
            })
            .collect::<Vec<_>>()
            .join("&");
        let url = format!("{}/{}?{}", self.bucket_base_url(bucket), req.key, query);

        let mut builder = self
            .http()
            .inner()
            .request(req.method, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization);
        if let Some(ct) = req.content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        if let Some(md5) = req.content_md5 {
            builder = builder.header("Content-MD5", md5);
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }
        builder
            .build()
            .map_err(cloud_core::CoreError::from)
            .map_err(OssError::from)
    }
}

/// 生成 CompleteMultipartUpload 的请求体 XML(按 part number 升序)。
fn complete_body(parts: &[(u32, String)]) -> String {
    let mut parts = parts.to_vec();
    parts.sort_by_key(|(n, _)| *n);
    let mut body = String::from("<CompleteMultipartUpload>");
    for (number, etag) in &parts {
        body.push_str(&format!(
            "<Part><PartNumber>{number}</PartNumber><ETag>{etag}</ETag></Part>"
        ));
    }
    body.push_str("</CompleteMultipartUpload>");
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> OssClient {
        OssClient::new(
            "44CF9590006BF252F707",
            "OtxrzxIsfpFjA7SwPzILwy8Bw21TLhquhboDYROV",
            "oss-cn-hangzhou.aliyuncs.com",
        )
    }

    #[test]
    fn initiate_request_signs_uploads_subresource() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_part_request(
                "oss-example",
                PartRequest {
                    method: Method::POST,
                    key: "big.bin",
                    subresources: &[("uploads", None)],
                    content_type: None,
                    content_md5: None,
                    body: None,
                },
                date,
            )
            .unwrap();

        assert_eq!(req.method(), Method::POST);
        assert_eq!(
            req.url().as_str(),
            "https://oss-example.oss-cn-hangzhou.aliyuncs.com/big.bin?uploads"
        );
        let sts = sign::string_to_sign("POST", "", "", date, "", "/oss-example/big.bin?uploads");
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts)
        );
    }

    #[test]
    fn upload_part_signs_sorted_subresources() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_part_request(
                "b",
                PartRequest {
                    method: Method::PUT,
                    key: "k",
                    subresources: &[("partNumber", Some("2")), ("uploadId", Some("UP42"))],
                    content_type: None,
                    content_md5: None,
                    body: Some(Bytes::from_static(b"chunk")),
                },
                date,
            )
            .unwrap();

        // 签名 canonical 里子资源按字典序:partNumber 先于 uploadId。
        let sts = sign::string_to_sign("PUT", "", "", date, "", "/b/k?partNumber=2&uploadId=UP42");
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts)
        );
    }

    #[test]
    fn complete_body_is_sorted_xml() {
        let parts = vec![
            (2, "\"E2\"".to_string()),
            (1, "\"E1\"".to_string()),
            (3, "\"E3\"".to_string()),
        ];
        let body = complete_body(&parts);
        assert_eq!(
            body,
            "<CompleteMultipartUpload>\
             <Part><PartNumber>1</PartNumber><ETag>\"E1\"</ETag></Part>\
             <Part><PartNumber>2</PartNumber><ETag>\"E2\"</ETag></Part>\
             <Part><PartNumber>3</PartNumber><ETag>\"E3\"</ETag></Part>\
             </CompleteMultipartUpload>"
        );
    }

    #[test]
    fn parses_initiate_result() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<InitiateMultipartUploadResult>
  <Bucket>oss-example</Bucket>
  <Key>big.bin</Key>
  <UploadId>0004B9894A22E5B1888A1E29F823</UploadId>
</InitiateMultipartUploadResult>"#;
        let parsed: InitiateResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.upload_id, "0004B9894A22E5B1888A1E29F823");
    }
}
