//! 分片上传:大文件分块上传,支持子资源签名(`uploads` / `uploadId` / `partNumber`)。
//!
//! 低层四步:[`OssClient::initiate_multipart_upload`] → 多次
//! [`OssClient::upload_part`] → [`OssClient::complete_multipart_upload`],出错时
//! [`OssClient::abort_multipart_upload`]。高层 [`OssClient::upload_multipart`] 把整块
//! 数据按 `part_size` 切分并自动编排(失败自动 abort)。

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, DATE, ETAG};
use reqwest::{Method, Request};
use serde::Deserialize;

use crate::client::{encode_key, OssClient};
use crate::error::{OssError, Result};
use crate::object::{check_status, now_gmt};
use crate::sign;

/// OSS 分片下限:除最后一片外,每片至少 100 KiB。
pub const MIN_PART_SIZE: usize = 100 * 1024;

/// 分片并发上传的默认并发度。取 4 是吞吐与内存 / 连接数的折中。
pub const UPLOAD_CONCURRENCY: usize = 4;

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
        self.upload_multipart_progress(bucket, key, data, part_size, content_type, |_, _| {})
            .await
    }

    /// 同 [`Self::upload_multipart`],但每传完一个分片回调一次 `(已上传字节, 总字节)`。
    pub async fn upload_multipart_progress<F: FnMut(u64, u64)>(
        &self,
        bucket: &str,
        key: &str,
        data: impl Into<Bytes>,
        part_size: usize,
        content_type: Option<&str>,
        mut on_progress: F,
    ) -> Result<()> {
        let data: Bytes = data.into();
        let part_size = part_size.max(MIN_PART_SIZE);
        let total = data.len() as u64;
        on_progress(0, total);

        let upload_id = self
            .initiate_multipart_upload(bucket, key, content_type)
            .await?;

        let outcome = self
            .upload_all_parts(bucket, key, &upload_id, &data, part_size, &mut on_progress)
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

    /// 流式分片上传:从 `stream` 边收边传,内存只保留"未满一片"的缓冲(≈ `part_size`),
    /// 与对象总大小无关。`total_hint` 仅用于进度分母(未知传 0)。任一步失败自动 abort。
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_multipart_stream<S, E>(
        &self,
        bucket: &str,
        key: &str,
        stream: S,
        part_size: usize,
        content_type: Option<&str>,
        total_hint: u64,
        mut on_progress: impl FnMut(u64, u64),
    ) -> Result<()>
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Unpin,
        E: std::fmt::Display,
    {
        let part_size = part_size.max(MIN_PART_SIZE);
        on_progress(0, total_hint);

        let upload_id = self
            .initiate_multipart_upload(bucket, key, content_type)
            .await?;

        let outcome = self
            .stream_parts(
                bucket,
                key,
                &upload_id,
                stream,
                part_size,
                total_hint,
                &mut on_progress,
            )
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

    /// 拉流 → 攒够 `part_size` 就顺序上传一片(内存受控)。收尾把剩余字节作为末片;
    /// 若整个流为空则仍传一个空片,满足"至少一片"。
    #[allow(clippy::too_many_arguments)]
    async fn stream_parts<S, E>(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        mut stream: S,
        part_size: usize,
        total: u64,
        on_progress: &mut impl FnMut(u64, u64),
    ) -> Result<Vec<(u32, String)>>
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Unpin,
        E: std::fmt::Display,
    {
        let mut parts = Vec::new();
        let mut buf = BytesMut::new();
        let mut part_number = 1u32;
        let mut uploaded = 0u64;

        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| {
                OssError::Core(cloud_core::CoreError::InvalidRequest(format!(
                    "source stream error: {e}"
                )))
            })?;
            buf.extend_from_slice(&chunk);
            while buf.len() >= part_size {
                let part = buf.split_to(part_size).freeze();
                let len = part.len() as u64;
                let etag = self
                    .upload_part(bucket, key, upload_id, part_number, part)
                    .await?;
                parts.push((part_number, etag));
                part_number += 1;
                uploaded += len;
                on_progress(uploaded, total);
            }
        }

        if !buf.is_empty() || parts.is_empty() {
            let part = buf.freeze();
            let len = part.len() as u64;
            let etag = self
                .upload_part(bucket, key, upload_id, part_number, part)
                .await?;
            parts.push((part_number, etag));
            uploaded += len;
            on_progress(uploaded, total);
        }
        Ok(parts)
    }

    /// 有界并发上传所有分片(同时最多 [`UPLOAD_CONCURRENCY`] 片在飞),每片完成后串行累加
    /// 进度;返回 `(part_number, etag)` 列表。分片乱序完成不影响结果——complete 会重新排序。
    async fn upload_all_parts<F: FnMut(u64, u64)>(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        data: &Bytes,
        part_size: usize,
        on_progress: &mut F,
    ) -> Result<Vec<(u32, String)>> {
        let total = data.len() as u64;
        let specs = split_parts(data, part_size);

        let mut stream = futures::stream::iter(specs.into_iter().map(|(number, chunk)| {
            let len = chunk.len() as u64;
            async move {
                let etag = self
                    .upload_part(bucket, key, upload_id, number, chunk)
                    .await?;
                Ok::<(u32, String, u64), OssError>((number, etag, len))
            }
        }))
        .buffer_unordered(UPLOAD_CONCURRENCY);

        let mut parts = Vec::new();
        let mut uploaded = 0u64;
        while let Some(result) = stream.next().await {
            let (number, etag, len) = result?;
            parts.push((number, etag));
            uploaded += len;
            on_progress(uploaded, total);
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
        let url = format!(
            "{}/{}?{}",
            self.bucket_base_url(bucket),
            encode_key(req.key),
            query
        );

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

/// 把整块数据切成 `(part_number, chunk)` 列表,part number 从 1 开始;空数据切成一个空片。
fn split_parts(data: &Bytes, part_size: usize) -> Vec<(u32, Bytes)> {
    let mut specs = Vec::new();
    let mut offset = 0usize;
    let mut part_number = 1u32;
    while offset < data.len() {
        let end = (offset + part_size).min(data.len());
        specs.push((part_number, data.slice(offset..end)));
        offset = end;
        part_number += 1;
    }
    if specs.is_empty() {
        specs.push((1, data.slice(0..0)));
    }
    specs
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
    fn split_parts_covers_all_bytes_and_handles_empty() {
        let specs = split_parts(&Bytes::from(vec![0u8; 25]), 10);
        assert_eq!(specs.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(specs.iter().map(|(_, c)| c.len()).sum::<usize>(), 25);
        let empty = split_parts(&Bytes::new(), 10);
        assert_eq!(empty.len(), 1);
        assert!(empty[0].1.is_empty());
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
