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
use crate::object::{check_status, xml_escape};

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

    /// 读取一个 bucket 的生命周期规则:`GET /?lifecycle`。未配置生命周期时服务端返回
    /// `NoSuchLifecycleConfiguration`,视为空规则列表。
    pub async fn get_bucket_lifecycle(&self, bucket: &str) -> Result<Vec<LifecycleRule>> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: "/",
            query: &[("lifecycle", None)],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(CosError::Api { code, .. }) if code == "NoSuchLifecycleConfiguration" => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_lifecycle(&body)
    }

    /// 设置一个 bucket 的生命周期规则(整套替换):`PUT /?lifecycle`。空列表时改发
    /// `DELETE /?lifecycle`(COS 不接受没有任何 `Rule` 的配置)。
    pub async fn set_bucket_lifecycle(&self, bucket: &str, rules: &[LifecycleRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_lifecycle(bucket).await;
        }
        let host = self.bucket_host(bucket);
        let body = build_lifecycle_xml(rules);
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: "/",
            query: &[("lifecycle", None)],
            content_type: Some("application/xml"),
            content_md5: None,
            cos_headers: &[],
            body: Some(bytes::Bytes::from(body)),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<()> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method: Method::DELETE,
            host: &host,
            uri_path: "/",
            query: &[("lifecycle", None)],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 查询一个 bucket 是否已启用版本控制:`GET /?versioning`。从未配置过时响应是空的
    /// `<VersioningConfiguration/>`(没有 `Status` 子元素),视为未启用。
    pub async fn get_bucket_versioning(&self, bucket: &str) -> Result<bool> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: "/",
            query: &[("versioning", None)],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        let parsed: VersioningConfigurationXml = quick_xml::de::from_str(&body)
            .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed.status.as_deref() == Some("Enabled"))
    }

    /// 启用或暂停一个 bucket 的版本控制:`PUT /?versioning`。
    pub async fn set_bucket_versioning(&self, bucket: &str, enabled: bool) -> Result<()> {
        let host = self.bucket_host(bucket);
        let status = if enabled { "Enabled" } else { "Suspended" };
        let body =
            format!("<VersioningConfiguration><Status>{status}</Status></VersioningConfiguration>");
        let request = self.build_signed(SignSpec {
            method: Method::PUT,
            host: &host,
            uri_path: "/",
            query: &[("versioning", None)],
            content_type: Some("application/xml"),
            content_md5: None,
            cos_headers: &[],
            body: Some(bytes::Bytes::from(body)),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 列出一个对象的全部历史版本(含删除标记):`GET /?versions&prefix={key}`。只取
    /// 第一页,按 `key` 精确匹配过滤,按修改时间从新到旧排序。
    pub async fn list_object_versions(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Vec<ObjectVersion>> {
        let host = self.bucket_host(bucket);
        let request = self.build_signed(SignSpec {
            method: Method::GET,
            host: &host,
            uri_path: "/",
            query: &[
                ("versions", None),
                ("prefix", Some(key)),
                ("max-keys", Some(MAX_KEYS)),
            ],
            content_type: None,
            content_md5: None,
            cos_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_object_versions(&body, key)
    }
}

/// 一条 bucket 生命周期规则(本 crate 的本地表示;适配层负责映射到上层统一模型)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleRule {
    pub id: String,
    pub prefix: String,
    pub enabled: bool,
    pub expiration_days: Option<u32>,
    /// `(天数, 目标存储类型字符串)` 有序对。
    pub transitions: Vec<(u32, String)>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LifecycleConfigurationXml {
    #[serde(default, rename = "Rule")]
    rule: Vec<RuleXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RuleXml {
    #[serde(rename = "ID", default)]
    id: String,
    #[serde(default)]
    filter: Option<FilterXml>,
    #[serde(default)]
    prefix: Option<String>,
    status: String,
    #[serde(default, rename = "Transition")]
    transition: Vec<TransitionXml>,
    #[serde(default)]
    expiration: Option<ExpirationXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FilterXml {
    #[serde(default)]
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TransitionXml {
    days: u32,
    storage_class: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExpirationXml {
    #[serde(default)]
    days: Option<u32>,
}

/// 解析 `GetBucketLifecycleConfiguration` 的 XML 响应。
fn parse_lifecycle(xml: &str) -> Result<Vec<LifecycleRule>> {
    let doc: LifecycleConfigurationXml = quick_xml::de::from_str(xml)
        .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))?;
    Ok(doc
        .rule
        .into_iter()
        .map(|r| LifecycleRule {
            id: r.id,
            prefix: r.filter.map(|f| f.prefix).or(r.prefix).unwrap_or_default(),
            enabled: r.status == "Enabled",
            expiration_days: r.expiration.and_then(|e| e.days),
            transitions: r
                .transition
                .into_iter()
                .map(|t| (t.days, t.storage_class))
                .collect(),
        })
        .collect())
}

/// 生成 `PutBucketLifecycleConfiguration` 的请求体 XML。
fn build_lifecycle_xml(rules: &[LifecycleRule]) -> String {
    let mut body = String::from("<LifecycleConfiguration>");
    for r in rules {
        body.push_str("<Rule><ID>");
        body.push_str(&xml_escape(&r.id));
        body.push_str("</ID><Filter><Prefix>");
        body.push_str(&xml_escape(&r.prefix));
        body.push_str("</Prefix></Filter><Status>");
        body.push_str(if r.enabled { "Enabled" } else { "Disabled" });
        body.push_str("</Status>");
        for (days, class) in &r.transitions {
            body.push_str(&format!(
                "<Transition><Days>{days}</Days><StorageClass>{}</StorageClass></Transition>",
                xml_escape(class)
            ));
        }
        if let Some(days) = r.expiration_days {
            body.push_str(&format!("<Expiration><Days>{days}</Days></Expiration>"));
        }
        body.push_str("</Rule>");
    }
    body.push_str("</LifecycleConfiguration>");
    body
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

/// 一条历史版本(本 crate 的本地表示;适配层负责映射到上层统一模型)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    pub version_id: String,
    pub is_latest: bool,
    pub is_delete_marker: bool,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersioningConfigurationXml {
    #[serde(rename = "Status", default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionXml {
    key: String,
    version_id: String,
    is_latest: bool,
    last_modified: String,
    #[serde(default)]
    e_tag: Option<String>,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMarkerXml {
    key: String,
    version_id: String,
    is_latest: bool,
    last_modified: String,
}

/// 解析 `ListObjectVersions` 的 XML 响应:按 `key` 精确过滤,按修改时间从新到旧排序。
/// `Version` 与 `DeleteMarker` 交错出现,quick_xml 的 struct+Vec 反序列化处理不了这种
/// 交错,改用事件流手动扫描顶层子元素、逐个片段反序列化(见 s3-core 的同名函数注释)。
fn parse_object_versions(xml: &str, key: &str) -> Result<Vec<ObjectVersion>> {
    use quick_xml::events::Event;
    use quick_xml::name::QName;
    use quick_xml::Reader;

    let to_err = |e: quick_xml::Error| CosError::Core(CoreError::InvalidResponse(e.to_string()));
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut versions = Vec::new();

    loop {
        match reader.read_event_into(&mut buf).map_err(to_err)? {
            Event::Start(e) if e.name() == QName(b"Version") => {
                let span = reader
                    .read_to_end_into(QName(b"Version"), &mut Vec::new())
                    .map_err(to_err)?;
                let inner = &xml[span.start as usize..span.end as usize];
                let frag = format!("<Version>{inner}</Version>");
                let v: VersionXml = quick_xml::de::from_str(&frag)
                    .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))?;
                if v.key == key {
                    versions.push(ObjectVersion {
                        version_id: v.version_id,
                        is_latest: v.is_latest,
                        is_delete_marker: false,
                        size: v.size,
                        etag: v.e_tag,
                        last_modified: v.last_modified,
                    });
                }
            }
            Event::Start(e) if e.name() == QName(b"DeleteMarker") => {
                let span = reader
                    .read_to_end_into(QName(b"DeleteMarker"), &mut Vec::new())
                    .map_err(to_err)?;
                let inner = &xml[span.start as usize..span.end as usize];
                let frag = format!("<DeleteMarker>{inner}</DeleteMarker>");
                let d: DeleteMarkerXml = quick_xml::de::from_str(&frag)
                    .map_err(|e| CosError::Core(CoreError::InvalidResponse(e.to_string())))?;
                if d.key == key {
                    versions.push(ObjectVersion {
                        version_id: d.version_id,
                        is_latest: d.is_latest,
                        is_delete_marker: true,
                        size: 0,
                        etag: None,
                        last_modified: d.last_modified,
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(versions)
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

    #[test]
    fn lifecycle_xml_round_trips() {
        let rules = vec![
            LifecycleRule {
                id: "archive-logs".into(),
                prefix: "logs/".into(),
                enabled: true,
                expiration_days: Some(365),
                transitions: vec![(30, "ARCHIVE".into()), (90, "DEEP_ARCHIVE".into())],
            },
            LifecycleRule {
                id: "disabled-rule".into(),
                prefix: String::new(),
                enabled: false,
                expiration_days: None,
                transitions: vec![],
            },
        ];
        let xml = build_lifecycle_xml(&rules);
        let parsed = parse_lifecycle(&xml).unwrap();
        assert_eq!(parsed, rules);
    }

    #[test]
    fn versioning_configuration_parses_status() {
        let enabled: VersioningConfigurationXml = quick_xml::de::from_str(
            "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
        )
        .unwrap();
        assert_eq!(enabled.status.as_deref(), Some("Enabled"));

        let never: VersioningConfigurationXml =
            quick_xml::de::from_str("<VersioningConfiguration/>").unwrap();
        assert_eq!(never.status, None);
    }

    #[test]
    fn parses_and_merges_versions_and_delete_markers_by_key_sorted_newest_first() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListVersionsResult>
  <Name>b</Name>
  <Prefix>photo.jpg</Prefix>
  <IsTruncated>false</IsTruncated>
  <Version>
    <Key>photo.jpg</Key>
    <VersionId>v-old</VersionId>
    <IsLatest>false</IsLatest>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>100</Size>
  </Version>
  <DeleteMarker>
    <Key>photo.jpg</Key>
    <VersionId>v-deleted</VersionId>
    <IsLatest>true</IsLatest>
    <LastModified>2024-03-01T00:00:00.000Z</LastModified>
  </DeleteMarker>
  <Version>
    <Key>photo.jpg.bak</Key>
    <VersionId>v-other-key</VersionId>
    <IsLatest>true</IsLatest>
    <LastModified>2024-02-01T00:00:00.000Z</LastModified>
    <ETag>"E2"</ETag>
    <Size>50</Size>
  </Version>
</ListVersionsResult>"#;
        let versions = parse_object_versions(xml, "photo.jpg").unwrap();
        assert_eq!(versions.len(), 2);
        assert!(versions[0].is_delete_marker);
        assert_eq!(versions[0].version_id, "v-deleted");
        assert!(!versions[1].is_delete_marker);
        assert_eq!(versions[1].version_id, "v-old");
        assert_eq!(versions[1].size, 100);
    }
}
