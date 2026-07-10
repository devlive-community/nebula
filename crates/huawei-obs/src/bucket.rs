//! 桶级操作:列举对象(GET Bucket)、列举 / 创建 / 删除 bucket。
//!
//! 列举对象的 `prefix` / `marker` / `max-keys` / `delimiter` 都是普通查询参数,不参与
//! 签名(CanonicalizedResource 只是 `/{bucket}/`)。翻页借助 [`cloud_core::paginate`]:
//! 游标即 `marker`,截断时以 `NextMarker` 或本页最后一个 Key 作为下一页游标。
//!
//! OBS 与 OSS 的两点差异:列举 bucket(GET Service)一次性返回全部、**不分页**;建桶在
//! 非默认区域需在请求体带 `<CreateBucketConfiguration><Location>{region}</Location>`。

use cloud_core::{paginate, CoreError, Page};
use futures::Stream;
use reqwest::header::{AUTHORIZATION, DATE};
use reqwest::{Method, Request, Url};
use serde::Deserialize;

use crate::client::ObsClient;
use crate::error::{ObsError, Result};
use crate::object::{check_status, now_gmt};
use crate::sign;

/// 单页返回的每个对象条目。
#[derive(Debug, Clone)]
pub struct ObjectSummary {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
    pub storage_class: String,
}

/// 账号下的一个 bucket。
#[derive(Debug, Clone)]
pub struct BucketSummary {
    pub name: String,
    pub location: String,
    pub creation_date: String,
}

/// GET Service(列举 bucket)的 XML 响应体。OBS 不分页,一次返回全部。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListAllMyBucketsResult {
    #[serde(default)]
    buckets: BucketsXml,
}

#[derive(Debug, Default, Deserialize)]
struct BucketsXml {
    #[serde(default, rename = "Bucket")]
    bucket: Vec<BucketXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BucketXml {
    name: String,
    #[serde(default)]
    creation_date: String,
    #[serde(default)]
    location: String,
}

/// GET Bucket 的 XML 响应体(仅取需要的字段)。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListBucketResult {
    #[serde(default)]
    is_truncated: bool,
    #[serde(default)]
    next_marker: Option<String>,
    #[serde(default, rename = "Contents")]
    contents: Vec<ContentsXml>,
    /// 使用 delimiter 时,被折叠的"子目录"公共前缀。
    #[serde(default, rename = "CommonPrefixes")]
    common_prefixes: Vec<CommonPrefixXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContentsXml {
    key: String,
    last_modified: String,
    e_tag: String,
    size: u64,
    #[serde(default)]
    storage_class: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CommonPrefixXml {
    prefix: String,
}

impl From<ContentsXml> for ObjectSummary {
    fn from(c: ContentsXml) -> Self {
        ObjectSummary {
            key: c.key,
            size: c.size,
            etag: c.e_tag,
            last_modified: c.last_modified,
            storage_class: c.storage_class,
        }
    }
}

/// 列举一层目录时的条目:对象文件,或被 delimiter 折叠出的子目录前缀。
#[derive(Debug, Clone)]
pub enum ListEntry {
    Object(ObjectSummary),
    Prefix(String),
}

/// 每页最多返回的对象数(OBS 上限 1000)。
const MAX_KEYS: &str = "1000";

impl ObsClient {
    /// 列举某个 bucket 下的对象(可选前缀过滤),自动翻页为一条对象流。
    pub fn list_objects<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
    ) -> impl Stream<Item = Result<ObjectSummary>> + 'a {
        paginate(String::new(), move |marker: String| {
            self.list_objects_page(bucket, prefix, marker)
        })
    }

    /// 拉取一页对象,并算出下一页游标。
    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: String,
    ) -> Result<Page<ObjectSummary, String>> {
        let date = now_gmt();
        let request = self.build_list_request(bucket, prefix, &marker, None, &date)?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
        // 响应带 encoding-type=url,key / marker 均为 URL 编码,需解码回原文。
        let next = next_marker(&parsed).map(|m| url_decode(&m));
        let items = parsed
            .contents
            .into_iter()
            .map(|c| {
                let mut o = ObjectSummary::from(c);
                o.key = url_decode(&o.key);
                o
            })
            .collect();
        Ok(Page { items, next })
    }

    /// 列举**一层目录**:用 `delimiter = "/"` 把子前缀折叠成目录,返回文件与子目录混合流。
    pub fn list_dir<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
    ) -> impl Stream<Item = Result<ListEntry>> + 'a {
        paginate(String::new(), move |marker: String| {
            self.list_dir_page(bucket, prefix, marker)
        })
    }

    /// 拉取一层目录的一页(子目录前缀在前,文件在后)。
    pub async fn list_dir_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: String,
    ) -> Result<Page<ListEntry, String>> {
        let date = now_gmt();
        let request = self.build_list_request(bucket, prefix, &marker, Some("/"), &date)?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
        let next = next_marker(&parsed).map(|m| url_decode(&m));

        let mut items: Vec<ListEntry> = parsed
            .common_prefixes
            .into_iter()
            .map(|p| ListEntry::Prefix(url_decode(&p.prefix)))
            .collect();
        items.extend(parsed.contents.into_iter().map(|c| {
            let mut o = ObjectSummary::from(c);
            o.key = url_decode(&o.key);
            ListEntry::Object(o)
        }));
        Ok(Page { items, next })
    }

    /// 组装并签名一次 GET Bucket 请求。抽出 `date` 便于确定性测试。
    fn build_list_request(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: &str,
        delimiter: Option<&str>,
        date: &str,
    ) -> Result<Request> {
        // 普通查询参数不参与签名,CanonicalizedResource 仅为 /{bucket}/。
        let sts = sign::string_to_sign("GET", "", "", date, "", &format!("/{bucket}/"));
        let authorization = sign::authorization(self.access_key(), self.secret_key(), &sts);

        let mut url = Url::parse(&format!("{}/", self.bucket_base_url(bucket)))
            .map_err(|e| ObsError::Core(CoreError::InvalidRequest(e.to_string())))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("max-keys", MAX_KEYS);
            if let Some(p) = prefix.filter(|p| !p.is_empty()) {
                qp.append_pair("prefix", p);
            }
            if !marker.is_empty() {
                qp.append_pair("marker", marker);
            }
            if let Some(d) = delimiter {
                qp.append_pair("delimiter", d);
            }
            // 让返回的 Key / Prefix / Marker 以 URL 编码给出,避免特殊字符在 XML 中丢失。
            qp.append_pair("encoding-type", "url");
        }

        self.http()
            .inner()
            .request(Method::GET, url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(ObsError::from)
    }
}

impl ObsClient {
    /// 列举当前账号下的所有 bucket。OBS 不分页,单次返回;为与适配层接口一致仍包成流。
    pub fn list_buckets(&self) -> impl Stream<Item = Result<BucketSummary>> + '_ {
        // 初始游标 = false(未取过);取过一次后 next = None,流即终止。
        paginate(false, move |fetched: bool| async move {
            if fetched {
                return Ok(Page {
                    items: Vec::new(),
                    next: None,
                });
            }
            let items = self.list_buckets_once().await?;
            Ok(Page {
                items,
                next: Some(true),
            })
        })
    }

    /// 创建一个 bucket(默认私有读写)。非默认区域会在请求体带 Location。
    pub async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_create_bucket_request(bucket, &date)?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 删除一个 bucket(必须为空)。
    pub async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_bucket_root_request(Method::DELETE, bucket, &date)?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 拉取全部 bucket(单次请求)。
    async fn list_buckets_once(&self) -> Result<Vec<BucketSummary>> {
        let date = now_gmt();
        let request = self.build_list_buckets_request(&date)?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(&body)
            .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed
            .buckets
            .bucket
            .into_iter()
            .map(|b| BucketSummary {
                name: b.name,
                location: b.location,
                creation_date: b.creation_date,
            })
            .collect())
    }

    /// 组装并签名一次针对 bucket 根(`/{bucket}/`)的请求,用于删桶等。
    fn build_bucket_root_request(
        &self,
        method: Method,
        bucket: &str,
        date: &str,
    ) -> Result<Request> {
        let sts = sign::string_to_sign(method.as_str(), "", "", date, "", &format!("/{bucket}/"));
        let authorization = sign::authorization(self.access_key(), self.secret_key(), &sts);
        let url = format!("{}/", self.bucket_base_url(bucket));
        self.http()
            .inner()
            .request(method, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(ObsError::from)
    }

    /// 组装并签名建桶请求。非默认区域带 CreateBucketConfiguration body。
    fn build_create_bucket_request(&self, bucket: &str, date: &str) -> Result<Request> {
        let sts = sign::string_to_sign("PUT", "", "", date, "", &format!("/{bucket}/"));
        let authorization = sign::authorization(self.access_key(), self.secret_key(), &sts);
        let url = format!("{}/", self.bucket_base_url(bucket));

        let mut builder = self
            .http()
            .inner()
            .request(Method::PUT, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization);
        // V2 签名不对 body 取哈希,故带 Location body 不影响签名。
        if let Some(region) = self.region() {
            let body = format!(
                "<CreateBucketConfiguration><Location>{region}</Location></CreateBucketConfiguration>"
            );
            builder = builder.body(body);
        }
        builder
            .build()
            .map_err(CoreError::from)
            .map_err(ObsError::from)
    }

    /// 组装并签名一次 GET Service(列举 bucket)请求,CanonicalizedResource 为 `/`。
    fn build_list_buckets_request(&self, date: &str) -> Result<Request> {
        let sts = sign::string_to_sign("GET", "", "", date, "", "/");
        let authorization = sign::authorization(self.access_key(), self.secret_key(), &sts);

        let url = format!("https://{}/", self.endpoint());
        self.http()
            .inner()
            .request(Method::GET, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(ObsError::from)
    }
}

/// URL 解码(配合 `encoding-type=url`),非法字节按 lossy 处理。
fn url_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// 下一页游标:未截断则无;截断时优先用 `NextMarker`,否则退回本页最后一个 Key。
fn next_marker(result: &ListBucketResult) -> Option<String> {
    if !result.is_truncated {
        return None;
    }
    result
        .next_marker
        .clone()
        .filter(|m| !m.is_empty())
        .or_else(|| result.contents.last().map(|c| c.key.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> ObsClient {
        ObsClient::new(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "obs.cn-north-4.myhuaweicloud.com",
        )
    }

    #[test]
    fn list_request_signs_bucket_resource_and_sets_query() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_list_request("examplebucket", Some("photos/"), "cat.jpg", None, date)
            .unwrap();

        // CanonicalizedResource 是 /{bucket}/,查询参数不参与签名。
        let sts = sign::string_to_sign("GET", "", "", date, "", "/examplebucket/");
        let expected = sign::authorization(client.access_key(), client.secret_key(), &sts);
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );

        let query = req.url().query().unwrap();
        assert!(query.contains("max-keys=1000"));
        assert!(query.contains("prefix=photos%2F"));
        assert!(query.contains("marker=cat.jpg"));
        assert_eq!(
            req.url().host_str(),
            Some("examplebucket.obs.cn-north-4.myhuaweicloud.com")
        );
    }

    #[test]
    fn empty_prefix_and_marker_are_omitted() {
        let client = test_client();
        let req = client
            .build_list_request("b", Some(""), "", None, "date")
            .unwrap();
        let query = req.url().query().unwrap();
        assert!(query.contains("max-keys=1000"));
        assert!(!query.contains("prefix="));
        assert!(!query.contains("marker="));
        assert!(!query.contains("delimiter="));
    }

    #[test]
    fn list_request_sets_encoding_type_and_delimiter() {
        let client = test_client();
        let req = client
            .build_list_request("b", Some("photos/"), "", Some("/"), "date")
            .unwrap();
        let query = req.url().query().unwrap();
        assert!(query.contains("encoding-type=url"));
        assert!(query.contains("delimiter=%2F"));
        assert!(query.contains("prefix=photos%2F"));
    }

    #[test]
    fn url_decode_restores_specials() {
        assert_eq!(url_decode("dir/%20a.mp4"), "dir/ a.mp4");
        assert_eq!(url_decode("%E5%9B%BE%E7%89%87.png"), "图片.png");
        assert_eq!(url_decode("plain.txt"), "plain.txt");
    }

    #[test]
    fn parses_common_prefixes_as_dirs() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>b</Name>
  <Delimiter>/</Delimiter>
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>photos/cover.jpg</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>100</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
  <CommonPrefixes><Prefix>photos/2023/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>photos/2024/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let parsed: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.common_prefixes.len(), 2);
        assert_eq!(parsed.common_prefixes[0].prefix, "photos/2023/");
        assert_eq!(parsed.contents.len(), 1);
    }

    #[test]
    fn parses_list_result_and_maps_contents() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>examplebucket</Name>
  <Prefix></Prefix>
  <Marker></Marker>
  <MaxKeys>1000</MaxKeys>
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>a.txt</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>10</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
  <Contents>
    <Key>b.txt</Key>
    <LastModified>2024-01-02T00:00:00.000Z</LastModified>
    <ETag>"E2"</ETag>
    <Size>20</Size>
    <StorageClass>WARM</StorageClass>
  </Contents>
</ListBucketResult>"#;
        let parsed: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.contents.len(), 2);
        assert_eq!(parsed.contents[0].key, "a.txt");
        assert_eq!(parsed.contents[1].size, 20);
        assert_eq!(parsed.contents[1].storage_class, "WARM");
        assert!(next_marker(&parsed).is_none()); // 未截断
    }

    #[test]
    fn list_buckets_request_signs_root_resource() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client.build_list_buckets_request(date).unwrap();

        // GET Service 的 CanonicalizedResource 是 "/"。
        let sts = sign::string_to_sign("GET", "", "", date, "", "/");
        let expected = sign::authorization(client.access_key(), client.secret_key(), &sts);
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );
        // 走 service endpoint,host 不含 bucket 前缀。
        assert_eq!(
            req.url().host_str(),
            Some("obs.cn-north-4.myhuaweicloud.com")
        );
    }

    #[test]
    fn create_bucket_request_includes_location_body() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_create_bucket_request("new-bucket", date)
            .unwrap();

        assert_eq!(req.method(), Method::PUT);
        assert_eq!(
            req.url().as_str(),
            "https://new-bucket.obs.cn-north-4.myhuaweicloud.com/"
        );
        let sts = sign::string_to_sign("PUT", "", "", date, "", "/new-bucket/");
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            sign::authorization(client.access_key(), client.secret_key(), &sts)
        );
        // 请求体应带解析出的区域。
        let body = req.body().unwrap().as_bytes().unwrap();
        assert_eq!(
            std::str::from_utf8(body).unwrap(),
            "<CreateBucketConfiguration><Location>cn-north-4</Location></CreateBucketConfiguration>"
        );
    }

    #[test]
    fn delete_bucket_request_targets_bucket_root() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let del = client
            .build_bucket_root_request(Method::DELETE, "old-bucket", date)
            .unwrap();
        assert_eq!(del.method(), Method::DELETE);
        assert_eq!(
            del.url().as_str(),
            "https://old-bucket.obs.cn-north-4.myhuaweicloud.com/"
        );
    }

    #[test]
    fn parses_list_all_my_buckets() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://obs.myhwclouds.com/doc/2015-06-30/">
  <Owner><ID>0000</ID></Owner>
  <Buckets>
    <Bucket>
      <Name>devlive-cdn</Name>
      <CreationDate>2020-01-01T00:00:00.000Z</CreationDate>
      <Location>cn-north-4</Location>
    </Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.buckets.bucket.len(), 1);
        assert_eq!(parsed.buckets.bucket[0].name, "devlive-cdn");
        assert_eq!(parsed.buckets.bucket[0].location, "cn-north-4");
    }

    #[test]
    fn next_marker_prefers_explicit_then_last_key() {
        let with_next = ListBucketResult {
            is_truncated: true,
            next_marker: Some("n.txt".into()),
            contents: vec![ContentsXml {
                key: "z.txt".into(),
                last_modified: String::new(),
                e_tag: String::new(),
                size: 0,
                storage_class: String::new(),
            }],
            common_prefixes: vec![],
        };
        assert_eq!(next_marker(&with_next).as_deref(), Some("n.txt"));

        let fallback = ListBucketResult {
            is_truncated: true,
            next_marker: None,
            contents: vec![ContentsXml {
                key: "z.txt".into(),
                last_modified: String::new(),
                e_tag: String::new(),
                size: 0,
                storage_class: String::new(),
            }],
            common_prefixes: vec![],
        };
        assert_eq!(next_marker(&fallback).as_deref(), Some("z.txt"));
    }
}
