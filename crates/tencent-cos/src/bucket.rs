//! 桶级操作:列举对象(GET Bucket)、列举 / 创建 / 删除 bucket。
//!
//! COS 列举用 V1 `marker` 翻页;list 的查询参数(prefix/marker/delimiter/max-keys)**参与
//! COS 签名**(`build_signed` 已把参数计入 canonical)。列桶走 `service.cos.myqcloud.com`。

use cloud_core::{paginate, CoreError, Page};
use futures::Stream;
use reqwest::{Method, Response};
use serde::Deserialize;

use crate::client::{CosClient, SignSpec};
use crate::error::{CosError, Result};
use crate::object::check_status;

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

/// 列举一层目录时的条目:对象文件,或被 delimiter 折叠出的子目录前缀。
#[derive(Debug, Clone)]
pub enum ListEntry {
    Object(ObjectSummary),
    Prefix(String),
}

/// GET Bucket 的 XML 响应体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListBucketResult {
    #[serde(default)]
    is_truncated: bool,
    #[serde(default)]
    next_marker: Option<String>,
    #[serde(default, rename = "Contents")]
    contents: Vec<ContentsXml>,
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

/// GET Service(列举 bucket)的 XML 响应体。
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
    location: String,
    #[serde(default)]
    creation_date: String,
}

const MAX_KEYS: &str = "1000";

impl CosClient {
    /// 列举某个 bucket 下的对象(可选前缀),自动翻页为一条对象流。
    pub fn list_objects<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
    ) -> impl Stream<Item = Result<ObjectSummary>> + 'a {
        paginate(String::new(), move |marker: String| {
            self.list_objects_page(bucket, prefix, marker)
        })
    }

    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: String,
    ) -> Result<Page<ObjectSummary, String>> {
        let parsed = self.list_page(bucket, prefix, None, &marker).await?;
        let next = next_marker(&parsed);
        let items = parsed
            .contents
            .into_iter()
            .map(ObjectSummary::from)
            .collect();
        Ok(Page { items, next })
    }

    /// 列举**一层目录**:用 `delimiter = "/"` 折叠子前缀,返回文件与子目录混合流。
    pub fn list_dir<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
    ) -> impl Stream<Item = Result<ListEntry>> + 'a {
        paginate(String::new(), move |marker: String| {
            self.list_dir_page(bucket, prefix, marker)
        })
    }

    pub async fn list_dir_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: String,
    ) -> Result<Page<ListEntry, String>> {
        let parsed = self.list_page(bucket, prefix, Some("/"), &marker).await?;
        let next = next_marker(&parsed);
        let mut items: Vec<ListEntry> = parsed
            .common_prefixes
            .into_iter()
            .map(|p| ListEntry::Prefix(p.prefix))
            .collect();
        items.extend(
            parsed
                .contents
                .into_iter()
                .map(|c| ListEntry::Object(ObjectSummary::from(c))),
        );
        Ok(Page { items, next })
    }

    /// 拉取一页 GET Bucket 并解析。
    async fn list_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        delimiter: Option<&str>,
        marker: &str,
    ) -> Result<ListBucketResult> {
        let mut query: Vec<(&str, Option<&str>)> = vec![("max-keys", Some(MAX_KEYS))];
        if let Some(p) = prefix.filter(|p| !p.is_empty()) {
            query.push(("prefix", Some(p)));
        }
        if !marker.is_empty() {
            query.push(("marker", Some(marker)));
        }
        if let Some(d) = delimiter {
            query.push(("delimiter", Some(d)));
        }
        let resp = self.send_bucket_get(bucket, &query).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        quick_xml::de::from_str(&body)
            .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))
    }

    /// 对某桶发一次 `GET /`(列举);若跨区域被拒且服务端给了正确 endpoint,缓存后重试一次。
    /// 自举区域缓存(即便本会话没先列过桶列表)。
    async fn send_bucket_get(
        &self,
        bucket: &str,
        query: &[(&str, Option<&str>)],
    ) -> Result<Response> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: "/",
            query,
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        match check_status(self.http().execute(request).await?).await {
            Ok(resp) => Ok(resp),
            Err(e) => match redirect_endpoint(&e, bucket) {
                Some(endpoint) => {
                    self.cache_bucket_endpoint(bucket, &endpoint);
                    let host = self.bucket_host(bucket);
                    let retry = self.build_signed(SignSpec {
                        method: Method::GET,
                        host: &host,
                        uri_path: "/",
                        query,
                        content_type: None,
                        content_md5: None,
                        cos_headers: &[],
                        body: None,
                    })?;
                    check_status(self.http().execute(retry).await?).await
                }
                None => Err(e),
            },
        }
    }

    /// 列举当前账号下的所有 bucket(GET Service,单次返回)。
    pub fn list_buckets(&self) -> impl Stream<Item = Result<BucketSummary>> + '_ {
        paginate(false, move |fetched: bool| async move {
            if fetched {
                return Ok(Page {
                    items: Vec::new(),
                    next: None,
                });
            }
            Ok(Page {
                items: self.list_buckets_once().await?,
                next: Some(true),
            })
        })
    }

    async fn list_buckets_once(&self) -> Result<Vec<BucketSummary>> {
        let host = self.service_host().to_string();
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: "/",
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(&body)
            .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed
            .buckets
            .bucket
            .into_iter()
            .map(|b| {
                // 缓存桶 → 区域 endpoint(据 Location 推导),之后访问该桶自动路由到正确区域。
                if !b.location.is_empty() {
                    self.cache_bucket_endpoint(
                        &b.name,
                        &format!("cos.{}.myqcloud.com", b.location),
                    );
                }
                BucketSummary {
                    name: b.name,
                    location: b.location,
                    creation_date: b.creation_date,
                }
            })
            .collect())
    }

    /// 创建一个 bucket(私有读写)。
    pub async fn create_bucket(&self, bucket: &str) -> Result<()> {
        self.bucket_root(Method::PUT, bucket).await
    }

    /// 删除一个 bucket(必须为空)。
    pub async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        self.bucket_root(Method::DELETE, bucket).await
    }

    async fn bucket_root(&self, method: Method, bucket: &str) -> Result<()> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method,
            host: &host,
            uri_path: "/",
            query: &[],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }
}

/// 下一页游标:未截断则无;截断时优先 NextMarker,否则退回本页最后一个 Key。
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

/// 从跨区域错误里取出正确的区域 endpoint(去掉可能的 `{bucket}.` 前缀)。非该类错误返回 None。
fn redirect_endpoint(err: &CosError, bucket: &str) -> Option<String> {
    match err {
        CosError::Api {
            endpoint: Some(ep), ..
        } if !ep.is_empty() => {
            let prefix = format!("{bucket}.");
            Some(ep.strip_prefix(&prefix).unwrap_or(ep).to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_bucket_with_common_prefixes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>bkt-123</Name>
  <IsTruncated>true</IsTruncated>
  <NextMarker>next.txt</NextMarker>
  <Contents>
    <Key>photos/a.jpg</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>100</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
  <CommonPrefixes><Prefix>photos/2024/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let parsed: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.contents[0].size, 100);
        assert_eq!(parsed.common_prefixes[0].prefix, "photos/2024/");
        assert_eq!(next_marker(&parsed).as_deref(), Some("next.txt"));
    }

    #[test]
    fn next_marker_none_when_not_truncated() {
        let parsed = ListBucketResult {
            is_truncated: false,
            next_marker: Some("x".into()),
            contents: vec![],
            common_prefixes: vec![],
        };
        assert!(next_marker(&parsed).is_none());
    }

    #[test]
    fn parses_list_all_buckets() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult>
  <Buckets>
    <Bucket><Name>bkt-123</Name><Location>ap-beijing</Location><CreationDate>2020-01-01T00:00:00Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.buckets.bucket[0].name, "bkt-123");
        assert_eq!(parsed.buckets.bucket[0].location, "ap-beijing");
    }
}
