//! Kodo(S3 兼容)客户端:持有凭证、区域 endpoint 与共享 HTTP 客户端,
//! 并负责给每个请求做 AWS SigV4 签名。

use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use cloud_core::{CoreError, HttpClient};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::{Method, Request};

use crate::error::{KodoError, Result};
use crate::sign;

/// RFC3986 unreserved 之外全部编码(查询串用;会编码 `/`)。
const QUERY_ENC: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// 路径编码集:除 unreserved 与 `/` 外全部编码(对象 key 用,保留路径分隔符)。
const PATH_ENC: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// SigV4 查询参数编码(每个键 / 值单独编码)。
pub(crate) fn uri_encode(s: &str) -> String {
    utf8_percent_encode(s, QUERY_ENC).to_string()
}

/// 对象 key 的路径编码(保留 `/`)。
pub(crate) fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, PATH_ENC).to_string()
}

/// 一次待签名请求的描述。
pub(crate) struct RequestSpec<'a> {
    pub method: Method,
    /// 路径风格的 canonical URI(已对 key 做路径编码),如 `/bucket/dir/a.txt`、`/bucket`、`/`。
    pub canonical_uri: &'a str,
    /// 未编码的查询参数(内部会排序 + 编码)。
    pub query: &'a [(String, String)],
    pub content_type: Option<&'a str>,
    /// 额外需签名的 `x-amz-*` 头(小写名),如 `x-amz-copy-source`。
    pub amz_headers: &'a [(&'a str, String)],
    pub body: Option<Bytes>,
}

/// 七牛云 Kodo 客户端(S3 兼容)。可低成本 clone。
#[derive(Debug, Clone)]
pub struct KodoClient {
    access_key: String,
    secret_key: String,
    /// 区域 endpoint,如 `s3.cn-east-1.qiniucs.com`(不含协议)。
    endpoint: String,
    /// 从 endpoint 解析出的 region,如 `cn-east-1`。
    region: String,
    http: HttpClient,
}

impl KodoClient {
    /// 用凭证与区域 endpoint 创建客户端。
    ///
    /// `endpoint` 可带或不带协议前缀;region 从 `s3.{region}.qiniucs.com` 解析,
    /// 解析不出时回退为空串(签名 scope 会用空 region,通常会被服务端拒绝)。
    pub fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let endpoint = normalize_endpoint(&endpoint.into());
        let region = parse_region(&endpoint);
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
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
    pub fn region(&self) -> &str {
        &self.region
    }
    pub fn http(&self) -> &HttpClient {
        &self.http
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
        let (amz_date, date) = amz_datetime(now);

        let payload_hash = match &spec.body {
            Some(b) => cloud_core::crypto::sha256_hex(b),
            None => sign::EMPTY_PAYLOAD_HASH.to_string(),
        };

        // 待签名头:host / x-amz-content-sha256 / x-amz-date(+ content-type)。按名排序。
        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), self.endpoint.clone()),
            ("x-amz-content-sha256".into(), payload_hash.clone()),
            ("x-amz-date".into(), amz_date.clone()),
        ];
        if let Some(ct) = spec.content_type {
            headers.push(("content-type".into(), ct.to_string()));
        }
        for (k, v) in spec.amz_headers {
            headers.push((k.to_ascii_lowercase(), v.clone()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
        let signed_headers = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_query = canonical_query_string(spec.query);

        let cr = sign::canonical_request(
            spec.method.as_str(),
            spec.canonical_uri,
            &canonical_query,
            &canonical_headers,
            &signed_headers,
            &payload_hash,
        );
        let scope = sign::credential_scope(&date, &self.region, sign::SERVICE);
        let sts = sign::string_to_sign(&amz_date, &scope, &cr);
        let key = sign::signing_key(&self.secret_key, &date, &self.region, sign::SERVICE);
        let signature = sign::signature(&key, &sts);
        let authorization =
            sign::authorization(&self.access_key, &scope, &signed_headers, &signature);

        // URL = https://{endpoint}{canonical_uri}[?{canonical_query}]
        let mut url = format!("https://{}{}", self.endpoint, spec.canonical_uri);
        if !canonical_query.is_empty() {
            url.push('?');
            url.push_str(&canonical_query);
        }

        let mut builder = self
            .http()
            .inner()
            .request(spec.method, &url)
            .header("x-amz-date", &amz_date)
            .header("x-amz-content-sha256", &payload_hash)
            .header(reqwest::header::AUTHORIZATION, authorization);
        if let Some(ct) = spec.content_type {
            builder = builder.header(reqwest::header::CONTENT_TYPE, ct);
        }
        for (k, v) in spec.amz_headers {
            builder = builder.header(*k, v);
        }
        if let Some(body) = spec.body {
            builder = builder.body(body);
        }
        builder
            .build()
            .map_err(CoreError::from)
            .map_err(KodoError::from)
    }
}

/// 把查询参数按 SigV4 规则规范化:每个键 / 值单独编码,按编码后的键排序,`k=v` 用 `&` 连。
pub(crate) fn canonical_query_string(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// 去掉协议前缀与尾部斜杠。
fn normalize_endpoint(endpoint: &str) -> String {
    endpoint
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

/// 从 `s3.{region}.qiniucs.com` 解析 region;解析不出返回空串。
fn parse_region(endpoint: &str) -> String {
    endpoint
        .strip_prefix("s3.")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string()
}

/// 当前时间格式化为 SigV4 的 `(YYYYMMDDTHHMMSSZ, YYYYMMDD)`。
pub(crate) fn amz_datetime(t: SystemTime) -> (String, String) {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let date = format!("{y:04}{m:02}{d:02}");
    (format!("{date}T{hh:02}{mm:02}{ss:02}Z"), date)
}

/// Howard Hinnant 的 days→civil 算法(把 unix 天数转成年月日)。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> KodoClient {
        KodoClient::new(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "s3.cn-east-1.qiniucs.com",
        )
    }

    #[test]
    fn parses_region_and_normalizes_endpoint() {
        let c = KodoClient::new("a", "b", "https://s3.cn-east-1.qiniucs.com/");
        assert_eq!(c.endpoint(), "s3.cn-east-1.qiniucs.com");
        assert_eq!(c.region(), "cn-east-1");
    }

    #[test]
    fn amz_datetime_known_values() {
        assert_eq!(
            amz_datetime(UNIX_EPOCH),
            ("19700101T000000Z".into(), "19700101".into())
        );
        // 2001-09-09T01:46:40Z(经典 1e9 秒)。
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        assert_eq!(
            amz_datetime(t),
            ("20010909T014640Z".into(), "20010909".into())
        );
    }

    #[test]
    fn encode_key_keeps_slash_encodes_specials() {
        assert_eq!(encode_key("dir/a.txt"), "dir/a.txt");
        assert_eq!(encode_key("dir/ b.txt"), "dir/%20b.txt");
        assert_eq!(encode_key("图片.png"), "%E5%9B%BE%E7%89%87.png");
    }

    #[test]
    fn canonical_query_sorts_and_encodes() {
        let q = vec![
            ("prefix".into(), "photos/".into()),
            ("list-type".into(), "2".into()),
        ];
        assert_eq!(canonical_query_string(&q), "list-type=2&prefix=photos%2F");
    }

    #[test]
    fn get_object_request_is_signed_and_addressed_path_style() {
        let c = client();
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_440_938_160);
        let req = c
            .build_signed_at(
                RequestSpec {
                    method: Method::GET,
                    canonical_uri: "/mybucket/hello.txt",
                    query: &[],
                    content_type: None,
                    amz_headers: &[],
                    body: None,
                },
                t,
            )
            .unwrap();

        // 路径风格 URL。
        assert_eq!(
            req.url().as_str(),
            "https://s3.cn-east-1.qiniucs.com/mybucket/hello.txt"
        );
        // 必带 SigV4 头。
        let (amz_date, date) = amz_datetime(t);
        assert_eq!(
            req.headers().get("x-amz-date").unwrap().to_str().unwrap(),
            amz_date
        );
        assert_eq!(
            req.headers()
                .get("x-amz-content-sha256")
                .unwrap()
                .to_str()
                .unwrap(),
            sign::EMPTY_PAYLOAD_HASH
        );

        // 独立复算 Authorization,验证签名整链一致。
        let ch = format!(
            "host:s3.cn-east-1.qiniucs.com\nx-amz-content-sha256:{}\nx-amz-date:{amz_date}\n",
            sign::EMPTY_PAYLOAD_HASH
        );
        let sh = "host;x-amz-content-sha256;x-amz-date";
        let cr = sign::canonical_request(
            "GET",
            "/mybucket/hello.txt",
            "",
            &ch,
            sh,
            sign::EMPTY_PAYLOAD_HASH,
        );
        let scope = sign::credential_scope(&date, "cn-east-1", "s3");
        let sts = sign::string_to_sign(&amz_date, &scope, &cr);
        let key = sign::signing_key(c.secret_key(), &date, "cn-east-1", "s3");
        let expected =
            sign::authorization(c.access_key(), &scope, sh, &sign::signature(&key, &sts));
        assert_eq!(
            req.headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            expected
        );
    }
}
