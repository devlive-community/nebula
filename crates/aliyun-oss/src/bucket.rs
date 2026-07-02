//! 桶级操作。当前提供列举对象(GET Bucket / List Objects,V1)。
//!
//! V1 的 `prefix` / `marker` / `max-keys` / `delimiter` 都是普通查询参数,不参与
//! 签名(CanonicalizedResource 只是 `/{bucket}/`),因此签名逻辑很简单。翻页借助
//! [`cloud_core::paginate`]:游标即 `marker`,截断时以 `NextMarker` 或本页最后一个
//! Key 作为下一页游标。

use cloud_core::{paginate, CoreError, Page};
use futures::Stream;
use reqwest::header::{AUTHORIZATION, DATE};
use reqwest::{Method, Request, Url};
use serde::Deserialize;

use crate::client::OssClient;
use crate::error::{OssError, Result};
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
    pub storage_class: String,
    /// 外网访问 endpoint。
    pub endpoint: String,
}

/// GET Service(列举 bucket)的 XML 响应体。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListAllMyBucketsResult {
    #[serde(default)]
    is_truncated: bool,
    #[serde(default)]
    next_marker: Option<String>,
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
    #[serde(default)]
    storage_class: String,
    #[serde(default)]
    extranet_endpoint: String,
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContentsXml {
    key: String,
    last_modified: String,
    e_tag: String,
    size: u64,
    storage_class: String,
}

/// 每页最多返回的对象数(OSS 上限 1000)。
const MAX_KEYS: &str = "1000";

impl OssClient {
    /// 列举某个 bucket 下的对象(可选前缀过滤),自动翻页为一条对象流。
    ///
    /// 返回的流按 `Result<ObjectSummary>` 逐个产出;任一页请求失败时,错误作为流的
    /// 最后一项产出后终止。
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
        let request = self.build_list_request(bucket, prefix, &marker, &date)?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
        let next = next_marker(&parsed);
        let items = parsed
            .contents
            .into_iter()
            .map(|c| ObjectSummary {
                key: c.key,
                size: c.size,
                etag: c.e_tag,
                last_modified: c.last_modified,
                storage_class: c.storage_class,
            })
            .collect();
        Ok(Page { items, next })
    }

    /// 组装并签名一次 GET Bucket 请求。抽出 `date` 便于确定性测试。
    fn build_list_request(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: &str,
        date: &str,
    ) -> Result<Request> {
        // 普通查询参数不参与签名,CanonicalizedResource 仅为 /{bucket}/。
        let sts = sign::string_to_sign("GET", "", "", date, "", &format!("/{bucket}/"));
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);

        let mut url = Url::parse(&format!("{}/", self.bucket_base_url(bucket)))
            .map_err(|e| OssError::Core(CoreError::InvalidRequest(e.to_string())))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("max-keys", MAX_KEYS);
            if let Some(p) = prefix.filter(|p| !p.is_empty()) {
                qp.append_pair("prefix", p);
            }
            if !marker.is_empty() {
                qp.append_pair("marker", marker);
            }
        }

        self.http()
            .inner()
            .request(Method::GET, url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(OssError::from)
    }
}

impl OssClient {
    /// 列举当前账号下的所有 bucket,自动翻页为一条流。
    pub fn list_buckets(&self) -> impl Stream<Item = Result<BucketSummary>> + '_ {
        paginate(String::new(), move |marker: String| {
            self.list_buckets_page(marker)
        })
    }

    /// 创建一个 bucket(默认配置,私有读写)。
    pub async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_bucket_root_request(Method::PUT, bucket, &date)?;
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

    /// 拉取一页 bucket 列表,并算出下一页游标。
    async fn list_buckets_page(&self, marker: String) -> Result<Page<BucketSummary, String>> {
        let date = now_gmt();
        let request = self.build_list_buckets_request(&marker, &date)?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;

        let next = if parsed.is_truncated {
            parsed
                .next_marker
                .clone()
                .filter(|m| !m.is_empty())
                .or_else(|| parsed.buckets.bucket.last().map(|b| b.name.clone()))
        } else {
            None
        };
        let items = parsed
            .buckets
            .bucket
            .into_iter()
            .map(|b| BucketSummary {
                name: b.name,
                location: b.location,
                creation_date: b.creation_date,
                storage_class: b.storage_class,
                endpoint: b.extranet_endpoint,
            })
            .collect();
        Ok(Page { items, next })
    }

    /// 组装并签名一次针对 bucket 根(`/{bucket}/`)的请求,用于建桶 / 删桶。
    fn build_bucket_root_request(
        &self,
        method: Method,
        bucket: &str,
        date: &str,
    ) -> Result<Request> {
        let sts = sign::string_to_sign(method.as_str(), "", "", date, "", &format!("/{bucket}/"));
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);
        let url = format!("{}/", self.bucket_base_url(bucket));
        self.http()
            .inner()
            .request(method, &url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(OssError::from)
    }

    /// 组装并签名一次 GET Service(列举 bucket)请求,CanonicalizedResource 为 `/`。
    fn build_list_buckets_request(&self, marker: &str, date: &str) -> Result<Request> {
        let sts = sign::string_to_sign("GET", "", "", date, "", "/");
        let authorization =
            sign::authorization(self.access_key_id(), self.access_key_secret(), &sts);

        let mut url = Url::parse(&format!("https://{}/", self.endpoint()))
            .map_err(|e| OssError::Core(CoreError::InvalidRequest(e.to_string())))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("max-keys", MAX_KEYS);
            if !marker.is_empty() {
                qp.append_pair("marker", marker);
            }
        }
        self.http()
            .inner()
            .request(Method::GET, url)
            .header(DATE, date)
            .header(AUTHORIZATION, authorization)
            .build()
            .map_err(CoreError::from)
            .map_err(OssError::from)
    }
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

    fn test_client() -> OssClient {
        OssClient::new(
            "44CF9590006BF252F707",
            "OtxrzxIsfpFjA7SwPzILwy8Bw21TLhquhboDYROV",
            "oss-cn-hangzhou.aliyuncs.com",
        )
    }

    #[test]
    fn list_request_signs_bucket_resource_and_sets_query() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_list_request("oss-example", Some("photos/"), "cat.jpg", date)
            .unwrap();

        // CanonicalizedResource 是 /{bucket}/,查询参数不参与签名。
        let sts = sign::string_to_sign("GET", "", "", date, "", "/oss-example/");
        let expected =
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts);
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
            Some("oss-example.oss-cn-hangzhou.aliyuncs.com")
        );
    }

    #[test]
    fn empty_prefix_and_marker_are_omitted() {
        let client = test_client();
        let req = client
            .build_list_request("b", Some(""), "", "date")
            .unwrap();
        let query = req.url().query().unwrap();
        assert!(query.contains("max-keys=1000"));
        assert!(!query.contains("prefix="));
        assert!(!query.contains("marker="));
    }

    #[test]
    fn parses_list_result_and_maps_contents() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>oss-example</Name>
  <Prefix></Prefix>
  <Marker></Marker>
  <MaxKeys>1000</MaxKeys>
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>a.txt</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>10</Size>
    <StorageClass>Standard</StorageClass>
  </Contents>
  <Contents>
    <Key>b.txt</Key>
    <LastModified>2024-01-02T00:00:00.000Z</LastModified>
    <ETag>"E2"</ETag>
    <Size>20</Size>
    <StorageClass>IA</StorageClass>
  </Contents>
</ListBucketResult>"#;
        let parsed: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.contents.len(), 2);
        assert_eq!(parsed.contents[0].key, "a.txt");
        assert_eq!(parsed.contents[1].size, 20);
        assert_eq!(parsed.contents[1].storage_class, "IA");
        assert!(next_marker(&parsed).is_none()); // 未截断
    }

    #[test]
    fn list_buckets_request_signs_root_resource() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client.build_list_buckets_request("mybucket", date).unwrap();

        // GET Service 的 CanonicalizedResource 是 "/"。
        let sts = sign::string_to_sign("GET", "", "", date, "", "/");
        let expected =
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts);
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );
        // 走 service endpoint,host 不含 bucket 前缀。
        assert_eq!(req.url().host_str(), Some("oss-cn-hangzhou.aliyuncs.com"));
        assert!(req.url().query().unwrap().contains("marker=mybucket"));
    }

    #[test]
    fn create_and_delete_bucket_requests_target_bucket_root() {
        let client = test_client();
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";

        let put = client
            .build_bucket_root_request(Method::PUT, "new-bucket", date)
            .unwrap();
        assert_eq!(put.method(), Method::PUT);
        assert_eq!(
            put.url().as_str(),
            "https://new-bucket.oss-cn-hangzhou.aliyuncs.com/"
        );
        let sts = sign::string_to_sign("PUT", "", "", date, "", "/new-bucket/");
        assert_eq!(
            put.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts)
        );

        let del = client
            .build_bucket_root_request(Method::DELETE, "new-bucket", date)
            .unwrap();
        assert_eq!(del.method(), Method::DELETE);
    }

    #[test]
    fn parses_list_all_my_buckets() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult>
  <Owner><ID>1</ID><DisplayName>1</DisplayName></Owner>
  <Buckets>
    <Bucket>
      <Name>devlive-cdn</Name>
      <CreationDate>2020-01-01T00:00:00.000Z</CreationDate>
      <Location>oss-cn-hangzhou</Location>
      <StorageClass>Standard</StorageClass>
      <ExtranetEndpoint>oss-cn-hangzhou.aliyuncs.com</ExtranetEndpoint>
    </Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.buckets.bucket.len(), 1);
        assert_eq!(parsed.buckets.bucket[0].name, "devlive-cdn");
        assert_eq!(parsed.buckets.bucket[0].location, "oss-cn-hangzhou");
        assert!(!parsed.is_truncated);
    }

    #[test]
    fn next_marker_prefers_explicit_then_last_key() {
        // 截断 + 显式 NextMarker。
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
        };
        assert_eq!(next_marker(&with_next).as_deref(), Some("n.txt"));

        // 截断但无 NextMarker,退回最后一个 Key。
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
        };
        assert_eq!(next_marker(&fallback).as_deref(), Some("z.txt"));
    }
}
