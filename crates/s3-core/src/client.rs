//! Kodo(S3 兼容)客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端。
//!
//! SigV4 签名与请求组装复用共享的 [`s3_sigv4`] crate;本文件只做 endpoint 规整、
//! region 解析,以及把签名委托给 `s3_sigv4`。

use std::time::SystemTime;

use cloud_core::HttpClient;
use reqwest::Request;
use s3_sigv4::{RequestSpec, SigningParams};

use crate::error::{Result, S3Error};

/// 七牛云 Kodo 客户端(S3 兼容)。可低成本 clone。
#[derive(Debug, Clone)]
pub struct S3Client {
    access_key: String,
    secret_key: String,
    /// 协议,`https`(默认)或 `http`(endpoint 以 `http://` 开头时,如自建 MinIO)。
    scheme: String,
    /// 主机名(不含协议),可带端口,如 `s3.cn-east-1.qiniucs.com`、`minio.local:9000`。
    endpoint: String,
    /// 从 endpoint 解析出的 region,如 `cn-east-1`。
    region: String,
    http: HttpClient,
}

impl S3Client {
    /// 用凭证与 endpoint 创建客户端。
    ///
    /// endpoint 可带协议前缀:`http://` 走明文(自建 MinIO 等),否则默认 `https`;可带端口。
    /// region 从 `s3.{region}.*` 解析,解析不出为空(用 [`Self::with_region`] 显式设置)。
    pub fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let (scheme, endpoint) = parse_endpoint(&endpoint.into());
        let region = parse_region(&endpoint);
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            scheme,
            endpoint,
            region,
            http: HttpClient::new(),
        }
    }

    /// 复用调用方自定义的 [`HttpClient`]。
    pub fn with_http_client(mut self, http: HttpClient) -> Self {
        self.http = http;
        self
    }

    /// 显式设置 region(当 endpoint 非标准域名、无法解析时)。
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }

    pub fn access_key(&self) -> &str {
        &self.access_key
    }
    pub fn secret_key(&self) -> &str {
        &self.secret_key
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn scheme(&self) -> &str {
        &self.scheme
    }
    pub fn region(&self) -> &str {
        &self.region
    }
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// 构造签名参数(S3 service)。
    pub(crate) fn params(&self) -> SigningParams<'_> {
        SigningParams {
            access_key: &self.access_key,
            secret_key: &self.secret_key,
            scheme: &self.scheme,
            endpoint: &self.endpoint,
            region: &self.region,
            service: s3_sigv4::sign::S3,
        }
    }

    /// 组装并做 SigV4 签名,返回可直接发送的 [`Request`]。用当前时间。
    pub(crate) fn build_signed(&self, spec: RequestSpec<'_>) -> Result<Request> {
        self.build_signed_at(spec, SystemTime::now())
    }

    /// 同 [`Self::build_signed`],但传入固定时间,便于确定性测试。
    pub(crate) fn build_signed_at(
        &self,
        spec: RequestSpec<'_>,
        now: SystemTime,
    ) -> Result<Request> {
        s3_sigv4::build_signed_request(self.http(), &self.params(), spec, now)
            .map_err(S3Error::from)
    }
}

/// 解析 endpoint 为 `(scheme, host)`。`http://` 前缀 → 明文;否则默认 `https`。去尾部斜杠。
fn parse_endpoint(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("http://") {
        ("http".to_string(), rest.trim_end_matches('/').to_string())
    } else {
        let host = raw
            .strip_prefix("https://")
            .unwrap_or(raw)
            .trim_end_matches('/');
        ("https".to_string(), host.to_string())
    }
}

/// 从 `s3.{region}.qiniucs.com` 解析 region;解析不出返回空串。
fn parse_region(endpoint: &str) -> String {
    endpoint
        .strip_prefix("s3.")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Method;

    fn client() -> S3Client {
        S3Client::new(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "s3.cn-east-1.qiniucs.com",
        )
    }

    #[test]
    fn parses_region_and_normalizes_endpoint() {
        let c = S3Client::new("a", "b", "https://s3.cn-east-1.qiniucs.com/");
        assert_eq!(c.endpoint(), "s3.cn-east-1.qiniucs.com");
        assert_eq!(c.region(), "cn-east-1");
    }

    #[test]
    fn non_standard_endpoint_has_empty_region() {
        let c = S3Client::new("a", "b", "files.example.com");
        assert_eq!(c.region(), "");
        assert_eq!(c.with_region("cn-east-1").region(), "cn-east-1");
    }

    #[test]
    fn build_signed_addresses_path_style() {
        let c = client();
        let req = c
            .build_signed(RequestSpec {
                method: Method::GET,
                canonical_uri: "/mybucket/hello.txt",
                query: &[],
                content_type: None,
                amz_headers: &[],
                body: None,
            })
            .unwrap();
        assert_eq!(
            req.url().as_str(),
            "https://s3.cn-east-1.qiniucs.com/mybucket/hello.txt"
        );
        assert!(req
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
    }

    #[test]
    fn http_endpoint_with_port() {
        // 自建 MinIO 场景:http 明文 + 自定义端口。
        let c = S3Client::new("a", "b", "http://minio.local:9000").with_region("us-east-1");
        assert_eq!(c.scheme(), "http");
        assert_eq!(c.endpoint(), "minio.local:9000");
        let req = c
            .build_signed(RequestSpec {
                method: Method::GET,
                canonical_uri: "/bkt/k",
                query: &[],
                content_type: None,
                amz_headers: &[],
                body: None,
            })
            .unwrap();
        // URL 走 http 且带端口;签名的 host 头亦为 minio.local:9000。
        assert_eq!(req.url().as_str(), "http://minio.local:9000/bkt/k");
    }
}
