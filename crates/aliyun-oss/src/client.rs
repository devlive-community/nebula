//! OSS 客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端。
//!
//! 本增量只覆盖构造与 endpoint 拼接;对象 / 桶操作在后续增量加入。

use cloud_core::HttpClient;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// 对象 key 放进 URL path 时的百分号编码集:除 `A-Za-z0-9 - _ . ~ /` 外全部编码。
/// 保留 `/` 作为路径分隔符;签名用的 CanonicalizedResource 仍用未编码的原始 key。
const OSS_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// 把对象 key 编码成可安全放进 URL path 的形式(保留 `/`)。
pub(crate) fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, OSS_PATH).to_string()
}

/// 阿里云 OSS 客户端。可低成本 clone(内部 HTTP 客户端引用计数)。
#[derive(Debug, Clone)]
pub struct OssClient {
    access_key_id: String,
    access_key_secret: String,
    /// 区域 endpoint,如 `oss-cn-hangzhou.aliyuncs.com`(不含协议与 bucket)。
    endpoint: String,
    /// 每个桶实际所在区域的 endpoint(OSS 各桶按区域分 endpoint,访问必须用对应 endpoint)。
    /// 列举桶时从响应的 ExtranetEndpoint / Location 填充,之后该桶请求路由到正确 endpoint。
    /// V2 签名与 endpoint 无关,故只需改 URL host、无需重新签名。
    bucket_endpoints: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    http: HttpClient,
}

impl OssClient {
    /// 用凭证与区域 endpoint 创建客户端。
    ///
    /// `endpoint` 可带或不带协议前缀,内部统一去掉;例如
    /// `oss-cn-hangzhou.aliyuncs.com` 或 `https://oss-cn-hangzhou.aliyuncs.com`。
    pub fn new(
        access_key_id: impl Into<String>,
        access_key_secret: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            access_key_secret: access_key_secret.into(),
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

    /// 复用调用方自定义的 [`HttpClient`](如需代理 / 自定义 TLS)。
    pub fn with_http_client(mut self, http: HttpClient) -> Self {
        self.http = http;
        self
    }

    /// AccessKeyId。
    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    /// AccessKeySecret。
    pub fn access_key_secret(&self) -> &str {
        &self.access_key_secret
    }

    /// 区域 endpoint(已去掉协议前缀)。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 共享 HTTP 客户端。
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// 拼出某个 bucket 的虚拟托管域名根 URL,如
    /// `https://my-bucket.oss-cn-hangzhou.aliyuncs.com`。有缓存则用桶自己的区域 endpoint。
    pub fn bucket_base_url(&self, bucket: &str) -> String {
        let endpoint = self
            .bucket_endpoints
            .lock()
            .unwrap()
            .get(bucket)
            .cloned()
            .unwrap_or_else(|| self.endpoint.clone());
        format!("https://{bucket}.{endpoint}")
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
        let c = OssClient::new("id", "secret", "https://oss-cn-hangzhou.aliyuncs.com/");
        assert_eq!(c.endpoint(), "oss-cn-hangzhou.aliyuncs.com");
    }

    #[test]
    fn accepts_bare_endpoint() {
        let c = OssClient::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com");
        assert_eq!(c.endpoint(), "oss-cn-hangzhou.aliyuncs.com");
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
        let c = OssClient::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com");
        assert_eq!(
            c.bucket_base_url("my-bucket"),
            "https://my-bucket.oss-cn-hangzhou.aliyuncs.com"
        );
    }

    #[test]
    fn bucket_base_url_uses_cached_region_endpoint() {
        let c = OssClient::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com");
        // 未缓存 → 账号默认 endpoint。
        assert_eq!(
            c.bucket_base_url("b"),
            "https://b.oss-cn-hangzhou.aliyuncs.com"
        );
        // 缓存桶所在区域后 → 路由到该区域 endpoint(去协议前缀)。
        c.cache_bucket_endpoint("b", "https://oss-cn-beijing.aliyuncs.com");
        assert_eq!(
            c.bucket_base_url("b"),
            "https://b.oss-cn-beijing.aliyuncs.com"
        );
    }
}
