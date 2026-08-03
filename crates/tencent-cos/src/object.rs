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
    /// 跨区域访问时服务端给出的正确 endpoint(若有)。
    #[serde(rename = "Endpoint")]
    endpoint: Option<String>,
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

    /// 从 `offset` 字节开始流式下载(HTTP Range),返回 `(对象总大小, 剩余字节流)`。
    /// 用于断点续传;`offset == 0` 等价于 [`Self::get_object_stream`]。
    pub async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
    ) -> Result<(Option<u64>, impl futures::Stream<Item = Result<Bytes>>)> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let mut request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        // Range 不参与 COS 签名,建完请求后附加即可。
        request.headers_mut().insert(
            reqwest::header::RANGE,
            reqwest::header::HeaderValue::from_str(&format!("bytes={offset}-")).map_err(|e| {
                CosError::Core(cloud_core::CoreError::InvalidRequest(e.to_string()))
            })?,
        );
        let resp = check_status(self.http().execute(request).await?).await?;
        let total = total_size(&resp);
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(|e| CosError::Core(cloud_core::CoreError::from(e))));
        Ok((total, stream))
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

    /// 转换对象存储类型:带 `x-cos-storage-class` + `x-cos-metadata-directive: Copy` 自我复制。
    pub async fn set_storage_class(&self, bucket: &str, key: &str, class: &str) -> Result<()> {
        let copy_source = format!("{}/{}", self.bucket_host(bucket), encode_key(key));
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[
                ("x-cos-copy-source", copy_source),
                ("x-cos-storage-class", class.to_string()),
                ("x-cos-metadata-directive", "Copy".to_string()),
            ],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 修改内容类型:带新 `Content-Type` + `x-cos-metadata-directive: Replace` 的自我复制。
    pub async fn set_content_type(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
    ) -> Result<()> {
        let copy_source = format!("{}/{}", self.bucket_host(bucket), encode_key(key));
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[],
            content_type: Some(content_type),
            content_md5: None,
            cos_headers: &[
                ("x-cos-copy-source", copy_source),
                ("x-cos-metadata-directive", "Replace".to_string()),
            ],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 设置对象预置 ACL(`public-read` / `private`):`PUT /{key}?acl` + `x-cos-acl` 头。
    pub async fn set_object_acl(&self, bucket: &str, key: &str, acl: &str) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[("acl", None)],
            content_type: None,
            content_md5: None,
            cos_headers: &[("x-cos-acl", acl.to_string())],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 对象的永久公共直链(不签名),虚拟托管风格。仅当对象为公开读时可访问。
    pub fn public_url(&self, bucket: &str, key: &str) -> String {
        format!("https://{}/{}", self.bucket_host(bucket), encode_key(key))
    }

    /// 取回归档对象:`POST /{key}?restore`,请求体指定保持天数与取回层级。
    pub async fn restore_object(&self, bucket: &str, key: &str, days: u32) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let body = format!(
            "<RestoreRequest><Days>{days}</Days>\
             <CASJobParameters><Tier>Standard</Tier></CASJobParameters></RestoreRequest>"
        );
        let request = self.build_signed(SignSpec {
            method: Method::POST,
            host: &host,
            uri_path: &uri,
            query: &[("restore", None)],
            content_type: Some("application/xml"),
            content_md5: None,
            cos_headers: &[],
            body: Some(Bytes::from(body)),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取对象标签:`GET /{key}?tagging`,解析 TagSet 为键值对。
    pub async fn get_object_tags(&self, bucket: &str, key: &str) -> Result<Vec<(String, String)>> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: &uri,
            query: &[("tagging", None)],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(cloud_core::CoreError::from)?;
        parse_tagging(&body)
    }

    /// 覆盖对象标签:`PUT /{key}?tagging`,请求体为整套 TagSet(空列表即清空)。
    pub async fn set_object_tags(
        &self,
        bucket: &str,
        key: &str,
        tags: &[(String, String)],
    ) -> Result<()> {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let body = build_tagging_xml(tags);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: &uri,
            query: &[("tagging", None)],
            content_type: Some("application/xml"),
            content_md5: None,
            cos_headers: &[],
            body: Some(Bytes::from(body)),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 生成一个 GET 预签名 URL,`expires_in` 秒后失效。纯本地签名,不发请求。
    pub fn presign_get(&self, bucket: &str, key: &str, expires_in: u64) -> Result<String> {
        Ok(self.build_presigned_url("get", bucket, key, expires_in, now_unix()))
    }

    /// 生成一个 PUT 预签名 URL(上传链接),`expires_in` 秒后失效。
    pub fn presign_put(&self, bucket: &str, key: &str, expires_in: u64) -> Result<String> {
        Ok(self.build_presigned_url("put", bucket, key, expires_in, now_unix()))
    }

    /// 用固定起始时间与方法构造预签名 URL,便于确定性测试。
    fn build_presigned_url(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        expires_in: u64,
        now: u64,
    ) -> String {
        let host = self.bucket_host(bucket);
        let uri = object_uri(key);
        let key_time = format!("{};{}", now, now + expires_in);
        // 预签名只签 host 头、无参数。
        let (header_list, header_string) = sign::canonical(&[("host", &host)]);
        let http = sign::http_string(method, &uri, "", &header_string);
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

/// 从响应推断对象总大小:优先 `Content-Range` 的 `/{total}`,否则退回 `Content-Length`。
pub(crate) fn total_size(resp: &Response) -> Option<u64> {
    if let Some(total) = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit('/').next())
        .and_then(|t| t.trim().parse::<u64>().ok())
    {
        return Some(total);
    }
    resp.content_length()
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
            endpoint: err.endpoint,
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
            endpoint: None,
        }),
    }
}

fn fallback_code(status: StatusCode) -> String {
    status
        .canonical_reason()
        .unwrap_or("UnknownError")
        .replace(' ', "")
}

/// `GetObjectTagging` 响应体(XML)。
#[derive(Debug, Deserialize)]
struct Tagging {
    #[serde(rename = "TagSet", default)]
    tag_set: TagSet,
}

#[derive(Debug, Default, Deserialize)]
struct TagSet {
    #[serde(rename = "Tag", default)]
    tags: Vec<TagEntry>,
}

#[derive(Debug, Deserialize)]
struct TagEntry {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "Value", default)]
    value: String,
}

/// 解析对象标签的 XML 响应为键值对(无标签时为空)。
fn parse_tagging(xml: &str) -> Result<Vec<(String, String)>> {
    let doc: Tagging = quick_xml::de::from_str(xml)
        .map_err(|e| CosError::Core(cloud_core::CoreError::InvalidResponse(e.to_string())))?;
    Ok(doc
        .tag_set
        .tags
        .into_iter()
        .map(|t| (t.key, t.value))
        .collect())
}

/// 生成 PutObjectTagging 的请求体 XML(空列表 → 空 TagSet)。
fn build_tagging_xml(tags: &[(String, String)]) -> String {
    let mut body = String::from("<Tagging><TagSet>");
    for (k, v) in tags {
        body.push_str("<Tag><Key>");
        body.push_str(&xml_escape(k));
        body.push_str("</Key><Value>");
        body.push_str(&xml_escape(v));
        body.push_str("</Value></Tag>");
    }
    body.push_str("</TagSet></Tagging>");
    body
}

/// 转义 XML 文本中的保留字符,避免键 / 值里的 `&<>"'` 破坏文档。
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
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
        let url = c.build_presigned_url("get", "bkt-123", "exampleobject", 3600, 1_600_000_000);
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
