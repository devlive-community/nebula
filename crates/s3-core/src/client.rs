//! Kodo(S3 兼容)客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端。
//!
//! SigV4 签名与请求组装复用共享的 [`s3_sigv4`] crate;本文件只做 endpoint 规整、
//! region 解析,以及把签名委托给 `s3_sigv4`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use cloud_core::HttpClient;
use reqwest::{Request, Response};
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
    /// 每个桶实际所在 region 的缓存(AWS 各桶按 region 分 endpoint)。首次访问跨区域桶时
    /// 从 `x-amz-bucket-region` 响应头学习并缓存,之后该桶的请求路由 + 签名到正确区域。
    bucket_regions: Arc<Mutex<HashMap<String, String>>>,
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
            bucket_regions: Arc::new(Mutex::new(HashMap::new())),
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

    /// 用给定 endpoint / region 构造签名参数(S3 service)。
    pub(crate) fn params_with<'a>(
        &'a self,
        endpoint: &'a str,
        region: &'a str,
    ) -> SigningParams<'a> {
        SigningParams {
            access_key: &self.access_key,
            secret_key: &self.secret_key,
            scheme: &self.scheme,
            endpoint,
            region,
            service: s3_sigv4::sign::S3,
        }
    }

    /// 某个 canonical URI(路径风格,首段是桶名)应路由到的 `(endpoint, region)`:
    /// 有缓存用缓存的桶区域并据此推导区域化 endpoint,否则用账号默认区域。
    pub(crate) fn route(&self, canonical_uri: &str) -> (String, String) {
        let region = bucket_from_uri(canonical_uri)
            .and_then(|b| self.bucket_regions.lock().unwrap().get(b).cloned())
            .unwrap_or_else(|| self.region.clone());
        (regional_endpoint(&self.endpoint, &region), region)
    }

    /// 组装并做 SigV4 签名(按桶路由到其区域)。用当前时间。
    pub(crate) fn build_signed(&self, spec: RequestSpec<'_>) -> Result<Request> {
        self.build_signed_at(spec, SystemTime::now())
    }

    /// 同 [`Self::build_signed`],但传入固定时间,便于确定性测试。
    pub(crate) fn build_signed_at(
        &self,
        spec: RequestSpec<'_>,
        now: SystemTime,
    ) -> Result<Request> {
        let (endpoint, region) = self.route(spec.canonical_uri);
        let params = self.params_with(&endpoint, &region);
        s3_sigv4::build_signed_request(self.http(), &params, spec, now).map_err(S3Error::from)
    }

    /// 发送一个按桶路由的请求;若响应表明该桶在别的区域(`x-amz-bucket-region` 头),
    /// 缓存正确区域后重试一次。用于列举等「首次访问某桶」的操作,以自举区域缓存,
    /// 之后该桶的其它操作经 [`build_signed`](Self::build_signed) 自动路由到正确区域。
    pub(crate) async fn send(&self, spec: RequestSpec<'_>) -> Result<Response> {
        let (_, used_region) = self.route(spec.canonical_uri);
        let request = self.build_signed(spec.clone())?;
        let resp = self.http().execute(request).await?;
        if let (Some(bucket), Some(correct)) = (
            bucket_from_uri(spec.canonical_uri),
            wrong_bucket_region(&resp, &used_region),
        ) {
            self.bucket_regions
                .lock()
                .unwrap()
                .insert(bucket.to_string(), correct);
            let retry = self.build_signed(spec)?;
            return Ok(self.http().execute(retry).await?);
        }
        Ok(resp)
    }
}

/// 取路径风格 canonical URI 的首段作为桶名(`/bucket/key` → `bucket`;`/` → None)。
fn bucket_from_uri(canonical_uri: &str) -> Option<&str> {
    let seg = canonical_uri.trim_start_matches('/');
    let seg = seg.split('/').next().unwrap_or("");
    let seg = seg.split('?').next().unwrap_or("");
    if seg.is_empty() {
        None
    } else {
        Some(seg)
    }
}

/// 据桶所在 region 推导应使用的 endpoint。仅对 AWS(`*.amazonaws.com`)按
/// `s3.{region}.amazonaws.com` 改写;其它厂商(单一 endpoint)原样返回。
fn regional_endpoint(base: &str, region: &str) -> String {
    if region.is_empty() || !base.ends_with(".amazonaws.com") {
        base.to_string()
    } else {
        format!("s3.{region}.amazonaws.com")
    }
}

/// 若响应表明桶在别的区域,返回正确区域,用于跨区域重定向的自动纠正。
///
/// 只在跨区域会出现的两种状态才判断(避免对普通 404/403 误重试):
/// `301 PermanentRedirect`(endpoint 不对)与 `400 AuthorizationHeaderMalformed`(签名区域不对),
/// 且响应头 `x-amz-bucket-region` 指向了与本次所用不同的区域。
fn wrong_bucket_region(resp: &Response, used_region: &str) -> Option<String> {
    let status = resp.status().as_u16();
    if status != 301 && status != 400 {
        return None;
    }
    let region = resp
        .headers()
        .get("x-amz-bucket-region")
        .and_then(|v| v.to_str().ok())?;
    if region.is_empty() || region == used_region {
        None
    } else {
        Some(region.to_string())
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
    fn bucket_from_uri_extracts_first_segment() {
        assert_eq!(bucket_from_uri("/mybucket/dir/a.txt"), Some("mybucket"));
        assert_eq!(bucket_from_uri("/mybucket"), Some("mybucket"));
        assert_eq!(bucket_from_uri("/"), None);
        assert_eq!(bucket_from_uri(""), None);
    }

    #[test]
    fn regional_endpoint_only_remaps_aws_hosts() {
        // AWS:按桶区域改写。
        assert_eq!(
            regional_endpoint("s3.us-east-1.amazonaws.com", "eu-west-1"),
            "s3.eu-west-1.amazonaws.com"
        );
        assert_eq!(
            regional_endpoint("s3.amazonaws.com", "ap-southeast-2"),
            "s3.ap-southeast-2.amazonaws.com"
        );
        // 非 AWS(MinIO / R2 / 七牛)与空 region:原样返回。
        assert_eq!(
            regional_endpoint("minio.local:9000", "us-east-1"),
            "minio.local:9000"
        );
        assert_eq!(
            regional_endpoint("s3.cn-east-1.qiniucs.com", "cn-north-1"),
            "s3.cn-east-1.qiniucs.com"
        );
        assert_eq!(
            regional_endpoint("s3.us-east-1.amazonaws.com", ""),
            "s3.us-east-1.amazonaws.com"
        );
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
