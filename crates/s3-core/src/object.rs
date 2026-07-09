//! 对象级操作:上传 / 下载 / 删除 / 元信息 / 服务端复制 / 预签名。
//!
//! 所有请求经 [`crate::client`] 的 SigV4 签名后由共享 HTTP 客户端发出;非 2xx 响应会被
//! 解析成 [`S3Error::Api`](读取 S3 的 Error XML)。

use std::time::SystemTime;

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED};
use reqwest::{Method, Response, StatusCode};
use s3_sigv4::{encode_path, RequestSpec};
use serde::Deserialize;

use crate::client::S3Client;
use crate::error::{Result, S3Error};

/// 对象元信息(HEAD 返回)。
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub content_length: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// S3 错误响应体(XML)。
#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "RequestId")]
    request_id: Option<String>,
}

impl S3Client {
    /// 上传一个对象。
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: impl Into<Bytes>,
        content_type: Option<&str>,
    ) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &object_uri(bucket, key),
            query: &[],
            content_type,
            amz_headers: &[],
            body: Some(body.into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 下载一个对象,返回完整字节。
    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Bytes> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &object_uri(bucket, key),
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        Ok(resp.bytes().await.map_err(cloud_core::CoreError::from)?)
    }

    /// 流式下载一个对象,返回 `(内容长度, 分块字节流)`。
    pub async fn get_object_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(Option<u64>, impl futures::Stream<Item = Result<Bytes>>)> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &object_uri(bucket, key),
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let len = resp.content_length();
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(|e| S3Error::Core(cloud_core::CoreError::from(e))));
        Ok((len, stream))
    }

    /// 删除一个对象(不存在时 S3 也返回 204,视为成功)。
    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &object_uri(bucket, key),
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取对象元信息(HEAD)。
    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta> {
        let request = self.build_signed(RequestSpec {
            method: Method::HEAD,
            canonical_uri: &object_uri(bucket, key),
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let headers = resp.headers();
        let content_length = headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok(ObjectMeta {
            content_length,
            content_type: header_string(headers, CONTENT_TYPE),
            etag: header_string(headers, ETAG),
            last_modified: header_string(headers, LAST_MODIFIED),
        })
    }

    /// 服务端复制对象(支持同桶 / 跨桶)。
    pub async fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<()> {
        // x-amz-copy-source 是被签名的 x-amz-* 头,值为 /{srcBucket}/{encodedSrcKey}。
        let copy_source = format!("/{src_bucket}/{}", encode_path(src_key));
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &object_uri(dst_bucket, dst_key),
            query: &[],
            content_type: None,
            amz_headers: &[("x-amz-copy-source", copy_source)],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 生成一个 GET 预签名 URL(SigV4 query 方式),`expires_in` 秒后失效。纯本地签名。
    pub fn presign_get(&self, bucket: &str, key: &str, expires_in: u64) -> Result<String> {
        Ok(self.build_presigned_url(bucket, key, expires_in, SystemTime::now()))
    }

    /// 用固定时间构造预签名 URL,便于确定性测试。委托给共享 SigV4 实现。
    fn build_presigned_url(
        &self,
        bucket: &str,
        key: &str,
        expires_in: u64,
        now: SystemTime,
    ) -> String {
        s3_sigv4::presigned_get_url(&self.params(), &object_uri(bucket, key), expires_in, now)
    }
}

/// 路径风格的对象 canonical URI:`/{bucket}/{encoded_key}`。
pub(crate) fn object_uri(bucket: &str, key: &str) -> String {
    format!("/{bucket}/{}", encode_path(key))
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: impl reqwest::header::AsHeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// 成功(2xx)原样返回;否则读取 S3 Error XML 转成 [`S3Error::Api`]。
pub(crate) async fn check_status(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    match quick_xml::de::from_str::<ErrorBody>(&body) {
        Ok(err) => Err(S3Error::Api {
            status: code,
            code: err.code,
            message: err.message,
            request_id: err.request_id,
        }),
        Err(_) => Err(S3Error::Api {
            status: code,
            code: fallback_code(status),
            message: if body.is_empty() {
                status.to_string()
            } else {
                body
            },
            request_id: None,
        }),
    }
}

fn fallback_code(status: StatusCode) -> String {
    status
        .canonical_reason()
        .unwrap_or("UnknownError")
        .replace(' ', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn client() -> S3Client {
        S3Client::new(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "s3.cn-east-1.qiniucs.com",
        )
    }

    #[test]
    fn object_uri_is_path_style_and_encoded() {
        assert_eq!(object_uri("b", "dir/a.txt"), "/b/dir/a.txt");
        assert_eq!(object_uri("b", "dir/ x.txt"), "/b/dir/%20x.txt");
    }

    #[test]
    fn presign_url_has_sigv4_query_params() {
        let c = client();
        let url = c.build_presigned_url("mybucket", "hello.txt", 3600, at(1_440_938_160));

        assert!(url.starts_with("https://s3.cn-east-1.qiniucs.com/mybucket/hello.txt?"));
        let parsed = reqwest::Url::parse(&url).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params.get("X-Amz-Algorithm").unwrap(), "AWS4-HMAC-SHA256");
        assert_eq!(params.get("X-Amz-Expires").unwrap(), "3600");
        assert!(params.contains_key("X-Amz-Signature"));
        assert!(params
            .get("X-Amz-Credential")
            .unwrap()
            .contains("/cn-east-1/s3/aws4_request"));
    }

    #[test]
    fn parses_s3_error_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message><RequestId>abc</RequestId></Error>"#;
        let parsed: ErrorBody = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.code, "NoSuchKey");
        assert_eq!(parsed.request_id.as_deref(), Some("abc"));
    }
}
