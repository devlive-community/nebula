//! 对象级操作:上传 / 下载 / 删除 / 元信息。
//!
//! 所有请求都经 [`crate::sign`] 签名后由共享 HTTP 客户端发出;非 2xx 响应会被
//! 解析成 [`OssError::Api`](读取 OSS 的 Error XML)。

use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, DATE, ETAG, LAST_MODIFIED};
use reqwest::{Method, Request, Response, StatusCode, Url};
use serde::Deserialize;

use crate::client::OssClient;
use crate::error::{OssError, Result};
use crate::sign;

/// 对象元信息(HEAD 返回)。
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub content_length: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// 一次对象请求的请求体及其相关头。get/delete/head 用 [`Payload::default`]。
#[derive(Default)]
struct Payload<'a> {
    content_type: Option<&'a str>,
    content_md5: Option<&'a str>,
    body: Option<Bytes>,
}

/// OSS 错误响应体(XML)。
#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "RequestId")]
    request_id: Option<String>,
}

impl OssClient {
    /// 上传一个对象。`content_type` 缺省时传 `None`。
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: impl Into<Bytes>,
        content_type: Option<&str>,
    ) -> Result<()> {
        let body: Bytes = body.into();
        let content_md5 = cloud_core::crypto::content_md5(&body);
        let date = now_gmt();
        let request = self.build_signed_request(
            Method::PUT,
            bucket,
            key,
            Payload {
                content_type,
                content_md5: Some(&content_md5),
                body: Some(body),
            },
            &date,
        )?;
        let resp = self.http().execute(request).await?;
        check_status(resp).await?;
        Ok(())
    }

    /// 下载一个对象,返回完整字节。
    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Bytes> {
        let date = now_gmt();
        let request =
            self.build_signed_request(Method::GET, bucket, key, Payload::default(), &date)?;
        let resp = self.http().execute(request).await?;
        let resp = check_status(resp).await?;
        Ok(resp.bytes().await.map_err(cloud_core::CoreError::from)?)
    }

    /// 流式下载一个对象,返回 `(内容长度, 分块字节流)`,用于边下边写并报进度。
    pub async fn get_object_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(Option<u64>, impl futures::Stream<Item = Result<Bytes>>)> {
        let date = now_gmt();
        let request =
            self.build_signed_request(Method::GET, bucket, key, Payload::default(), &date)?;
        let resp = self.http().execute(request).await?;
        let resp = check_status(resp).await?;
        let len = resp.content_length();
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(|e| OssError::Core(cloud_core::CoreError::from(e))));
        Ok((len, stream))
    }

    /// 删除一个对象。对象不存在时 OSS 也返回 204,视为成功。
    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let date = now_gmt();
        let request =
            self.build_signed_request(Method::DELETE, bucket, key, Payload::default(), &date)?;
        let resp = self.http().execute(request).await?;
        check_status(resp).await?;
        Ok(())
    }

    /// 服务端复制对象(支持同桶 / 跨桶),无需下载再上传。
    pub async fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<()> {
        let date = now_gmt();
        let request = self.build_copy_request(src_bucket, src_key, dst_bucket, dst_key, &date)?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 组装并签名一次 CopyObject 请求。抽出 `date` 便于确定性测试。
    fn build_copy_request(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
        date: &str,
    ) -> Result<Request> {
        let copy_source = format!("/{src_bucket}/{src_key}");
        // x-oss-copy-source 属于 x-oss- 头,需计入 CanonicalizedOSSHeaders。
        let oss_headers =
            sign::canonicalized_oss_headers([("x-oss-copy-source", copy_source.as_str())]);
        let canonical = format!("/{dst_bucket}/{dst_key}");
        let sts = sign::string_to_sign("PUT", "", "", date, &oss_headers, &canonical);
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);

        let url = format!("{}/{}", self.bucket_base_url(dst_bucket), dst_key);
        self.http()
            .inner()
            .request(Method::PUT, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .header("x-oss-copy-source", &copy_source)
            .build()
            .map_err(cloud_core::CoreError::from)
            .map_err(OssError::from)
    }

    /// 生成一个 GET 预签名 URL,`expires_in` 秒后失效。纯本地签名,不发请求。
    pub fn presign_get(&self, bucket: &str, key: &str, expires_in: u64) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| OssError::Core(cloud_core::CoreError::Signature(e.to_string())))?
            .as_secs();
        self.build_presigned_url(bucket, key, now + expires_in)
    }

    /// 用绝对过期时间戳构造预签名 URL。抽出 `expiration` 便于确定性测试。
    fn build_presigned_url(&self, bucket: &str, key: &str, expiration: u64) -> Result<String> {
        // 预签名的 StringToSign 用 Expires 顶替 Date 那一行。
        let canonical = format!("/{bucket}/{key}");
        let sts = sign::string_to_sign("GET", "", "", &expiration.to_string(), "", &canonical);
        let signature = sign::signature(self.access_key_secret(), &sts);

        let mut url = Url::parse(&format!("{}/{}", self.bucket_base_url(bucket), key))
            .map_err(|e| OssError::Core(cloud_core::CoreError::InvalidRequest(e.to_string())))?;
        url.query_pairs_mut()
            .append_pair("OSSAccessKeyId", self.access_key_id())
            .append_pair("Expires", &expiration.to_string())
            .append_pair("Signature", &signature);
        Ok(url.to_string())
    }

    /// 读取对象元信息(HEAD)。
    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta> {
        let date = now_gmt();
        let request =
            self.build_signed_request(Method::HEAD, bucket, key, Payload::default(), &date)?;
        let resp = self.http().execute(request).await?;
        let resp = check_status(resp).await?;

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

    /// 组装并签名一个对象请求。抽出 `date` 参数便于确定性测试。
    fn build_signed_request(
        &self,
        method: Method,
        bucket: &str,
        key: &str,
        payload: Payload<'_>,
        date: &str,
    ) -> Result<Request> {
        let canonical_resource = format!("/{bucket}/{key}");
        let sts = sign::string_to_sign(
            method.as_str(),
            payload.content_md5.unwrap_or(""),
            payload.content_type.unwrap_or(""),
            date,
            "", // 基础对象操作暂无 x-oss-* 头
            &canonical_resource,
        );
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);

        let url = format!("{}/{}", self.bucket_base_url(bucket), key);
        let mut builder = self
            .http()
            .inner()
            .request(method, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization);
        if let Some(ct) = payload.content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        if let Some(md5) = payload.content_md5 {
            builder = builder.header("Content-MD5", md5);
        }
        if let Some(body) = payload.body {
            builder = builder.body(body);
        }
        builder
            .build()
            .map_err(cloud_core::CoreError::from)
            .map_err(OssError::from)
    }
}

/// 当前时间的 HTTP GMT 格式,如 `Thu, 17 Nov 2005 18:49:58 GMT`。
pub(crate) fn now_gmt() -> String {
    httpdate::fmt_http_date(SystemTime::now())
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

/// 成功(2xx)原样返回;否则读取 OSS Error XML 转成 [`OssError::Api`]。
pub(crate) async fn check_status(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    match quick_xml::de::from_str::<ErrorBody>(&body) {
        Ok(err) => Err(OssError::Api {
            status: code,
            code: err.code,
            message: err.message,
            request_id: err.request_id,
        }),
        Err(_) => Err(OssError::Api {
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

    fn test_client() -> OssClient {
        OssClient::new(
            "44CF9590006BF252F707",
            "OtxrzxIsfpFjA7SwPzILwy8Bw21TLhquhboDYROV",
            "oss-cn-hangzhou.aliyuncs.com",
        )
    }

    #[test]
    fn get_request_is_signed_correctly() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_signed_request(
                Method::GET,
                "oss-example",
                "nelson",
                Payload::default(),
                date,
            )
            .unwrap();

        // URL 用 bucket 虚拟托管域名。
        assert_eq!(
            req.url().as_str(),
            "https://oss-example.oss-cn-hangzhou.aliyuncs.com/nelson"
        );

        // Authorization 应与手工按同样输入计算的签名一致。
        let sts = sign::string_to_sign("GET", "", "", date, "", "/oss-example/nelson");
        let expected =
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts);
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );
        assert_eq!(req.headers().get(DATE).unwrap().to_str().unwrap(), date);
    }

    #[test]
    fn put_request_includes_content_md5() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let body = Bytes::from_static(b"hello");
        let md5 = cloud_core::crypto::content_md5(&body);
        let req = client
            .build_signed_request(
                Method::PUT,
                "b",
                "k",
                Payload {
                    content_type: Some("text/plain"),
                    content_md5: Some(&md5),
                    body: Some(body),
                },
                date,
            )
            .unwrap();

        assert_eq!(req.method(), Method::PUT);
        assert_eq!(
            req.headers().get("Content-MD5").unwrap().to_str().unwrap(),
            md5
        );
        assert_eq!(
            req.headers().get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn copy_request_signs_copy_source_header() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_copy_request("srcb", "a.txt", "dstb", "b.txt", date)
            .unwrap();

        assert_eq!(req.method(), Method::PUT);
        assert_eq!(
            req.url().as_str(),
            "https://dstb.oss-cn-hangzhou.aliyuncs.com/b.txt"
        );
        assert_eq!(
            req.headers()
                .get("x-oss-copy-source")
                .unwrap()
                .to_str()
                .unwrap(),
            "/srcb/a.txt"
        );

        // 签名需把 x-oss-copy-source 计入 CanonicalizedOSSHeaders,资源为目标对象。
        let oss_headers = sign::canonicalized_oss_headers([("x-oss-copy-source", "/srcb/a.txt")]);
        let sts = sign::string_to_sign("PUT", "", "", date, &oss_headers, "/dstb/b.txt");
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts)
        );
    }

    #[test]
    fn presign_builds_signed_query_url() {
        let client = test_client();
        let url = client
            .build_presigned_url("oss-example", "nelson", 1_234_567_890)
            .unwrap();

        assert!(url.starts_with("https://oss-example.oss-cn-hangzhou.aliyuncs.com/nelson?"));

        let parsed = Url::parse(&url).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(
            params.get("OSSAccessKeyId").unwrap(),
            "44CF9590006BF252F707"
        );
        assert_eq!(params.get("Expires").unwrap(), "1234567890");

        // 独立按预签名规则算出期望签名(Expires 顶替 Date)。
        let sts = sign::string_to_sign("GET", "", "", "1234567890", "", "/oss-example/nelson");
        let expected = sign::signature(client.access_key_secret(), &sts);
        assert_eq!(params.get("Signature").unwrap(), &expected);
    }

    #[test]
    fn parses_oss_error_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchKey</Code>
  <Message>The specified key does not exist.</Message>
  <RequestId>5E5D6A1B</RequestId>
</Error>"#;
        let parsed: ErrorBody = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.code, "NoSuchKey");
        assert_eq!(parsed.message, "The specified key does not exist.");
        assert_eq!(parsed.request_id.as_deref(), Some("5E5D6A1B"));
    }
}
