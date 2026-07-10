//! 对象级操作:上传 / 下载 / 删除 / 元信息 / 服务端复制 / 预签名。

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED};
use reqwest::{Method, Response, StatusCode};
use serde::Deserialize;

use crate::client::{encode_key, now_unix, CosClient, SignSpec};
use crate::error::{CosError, Result};
use crate::sign;

/// 对象元信息(HEAD 返回)。
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub content_length: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// COS 错误响应体(XML)。
#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "RequestId")]
    request_id: Option<String>,
}

impl CosClient {
    /// 上传一个对象。
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: impl Into<Bytes>,
        content_type: Option<&str>,
    ) -> Result<()> {
        let body: Bytes = body.into();
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type,
            content_md5: None,
            cos_headers: &[],
            body: Some(body),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 下载一个对象,返回完整字节。
    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Bytes> {
        let resp = self.get_object_response(bucket, key).await?;
        Ok(resp.bytes().await.map_err(cloud_core::CoreError::from)?)
    }

    /// 流式下载,返回 `(内容长度, 分块字节流)`。
    pub async fn get_object_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(Option<u64>, impl futures::Stream<Item = Result<Bytes>>)> {
        let resp = self.get_object_response(bucket, key).await?;
        let len = resp.content_length();
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(|e| CosError::Core(cloud_core::CoreError::from(e))));
        Ok((len, stream))
    }

    async fn get_object_response(&self, bucket: &str, key: &str) -> Result<Response> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await
    }

    /// 删除一个对象。
    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::DELETE,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取对象元信息(HEAD)。
    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::HEAD,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
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
        // x-cos-copy-source = {srcHost}/{encodedSrcKey},作为被签名的 x-cos-* 头。
        let copy_source = format!("{}/{}", self.bucket_host(src_bucket), encode_key(src_key));
        let host = self.bucket_host(dst_bucket);
        let uri = object_uri(dst_key);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[("x-cos-copy-source", copy_source)],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 生成一个 GET 预签名 URL,`expires_in` 秒后失效。纯本地签名,不发请求。
    pub fn presign_get(&self, bucket: &str, key: &str, expires_in: u64) -> Result<String> {
        Ok(self.build_presigned_url(bucket, key, expires_in, now_unix()))
    }

    /// 用固定起始时间构造预签名 URL,便于确定性测试。
    fn build_presigned_url(&self, bucket: &str, key: &str, expires_in: u64, now: u64) -> String {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let key_time = format!("{};{}", now, now + expires_in);
        // 预签名只签 host 头、无参数。
        let (header_list, header_string) = sign::canonical(&[("host", &host)]);
        let http = sign::http_string("GET", &uri, "", &header_string);
        let sign_key = sign::sign_key(self.secret_key(), &key_time);
        let sts = sign::string_to_sign(&key_time, &http);
        let signature = sign::signature(&sign_key, &sts);
        let auth = sign::authorization(self.secret_id(), &key_time, &header_list, "", &signature);
        format!("https://{host}{uri}?{auth}")
    }
}

/// 对象的 URI path:`/{encoded_key}`。
pub(crate) fn object_uri(key: &str) -> String {
    format!("/{}", encode_key(key))
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

/// 成功(2xx)原样返回;否则读取 COS Error XML 转成 [`CosError::Api`]。
pub(crate) async fn check_status(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    match quick_xml::de::from_str::<ErrorBody>(&body) {
        Ok(err) => Err(CosError::Api {
            status: code,
            code: err.code,
            message: err.message,
            request_id: err.request_id,
        }),
        Err(_) => Err(CosError::Api {
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

    fn client() -> CosClient {
        CosClient::new(
            "MySecretId",
            "MySecretKey123",
            "cos.ap-beijing.myqcloud.com",
        )
    }

    #[test]
    fn object_uri_is_encoded() {
        assert_eq!(object_uri("dir/a.txt"), "/dir/a.txt");
        assert_eq!(object_uri("dir/ b.txt"), "/dir/%20b.txt");
    }

    #[test]
    fn presign_url_has_qsign_params() {
        let c = client();
        let url = c.build_presigned_url("bkt-123", "exampleobject", 3600, 1_600_000_000);
        assert!(url.starts_with("https://bkt-123.cos.ap-beijing.myqcloud.com/exampleobject?"));
        assert!(url.contains("q-sign-algorithm=sha1"));
        assert!(url.contains("q-ak=MySecretId"));
        assert!(url.contains("q-sign-time=1600000000;1600003600"));
        assert!(url.contains("q-signature="));
    }

    #[test]
    fn parses_cos_error_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error><Code>NoSuchKey</Code><Message>key not found</Message><RequestId>abc</RequestId></Error>"#;
        let parsed: ErrorBody = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.code, "NoSuchKey");
        assert_eq!(parsed.request_id.as_deref(), Some("abc"));
    }
}
