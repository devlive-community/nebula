//! 分片上传(COS Multipart Upload,S3 风格)。
//!
//! 四步:initiate → 多次 upload_part → complete,出错时 abort。分片相关查询参数
//! (`uploads` / `partNumber` / `uploadId`)由 COS 签名自动覆盖(参与 canonical)。

use bytes::Bytes;
use reqwest::header::ETAG;
use reqwest::Method;
use serde::Deserialize;

use crate::client::{CosClient, SignSpec};
use crate::error::{CosError, Result};
use crate::object::{check_status, object_uri};

/// COS 分片下限:除最后一片外,每片至少 1 MiB。
pub const MIN_PART_SIZE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InitiateResult {
    upload_id: String,
}

impl CosClient {
    /// 初始化一次分片上传,返回 `UploadId`。
    pub async fn initiate_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: Option<&str>,
    ) -> Result<String> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::POST,
            host: &host,
            uri_path: &uri,
            query: &[("uploads", None)],
            content_type,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(cloud_core::CoreError::from)?;
        let parsed: InitiateResult = quick_xml::de::from_str(&body)
            .map_err(|e| CosError::Core(cloud_core::CoreError::InvalidResponse(e.to_string())))?;
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
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let part = part_number.to_string();
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[("partNumber", Some(&part)), ("uploadId", Some(upload_id))],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: Some(body.into()),
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        resp.headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                CosError::Core(cloud_core::CoreError::InvalidResponse(
                    "upload part response missing ETag".into(),
                ))
            })
    }

    /// 完成分片上传。`parts` 为 `(part_number, etag)` 列表,内部会按序号排序。
    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::POST,
            host: &host,
            uri_path: &uri,
            query: &[("uploadId", Some(upload_id))],
            content_type: Some("application/xml"),
            content_md5: None,
            cos_headers: &[],
            body: Some(Bytes::from(complete_body(parts))),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 中止一次分片上传。
    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::DELETE,
            host: &host,
            uri_path: &uri,
            query: &[("uploadId", Some(upload_id))],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 高层封装:按 `part_size` 切分并完成分片上传;任一步失败自动 abort。
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

    /// 同 [`Self::upload_multipart`],但每传完一个分片回调 `(已上传字节, 总字节)`。
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
            on_progress(offset as u64, total);
            part_number += 1;
            if offset >= data.len() {
                break;
            }
        }
        Ok(parts)
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

    #[test]
    fn complete_body_is_sorted_xml() {
        let parts = vec![(2, "\"E2\"".to_string()), (1, "\"E1\"".to_string())];
        assert_eq!(
            complete_body(&parts),
            "<CompleteMultipartUpload>\
             <Part><PartNumber>1</PartNumber><ETag>\"E1\"</ETag></Part>\
             <Part><PartNumber>2</PartNumber><ETag>\"E2\"</ETag></Part>\
             </CompleteMultipartUpload>"
        );
    }

    #[test]
    fn parses_initiate_result() {
        let xml = r#"<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><UploadId>UP-9</UploadId></InitiateMultipartUploadResult>"#;
        let parsed: InitiateResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.upload_id, "UP-9");
    }
}
