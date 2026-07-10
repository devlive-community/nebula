//! 分片上传(COS Multipart Upload,S3 风格)。
//!
//! 四步:initiate → 多次 upload_part → complete,出错时 abort。分片相关查询参数
//! (`uploads` / `partNumber` / `uploadId`)由 COS 签名自动覆盖(参与 canonical)。

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use reqwest::header::ETAG;
use reqwest::Method;
use serde::Deserialize;

use crate::client::{CosClient, SignSpec};
use crate::error::{CosError, Result};
use crate::object::{check_status, object_uri};

/// COS 分片下限:除最后一片外,每片至少 1 MiB。
pub const MIN_PART_SIZE: usize = 1024 * 1024;

/// 分片并发上传的默认并发度。取 4 是吞吐与内存 / 连接数的折中。
pub const UPLOAD_CONCURRENCY: usize = 4;

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
                CosError::Core(cloud_core::CoreError::InvalidRequest(format!(
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
                Ok::<(u32, String, u64), CosError>((number, etag, len))
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
        let xml = r#"<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><UploadId>UP-9</UploadId></InitiateMultipartUploadResult>"#;
        let parsed: InitiateResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.upload_id, "UP-9");
    }
}
