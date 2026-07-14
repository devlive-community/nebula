//! COS 客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端,并给每个请求做 COS 专有签名。

use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use cloud_core::{CoreError, HttpClient};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Method, Request};

use crate::error::{CosError, Result};
use crate::sign;

/// 对象 key 放进 URL path 的编码集:除 `A-Za-z0-9 - _ . ~ /` 外全部编码(保留 `/`)。
const COS_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// 把对象 key 编码成可放进 URL path 的形式(保留 `/`)。
pub(crate) fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, COS_PATH).to_string()
}

/// 一次待签名请求的描述。
pub(crate) struct SignSpec<'a> {
    pub method: Method,
    /// 主机名,如 `bkt-123.cos.ap-beijing.myqcloud.com` 或 `service.cos.myqcloud.com`。
    pub host: &'a str,
    /// 已编码的 URI path,如 `/exampleobject`、`/`。
    pub uri_path: &'a str,
    /// 查询参数(未编码);无值参数(如 `uploads`)用 `None`。
    pub query: &'a [(&'a str, Option<&'a str>)],
    pub content_type: Option<&'a str>,
    pub content_md5: Option<&'a str>,
    /// 额外需签名的 `x-cos-*` 头。
    pub cos_headers: &'a [(&'a str, String)],
    pub body: Option<Bytes>,
}

/// 腾讯云 COS 客户端。可低成本 clone。
#[derive(Debug, Clone)]
pub struct CosClient {
    secret_id: String,
    secret_key: String,
    /// 区域 endpoint,如 `cos.ap-beijing.myqcloud.com`(不含协议与 bucket)。
    endpoint: String,
    /// 每个桶实际所在区域的 endpoint(COS 各桶按区域分 endpoint)。列举桶时从 Location 填充,
    /// 之后该桶请求路由到正确区域(q-sign 会对所用 host 签名,故 host 一致即签名一致)。
    bucket_endpoints: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    http: HttpClient,
}

impl CosClient {
    /// 用凭证与区域 endpoint 创建客户端。`endpoint` 可带或不带协议前缀。
    pub fn new(
        secret_id: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            secret_id: secret_id.into(),
            secret_key: secret_key.into(),
            endpoint: normalize_endpoint(&endpoint.into()),
            bucket_endpoints: Default::default(),
            http: HttpClient::new(),
        }
    }

    /// 记录某个桶的真实 endpoint(列举桶时调用),之后该桶请求路由到此 endpoint。
    pub(crate) fn cache_bucket_endpoint(&self, bucket: &str, endpoint: &str) {
        if !endpoint.is_empty() {
            self.bucket_endpoints
                .lock()
                .unwrap()
                .insert(bucket.to_string(), normalize_endpoint(endpoint));
        }
    }

    /// 复用调用方自定义的 [`HttpClient`]。
    pub fn with_http_client(mut self, http: HttpClient) -> Self {
        self.http = http;
        self
    }

    pub fn secret_id(&self) -> &str {
        &self.secret_id
    }
    pub fn secret_key(&self) -> &str {
        &self.secret_key
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// 某个 bucket 的虚拟托管主机名,如 `bkt-123.cos.ap-beijing.myqcloud.com`。
    /// COS 的 bucket 名自带 appid(`{name}-{appid}`)。有缓存则用桶自己的区域 endpoint。
    pub fn bucket_host(&self, bucket: &str) -> String {
        let endpoint = self
            .bucket_endpoints
            .lock()
            .unwrap()
            .get(bucket)
            .cloned()
            .unwrap_or_else(|| self.endpoint.clone());
        format!("{bucket}.{endpoint}")
    }

    /// 列桶的 service 主机名。
    pub fn service_host(&self) -> &str {
        "service.cos.myqcloud.com"
    }

    /// 组装并做 COS 签名,返回可直接发送的 [`Request`]。用当前时间。
    pub(crate) fn build_signed(&self, spec: SignSpec<'_>) -> Result<Request> {
        self.build_signed_at(spec, now_unix())
    }

    /// 同 [`Self::build_signed`],但传入固定起始时间戳,便于确定性测试。
    pub(crate) fn build_signed_at(&self, spec: SignSpec<'_>, now: u64) -> Result<Request> {
        let key_time = format!("{};{}", now, now + 3600);

        // 参与签名的头:host 必签,content-type / content-md5(若有)与 x-cos-* 一并。
        let mut sign_headers: Vec<(&str, &str)> = vec![("host", spec.host)];
        if let Some(ct) = spec.content_type {
            sign_headers.push(("content-type", ct));
        }
        if let Some(md5) = spec.content_md5 {
            sign_headers.push(("content-md5", md5));
        }
        for (k, v) in spec.cos_headers {
            sign_headers.push((k, v));
        }
        let (header_list, header_string) = sign::canonical(&sign_headers);

        // 参与签名的查询参数(无值用空串)。
        let param_pairs: Vec<(&str, &str)> = spec
            .query
            .iter()
            .map(|(k, v)| (*k, v.unwrap_or("")))
            .collect();
        let (url_param_list, param_string) = sign::canonical(&param_pairs);

        let http_string = sign::http_string(
            spec.method.as_str(),
            spec.uri_path,
            &param_string,
            &header_string,
        );
        let sign_key = sign::sign_key(&self.secret_key, &key_time);
        let string_to_sign = sign::string_to_sign(&key_time, &http_string);
        let signature = sign::signature(&sign_key, &string_to_sign);
        let authorization = sign::authorization(
            &self.secret_id,
            &key_time,
            &header_list,
            &url_param_list,
            &signature,
        );

        // URL = https://{host}{uri_path}[?{param_string}](已编码,与签名一致)。
        let mut url = format!("https://{}{}", spec.host, spec.uri_path);
        if !param_string.is_empty() {
            url.push('?');
            url.push_str(&param_string);
        }

        let mut builder = self
            .http()
            .inner()
            .request(spec.method, &url)
            .header(AUTHORIZATION, authorization);
        if let Some(ct) = spec.content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        if let Some(md5) = spec.content_md5 {
            builder = builder.header("Content-MD5", md5);
        }
        for (k, v) in spec.cos_headers {
            builder = builder.header(*k, v);
        }
        if let Some(body) = spec.body {
            builder = builder.body(body);
        }
        builder
            .build()
            .map_err(CoreError::from)
            .map_err(CosError::from)
    }
}

/// 当前 unix 秒。
pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 去掉 endpoint 上的协议前缀与尾部斜杠。
fn normalize_endpoint(endpoint: &str) -> String {
    endpoint
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
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
    fn strips_scheme_and_builds_bucket_host() {
        let c = CosClient::new("i", "k", "https://cos.ap-beijing.myqcloud.com/");
        assert_eq!(c.endpoint(), "cos.ap-beijing.myqcloud.com");
        assert_eq!(
            c.bucket_host("bkt-123"),
            "bkt-123.cos.ap-beijing.myqcloud.com"
        );
    }

    #[test]
    fn bucket_host_uses_cached_region_endpoint() {
        let c = CosClient::new("i", "k", "cos.ap-beijing.myqcloud.com");
        c.cache_bucket_endpoint("bkt-123", "cos.ap-guangzhou.myqcloud.com");
        assert_eq!(
            c.bucket_host("bkt-123"),
            "bkt-123.cos.ap-guangzhou.myqcloud.com"
        );
    }

    #[test]
    fn encode_key_keeps_slash() {
        assert_eq!(encode_key("dir/a.txt"), "dir/a.txt");
        assert_eq!(encode_key("dir/ b.txt"), "dir/%20b.txt");
    }

    #[test]
    fn get_object_request_is_signed_and_addressed() {
        let c = client();
        let host = c.bucket_host("bkt-123");
        let req = c
            .build_signed_at(
                SignSpec {
                    method: Method::GET,
                    host: &host,
                    uri_path: "/exampleobject",
                    query: &[],
                    content_type: None,
                    content_md5: None,
                    cos_headers: &[],
                    body: None,
                },
                1_600_000_000,
            )
            .unwrap();

        assert_eq!(
            req.url().as_str(),
            "https://bkt-123.cos.ap-beijing.myqcloud.com/exampleobject"
        );

        // 独立复算 Authorization,验证整链一致。
        let key_time = "1600000000;1600003600";
        let (hl, hs) = sign::canonical(&[("host", &host)]);
        let http = sign::http_string("GET", "/exampleobject", "", &hs);
        let sk = sign::sign_key(c.secret_key(), key_time);
        let sts = sign::string_to_sign(key_time, &http);
        let expected = sign::authorization(
            c.secret_id(),
            key_time,
            &hl,
            "",
            &sign::signature(&sk, &sts),
        );
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );
    }
}
