//! # s3-sigv4
//!
//! 可复用的 **AWS Signature V4** 签名器,面向 **S3 兼容对象存储**(七牛 Kodo、AWS S3、
//! Cloudflare R2、MinIO 等)。提供:
//!
//! - [`sign`] — 纯 SigV4 签名函数(canonical request / string to sign / 派生密钥 / 签名)
//! - [`build_signed_request`] — 给定凭证 + endpoint + region,组装并签名一个路径风格的
//!   reqwest 请求(自动补 `host` / `x-amz-date` / `x-amz-content-sha256` / `Authorization`)
//! - [`presigned_get_url`] — 生成 SigV4 query 方式的 GET 预签名 URL
//!
//! 只依赖 [`cloud_core`] 与 reqwest,不感知任何具体厂商。签名正确性由 [`sign`] 中的
//! AWS 官方 `get-vanilla` 向量单测逐字节保证。

pub mod sign;

use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use cloud_core::{CoreError, HttpClient};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::{Method, Request};

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
pub fn uri_encode(s: &str) -> String {
    utf8_percent_encode(s, QUERY_ENC).to_string()
}

/// 对象 key 的路径编码(保留 `/`)。
pub fn encode_path(s: &str) -> String {
    utf8_percent_encode(s, PATH_ENC).to_string()
}

/// 把查询参数按 SigV4 规则规范化:每个键 / 值单独编码,按编码后的键排序,`k=v` 用 `&` 连。
pub fn canonical_query_string(query: &[(String, String)]) -> String {
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

/// 当前时间格式化为 SigV4 的 `(YYYYMMDDTHHMMSSZ, YYYYMMDD)`。
pub fn amz_datetime(t: SystemTime) -> (String, String) {
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

/// Howard Hinnant 的 days→civil 算法(把 unix 天数转成年月日),避免引入 chrono/time。
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

/// 签名所需的凭证与目标定位(借用,按需构造,开销极低)。
#[derive(Debug, Clone, Copy)]
pub struct SigningParams<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    /// 协议,`https` 或 `http`(MinIO 等自建服务可能用 http)。
    pub scheme: &'a str,
    /// 主机名(不含协议),可带端口,如 `s3.cn-east-1.qiniucs.com`、`minio.local:9000`。
    pub endpoint: &'a str,
    pub region: &'a str,
    /// service 名,S3 用 [`sign::S3`]。
    pub service: &'a str,
}

/// 一次待签名请求的描述(路径风格)。
pub struct RequestSpec<'a> {
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

/// 组装并做 SigV4 签名,返回可直接发送的 [`Request`]。`now` 便于确定性测试。
pub fn build_signed_request(
    http: &HttpClient,
    params: &SigningParams<'_>,
    spec: RequestSpec<'_>,
    now: SystemTime,
) -> Result<Request, CoreError> {
    let (amz_date, date) = amz_datetime(now);

    let payload_hash = match &spec.body {
        Some(b) => cloud_core::crypto::sha256_hex(b),
        None => sign::EMPTY_PAYLOAD_HASH.to_string(),
    };

    // 待签名头:host / x-amz-content-sha256 / x-amz-date(+ content-type + 额外 x-amz-*)。按名排序。
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), params.endpoint.to_string()),
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
    let scope = sign::credential_scope(&date, params.region, params.service);
    let sts = sign::string_to_sign(&amz_date, &scope, &cr);
    let key = sign::signing_key(params.secret_key, &date, params.region, params.service);
    let signature = sign::signature(&key, &sts);
    let authorization = sign::authorization(params.access_key, &scope, &signed_headers, &signature);

    let mut url = format!(
        "{}://{}{}",
        params.scheme, params.endpoint, spec.canonical_uri
    );
    if !canonical_query.is_empty() {
        url.push('?');
        url.push_str(&canonical_query);
    }

    let mut builder = http
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
    builder.build().map_err(CoreError::from)
}

/// 生成 SigV4 query 方式的 GET 预签名 URL,`expires_in` 秒后失效。纯本地签名。
pub fn presigned_get_url(
    params: &SigningParams<'_>,
    canonical_uri: &str,
    expires_in: u64,
    now: SystemTime,
) -> String {
    presigned_url(params, "GET", canonical_uri, expires_in, now)
}

/// 生成 SigV4 query 方式、指定 HTTP 方法的预签名 URL(GET 下载 / PUT 上传等)。纯本地签名。
pub fn presigned_url(
    params: &SigningParams<'_>,
    method: &str,
    canonical_uri: &str,
    expires_in: u64,
    now: SystemTime,
) -> String {
    let (amz_date, date) = amz_datetime(now);
    let scope = sign::credential_scope(&date, params.region, params.service);
    let credential = format!("{}/{scope}", params.access_key);

    let query = vec![
        ("X-Amz-Algorithm".to_string(), sign::ALGORITHM.to_string()),
        ("X-Amz-Credential".to_string(), credential),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_in.to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    let canonical_query = canonical_query_string(&query);

    let canonical_headers = format!("host:{}\n", params.endpoint);
    let cr = sign::canonical_request(
        method,
        canonical_uri,
        &canonical_query,
        &canonical_headers,
        "host",
        "UNSIGNED-PAYLOAD",
    );
    let sts = sign::string_to_sign(&amz_date, &scope, &cr);
    let key = sign::signing_key(params.secret_key, &date, params.region, params.service);
    let signature = sign::signature(&key, &sts);

    format!(
        "{}://{}{canonical_uri}?{canonical_query}&X-Amz-Signature={}",
        params.scheme,
        params.endpoint,
        uri_encode(&signature),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params<'a>() -> SigningParams<'a> {
        SigningParams {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            scheme: "https",
            endpoint: "s3.cn-east-1.qiniucs.com",
            region: "cn-east-1",
            service: sign::S3,
        }
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn amz_datetime_known_values() {
        assert_eq!(
            amz_datetime(UNIX_EPOCH),
            ("19700101T000000Z".into(), "19700101".into())
        );
        assert_eq!(
            amz_datetime(at(1_000_000_000)),
            ("20010909T014640Z".into(), "20010909".into())
        );
    }

    #[test]
    fn encode_path_keeps_slash() {
        assert_eq!(encode_path("dir/a.txt"), "dir/a.txt");
        assert_eq!(encode_path("dir/ b.txt"), "dir/%20b.txt");
        assert_eq!(encode_path("图片.png"), "%E5%9B%BE%E7%89%87.png");
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
    fn build_signed_request_is_path_style_and_matches_recompute() {
        let http = HttpClient::new();
        let p = params();
        let t = at(1_440_938_160);
        let req = build_signed_request(
            &http,
            &p,
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

        assert_eq!(
            req.url().as_str(),
            "https://s3.cn-east-1.qiniucs.com/mybucket/hello.txt"
        );

        // 独立复算 Authorization,验证签名整链一致。
        let (amz_date, date) = amz_datetime(t);
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
        let key = sign::signing_key(p.secret_key, &date, "cn-east-1", "s3");
        let expected = sign::authorization(p.access_key, &scope, sh, &sign::signature(&key, &sts));
        assert_eq!(
            req.headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            expected
        );
    }

    #[test]
    fn presigned_url_has_sigv4_query_params() {
        let p = params();
        let url = presigned_get_url(&p, "/mybucket/hello.txt", 3600, at(1_440_938_160));
        assert!(url.starts_with("https://s3.cn-east-1.qiniucs.com/mybucket/hello.txt?"));
        let parsed = reqwest::Url::parse(&url).unwrap();
        let m: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(m.get("X-Amz-Algorithm").unwrap(), "AWS4-HMAC-SHA256");
        assert_eq!(m.get("X-Amz-Expires").unwrap(), "3600");
        assert!(m.contains_key("X-Amz-Signature"));
    }
}
