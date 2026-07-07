//! OBS 客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端。
//!
//! 本增量只覆盖构造与 endpoint 拼接;对象 / 桶操作在后续增量加入。

use cloud_core::HttpClient;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// 对象 key 放进 URL path 时的百分号编码集:除 `A-Za-z0-9 - _ . ~ /` 外全部编码。
/// 保留 `/` 作为路径分隔符;签名用的 CanonicalizedResource 仍用未编码的原始 key。
const OBS_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// 把对象 key 编码成可安全放进 URL path 的形式(保留 `/`)。
pub(crate) fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, OBS_PATH).to_string()
}

/// 华为云 OBS 客户端。可低成本 clone(内部 HTTP 客户端引用计数)。
#[derive(Debug, Clone)]
pub struct ObsClient {
    access_key: String,
    secret_key: String,
    /// 区域 endpoint,如 `obs.cn-north-4.myhuaweicloud.com`(不含协议与 bucket)。
    endpoint: String,
    http: HttpClient,
}

impl ObsClient {
    /// 用凭证与区域 endpoint 创建客户端。
    ///
    /// `endpoint` 可带或不带协议前缀,内部统一去掉;例如
    /// `obs.cn-north-4.myhuaweicloud.com` 或 `https://obs.cn-north-4.myhuaweicloud.com`。
    pub fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            endpoint: normalize_endpoint(&endpoint.into()),
            http: HttpClient::new(),
        }
    }

    /// 复用调用方自定义的 [`HttpClient`](如需代理 / 自定义 TLS)。
    pub fn with_http_client(mut self, http: HttpClient) -> Self {
        self.http = http;
        self
    }

    /// AccessKey(AK)。
    pub fn access_key(&self) -> &str {
        &self.access_key
    }

    /// SecretKey(SK)。
    pub fn secret_key(&self) -> &str {
        &self.secret_key
    }

    /// 区域 endpoint(已去掉协议前缀)。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 共享 HTTP 客户端。
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// 从 endpoint 解析区域,如 `obs.cn-north-4.myhuaweicloud.com` → `cn-north-4`。
    ///
    /// 非默认区域的建桶请求需要在请求体里带该区域(见 `bucket.rs`)。无法解析时返回 `None`。
    pub fn region(&self) -> Option<&str> {
        self.endpoint
            .strip_prefix("obs.")
            .and_then(|rest| rest.split('.').next())
            .filter(|r| !r.is_empty())
    }

    /// 拼出某个 bucket 的虚拟托管域名根 URL,如
    /// `https://my-bucket.obs.cn-north-4.myhuaweicloud.com`。
    pub fn bucket_base_url(&self, bucket: &str) -> String {
        format!("https://{bucket}.{}", self.endpoint)
    }
}

/// 去掉 endpoint 上可能存在的协议前缀与尾部斜杠。
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

    #[test]
    fn strips_scheme_and_trailing_slash() {
        let c = ObsClient::new("id", "secret", "https://obs.cn-north-4.myhuaweicloud.com/");
        assert_eq!(c.endpoint(), "obs.cn-north-4.myhuaweicloud.com");
    }

    #[test]
    fn accepts_bare_endpoint() {
        let c = ObsClient::new("id", "secret", "obs.cn-north-4.myhuaweicloud.com");
        assert_eq!(c.endpoint(), "obs.cn-north-4.myhuaweicloud.com");
    }

    #[test]
    fn parses_region_from_endpoint() {
        let c = ObsClient::new("id", "secret", "obs.cn-north-4.myhuaweicloud.com");
        assert_eq!(c.region(), Some("cn-north-4"));

        // 非 obs. 前缀的自定义域名解析不出区域。
        let custom = ObsClient::new("id", "secret", "files.example.com");
        assert_eq!(custom.region(), None);
    }

    #[test]
    fn encode_key_percent_encodes_specials_but_keeps_slash() {
        assert_eq!(encode_key("dir/a.mp4"), "dir/a.mp4");
        assert_eq!(encode_key("dir/ .mp4"), "dir/%20.mp4"); // 前导空格保留为 %20
        assert_eq!(encode_key("a b+c#d"), "a%20b%2Bc%23d");
        assert_eq!(encode_key("图片.png"), "%E5%9B%BE%E7%89%87.png");
    }

    #[test]
    fn builds_virtual_hosted_bucket_url() {
        let c = ObsClient::new("id", "secret", "obs.cn-north-4.myhuaweicloud.com");
        assert_eq!(
            c.bucket_base_url("my-bucket"),
            "https://my-bucket.obs.cn-north-4.myhuaweicloud.com"
        );
    }
}
