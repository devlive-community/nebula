//! OSS 客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端。
//!
//! 本增量只覆盖构造与 endpoint 拼接;对象 / 桶操作在后续增量加入。

use cloud_core::HttpClient;

/// 阿里云 OSS 客户端。可低成本 clone(内部 HTTP 客户端引用计数)。
#[derive(Debug, Clone)]
pub struct OssClient {
    access_key_id: String,
    access_key_secret: String,
    /// 区域 endpoint,如 `oss-cn-hangzhou.aliyuncs.com`(不含协议与 bucket)。
    endpoint: String,
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
            http: HttpClient::new(),
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
    /// `https://my-bucket.oss-cn-hangzhou.aliyuncs.com`。
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
        let c = OssClient::new("id", "secret", "https://oss-cn-hangzhou.aliyuncs.com/");
        assert_eq!(c.endpoint(), "oss-cn-hangzhou.aliyuncs.com");
    }

    #[test]
    fn accepts_bare_endpoint() {
        let c = OssClient::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com");
        assert_eq!(c.endpoint(), "oss-cn-hangzhou.aliyuncs.com");
    }

    #[test]
    fn builds_virtual_hosted_bucket_url() {
        let c = OssClient::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com");
        assert_eq!(
            c.bucket_base_url("my-bucket"),
            "https://my-bucket.oss-cn-hangzhou.aliyuncs.com"
        );
    }
}
