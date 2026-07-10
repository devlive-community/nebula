//! 分片上传(S3 Multipart Upload)。
//!
//! 四步:[`S3Client::initiate_multipart_upload`] → 多次 [`S3Client::upload_part`] →
//! [`S3Client::complete_multipart_upload`],出错时 [`S3Client::abort_multipart_upload`]。
//! 高层 [`S3Client::upload_multipart`] 按 `part_size` 切分并自动编排(失败自动 abort)。
//!
//! 分片相关的查询参数(`uploads` / `partNumber` / `uploadId`)由 SigV4 自动签名,
//! 无需 V2 那样的子资源特判。

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::ETAG;
use reqwest::Method;
use serde::Deserialize;

use s3_sigv4::RequestSpec;

use crate::client::S3Client;
use crate::error::{Result, S3Error};
use crate::object::{check_status, object_uri};

/// S3 分片下限:除最后一片外,每片至少 5 MiB。
pub const MIN_PART_SIZE: usize = 5 * 1024 * 1024;

/// 分片并发上传的默认并发度。取 4 是吞吐与内存 / 连接数的折中。
pub const UPLOAD_CONCURRENCY: usize = 4;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InitiateResult {
    upload_id: String,
}

impl S3Client {
    /// 初始化一次分片上传,返回 `UploadId`。
    pub async fn initiate_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: Option<&str>,
    ) -> Result<String> {
        let request = self.build_signed(RequestSpec {
            method: Method::POST,
            canonical_uri: &object_uri(bucket, key),
            query: &[("uploads".to_string(), String::new())],
            content_type,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(cloud_core::CoreError::from)?;
        let parsed: InitiateResult = quick_xml::de::from_str(&body)
            .map_err(|e| S3Error::Core(cloud_core::CoreError::InvalidResponse(e.to_string())))?;
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
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &object_uri(bucket, key),
            query: &[
                ("partNumber".to_string(), part_number.to_string()),
                ("uploadId".to_string(), upload_id.to_string()),
            ],
            content_type: None,
            amz_headers: &[],
            body: Some(body.into()),
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        resp.headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                S3Error::Core(cloud_core::CoreError::InvalidResponse(
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
        let request = self.build_signed(RequestSpec {
            method: Method::POST,
            canonical_uri: &object_uri(bucket, key),
            query: &[("uploadId".to_string(), upload_id.to_string())],
            content_type: Some("application/xml"),
            amz_headers: &[],
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
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &object_uri(bucket, key),
            query: &[("uploadId".to_string(), upload_id.to_string())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 高层封装:按 `part_size` 切分整块数据并完成分片上传;任一步失败自动 abort。
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
        let specs = split_parts(data, part_size);

        // 有界并发上传:同时最多 [`UPLOAD_CONCURRENCY`] 片在飞。`buffer_unordered` 谁先完成谁
        // 先返回,进度在这个消费循环里串行累加,回调不进并发任务,无需加锁。分片乱序完成不影响
        // 最终结果——`complete_multipart_upload` 会按 part number 重新排序。
        let mut stream = futures::stream::iter(specs.into_iter().map(|(number, chunk)| {
            let len = chunk.len() as u64;
            async move {
                let etag = self
                    .upload_part(bucket, key, upload_id, number, chunk)
                    .await?;
                Ok::<(u32, String, u64), S3Error>((number, etag, len))
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
}

/// 把整块数据切成 `(part_number, chunk)` 列表,part number 从 1 开始。
/// 空数据切成一个空分片(部分实现要求至少一片)。
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

    #[test]
    fn complete_body_is_sorted_xml() {
        let parts = vec![
            (2, "\"E2\"".to_string()),
            (1, "\"E1\"".to_string()),
            (3, "\"E3\"".to_string()),
        ];
        assert_eq!(
            complete_body(&parts),
            "<CompleteMultipartUpload>\
             <Part><PartNumber>1</PartNumber><ETag>\"E1\"</ETag></Part>\
             <Part><PartNumber>2</PartNumber><ETag>\"E2\"</ETag></Part>\
             <Part><PartNumber>3</PartNumber><ETag>\"E3\"</ETag></Part>\
             </CompleteMultipartUpload>"
        );
    }

    #[test]
    fn split_parts_covers_all_bytes_in_order() {
        let data = Bytes::from(vec![0u8; 25]);
        let specs = split_parts(&data, 10);
        assert_eq!(specs.len(), 3);
        assert_eq!(specs.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(
            specs.iter().map(|(_, c)| c.len()).collect::<Vec<_>>(),
            [10, 10, 5]
        );
        let total: usize = specs.iter().map(|(_, c)| c.len()).sum();
        assert_eq!(total, 25);
    }

    #[test]
    fn split_parts_empty_data_yields_one_empty_part() {
        let specs = split_parts(&Bytes::new(), 10);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].0, 1);
        assert!(specs[0].1.is_empty());
    }

    #[test]
    fn parses_initiate_result() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><UploadId>UP-123</UploadId></InitiateMultipartUploadResult>"#;
        let parsed: InitiateResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.upload_id, "UP-123");
    }
}
