//! 桶级操作:列举对象(ListObjectsV2)、列举 / 创建 / 删除 bucket。
//!
//! 与 OSS/OBS 不同:S3 的 list 查询参数(`list-type`/`prefix`/`delimiter`/…)**参与 SigV4 签名**
//! (`build_signed` 已把查询计入 canonical),翻页游标是 `continuation-token`。

use cloud_core::{paginate, CoreError, Page};
use futures::Stream;
use reqwest::Method;
use serde::Deserialize;

use s3_sigv4::RequestSpec;

use crate::client::S3Client;
use crate::error::{Result, S3Error};
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
    pub creation_date: String,
}

/// 列举一层目录时的条目:对象文件,或被 delimiter 折叠出的子目录前缀。
#[derive(Debug, Clone)]
pub enum ListEntry {
    Object(ObjectSummary),
    Prefix(String),
}

/// ListObjectsV2 的 XML 响应体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListBucketResult {
    #[serde(default)]
    is_truncated: bool,
    #[serde(default)]
    next_continuation_token: Option<String>,
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
    creation_date: String,
}

const MAX_KEYS: &str = "1000";

impl S3Client {
    /// 列举某个 bucket 下的对象(可选前缀),自动翻页为一条对象流。
    pub fn list_objects<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
    ) -> impl Stream<Item = Result<ObjectSummary>> + 'a {
        paginate(String::new(), move |token: String| {
            self.list_objects_page(bucket, prefix, token)
        })
    }

    async fn list_objects_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        token: String,
    ) -> Result<Page<ObjectSummary, String>> {
        let parsed = self.list_page(bucket, prefix, None, &token).await?;
        let next = next_token(&parsed);
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
        paginate(String::new(), move |token: String| {
            self.list_dir_page(bucket, prefix, token)
        })
    }

    pub async fn list_dir_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        token: String,
    ) -> Result<Page<ListEntry, String>> {
        let parsed = self.list_page(bucket, prefix, Some("/"), &token).await?;
        let next = next_token(&parsed);
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

    /// 拉取一页 ListObjectsV2 并解析。
    async fn list_page(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        delimiter: Option<&str>,
        token: &str,
    ) -> Result<ListBucketResult> {
        let query = list_query(prefix, delimiter, token);
        // 用 send:首次列举跨区域桶时会从 x-amz-bucket-region 学到正确区域并缓存,
        // 之后该桶的其它操作经 build_signed 自动路由。
        let resp = check_status(
            self.send(RequestSpec {
                method: Method::GET,
                canonical_uri: &format!("/{bucket}"),
                query: &query,
                content_type: None,
                amz_headers: &[],
                body: None,
            })
            .await?,
        )
        .await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        quick_xml::de::from_str(&body)
            .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))
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
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: "/",
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(&body)
            .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed
            .buckets
            .bucket
            .into_iter()
            .map(|b| BucketSummary {
                name: b.name,
                creation_date: b.creation_date,
            })
            .collect())
    }

    /// 创建一个 bucket。请求体带 LocationConstraint = region。
    pub async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let body = format!(
            "<CreateBucketConfiguration><LocationConstraint>{}</LocationConstraint></CreateBucketConfiguration>",
            self.region()
        );
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &format!("/{bucket}"),
            query: &[],
            content_type: Some("application/xml"),
            amz_headers: &[],
            body: Some(body.into_bytes().into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 删除一个 bucket(必须为空)。
    pub async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &format!("/{bucket}"),
            query: &[],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取一个 bucket 的生命周期规则:`GET /{bucket}?lifecycle`。未配置时服务端返回
    /// `NoSuchLifecycleConfiguration`,视为空规则列表。
    pub async fn get_bucket_lifecycle(&self, bucket: &str) -> Result<Vec<LifecycleRule>> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &format!("/{bucket}"),
            query: &[("lifecycle".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(S3Error::Api { code, .. }) if code == "NoSuchLifecycleConfiguration" => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_lifecycle(&body)
    }

    /// 设置一个 bucket 的生命周期规则(整套替换):`PUT /{bucket}?lifecycle`,请求体为
    /// 完整的 `LifecycleConfiguration`。空列表时改发 `DELETE /{bucket}?lifecycle`——
    /// 服务端不接受没有任何 `Rule` 的 `LifecycleConfiguration`。
    pub async fn set_bucket_lifecycle(&self, bucket: &str, rules: &[LifecycleRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_lifecycle(bucket).await;
        }
        let body = build_lifecycle_xml(rules);
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &format!("/{bucket}"),
            query: &[("lifecycle".to_string(), String::new())],
            content_type: Some("application/xml"),
            amz_headers: &[],
            body: Some(body.into_bytes().into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &format!("/{bucket}"),
            query: &[("lifecycle".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取一个 bucket 的 CORS 规则:`GET /{bucket}?cors`。未配置时服务端返回
    /// `NoSuchCORSConfiguration`,视为空规则列表。
    pub async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &format!("/{bucket}"),
            query: &[("cors".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(S3Error::Api { code, .. }) if code == "NoSuchCORSConfiguration" => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_cors(&body)
    }

    /// 设置一个 bucket 的 CORS 规则(整套替换):`PUT /{bucket}?cors`。空列表时改发
    /// `DELETE /{bucket}?cors`——服务端不接受没有任何 `CORSRule` 的配置。
    pub async fn set_bucket_cors(&self, bucket: &str, rules: &[CorsRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_cors(bucket).await;
        }
        let body = build_cors_xml(rules);
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &format!("/{bucket}"),
            query: &[("cors".to_string(), String::new())],
            content_type: Some("application/xml"),
            amz_headers: &[],
            body: Some(body.into_bytes().into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &format!("/{bucket}"),
            query: &[("cors".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取一个 bucket 的静态网站托管配置:`GET /{bucket}?website`。未配置时服务端返回
    /// `NoSuchWebsiteConfiguration`,视为 `None`。
    pub async fn get_bucket_website(&self, bucket: &str) -> Result<Option<WebsiteConfig>> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &format!("/{bucket}"),
            query: &[("website".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(S3Error::Api { code, .. }) if code == "NoSuchWebsiteConfiguration" => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_website(&body).map(Some)
    }

    /// 设置(`Some`)或取消(`None`,发 `DELETE`)一个 bucket 的静态网站托管配置:
    /// `PUT`/`DELETE /{bucket}?website`。
    pub async fn set_bucket_website(
        &self,
        bucket: &str,
        config: Option<&WebsiteConfig>,
    ) -> Result<()> {
        let Some(config) = config else {
            return self.delete_bucket_website(bucket).await;
        };
        let body = build_website_xml(config);
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &format!("/{bucket}"),
            query: &[("website".to_string(), String::new())],
            content_type: Some("application/xml"),
            amz_headers: &[],
            body: Some(body.into_bytes().into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_website(&self, bucket: &str) -> Result<()> {
        let request = self.build_signed(RequestSpec {
            method: Method::DELETE,
            canonical_uri: &format!("/{bucket}"),
            query: &[("website".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 查询一个 bucket 是否已启用版本控制:`GET /{bucket}?versioning`。从未配置过时
    /// 响应是空的 `<VersioningConfiguration/>`(没有 `Status` 子元素),视为未启用。
    pub async fn get_bucket_versioning(&self, bucket: &str) -> Result<bool> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &format!("/{bucket}"),
            query: &[("versioning".to_string(), String::new())],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        let parsed: VersioningConfigurationXml = quick_xml::de::from_str(&body)
            .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed.status.as_deref() == Some("Enabled"))
    }

    /// 启用或暂停一个 bucket 的版本控制:`PUT /{bucket}?versioning`。
    pub async fn set_bucket_versioning(&self, bucket: &str, enabled: bool) -> Result<()> {
        let status = if enabled { "Enabled" } else { "Suspended" };
        let body =
            format!("<VersioningConfiguration><Status>{status}</Status></VersioningConfiguration>");
        let request = self.build_signed(RequestSpec {
            method: Method::PUT,
            canonical_uri: &format!("/{bucket}"),
            query: &[("versioning".to_string(), String::new())],
            content_type: Some("application/xml"),
            amz_headers: &[],
            body: Some(body.into_bytes().into()),
        })?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 列出一个对象的全部历史版本(含删除标记):`GET /{bucket}?versions&prefix={key}`。
    /// 只取第一页(最多 1000 条),按 `key` 精确匹配过滤(prefix 匹配可能带出前缀相同的
    /// 其它 key),再按修改时间从新到旧排序(合并 Version / DeleteMarker 两种元素后,
    /// 服务端给出的原始交错顺序在 quick_xml 反序列化时已经丢失,需要重新排一次)。
    pub async fn list_object_versions(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Vec<ObjectVersion>> {
        let request = self.build_signed(RequestSpec {
            method: Method::GET,
            canonical_uri: &format!("/{bucket}"),
            query: &[
                ("versions".to_string(), String::new()),
                ("prefix".to_string(), key.to_string()),
                ("max-keys".to_string(), MAX_KEYS.to_string()),
            ],
            content_type: None,
            amz_headers: &[],
            body: None,
        })?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_object_versions(&body, key)
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

/// 解析 `ListObjectVersions` 的 XML 响应:按 `key` 精确过滤,合并两种元素并按修改时间
/// 从新到旧排序。
///
/// `Version` 与 `DeleteMarker` 在真实响应里是交错出现的(同一个 key 的删除标记可能夹在
/// 两个内容版本中间)。quick_xml 的 serde 支持在"结构体里的重复元素字段被其它兄弟元素
/// 打断"时会报 `duplicate field`——哪怕那个兄弟元素类型压根不在结构体里、会被忽略。
/// 所以这里改用事件流手动扫描顶层子元素,一次只把**单个** `<Version>`/`<DeleteMarker>`
/// 片段丢给 `quick_xml::de` 反序列化(和其它地方"整个响应体就是一个对象"的用法一致),
/// 绕开"同一层多个重复字段"这个 quick_xml 的已知短板。
fn parse_object_versions(xml: &str, key: &str) -> Result<Vec<ObjectVersion>> {
    use quick_xml::events::Event;
    use quick_xml::name::QName;
    use quick_xml::Reader;

    let to_err = |e: quick_xml::Error| S3Error::Core(CoreError::InvalidResponse(e.to_string()));
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
                    .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
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
                    .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
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
    // 旧版 API 把 Prefix 直接放在 Rule 下(没有 Filter 包一层);两种都接受。
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
        .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
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

/// 一条 CORS 规则(本 crate 的本地表示;适配层负责映射到上层统一模型)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsRule {
    pub id: Option<String>,
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub max_age_seconds: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CorsConfigurationXml {
    #[serde(default, rename = "CORSRule")]
    cors_rule: Vec<CorsRuleXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CorsRuleXml {
    #[serde(rename = "ID", default)]
    id: Option<String>,
    #[serde(rename = "AllowedOrigin", default)]
    allowed_origin: Vec<String>,
    #[serde(rename = "AllowedMethod", default)]
    allowed_method: Vec<String>,
    #[serde(rename = "AllowedHeader", default)]
    allowed_header: Vec<String>,
    #[serde(rename = "ExposeHeader", default)]
    expose_header: Vec<String>,
    #[serde(default)]
    max_age_seconds: Option<u32>,
}

/// 解析 `GetBucketCors` 的 XML 响应。
fn parse_cors(xml: &str) -> Result<Vec<CorsRule>> {
    let doc: CorsConfigurationXml = quick_xml::de::from_str(xml)
        .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
    Ok(doc
        .cors_rule
        .into_iter()
        .map(|r| CorsRule {
            id: r.id,
            allowed_origins: r.allowed_origin,
            allowed_methods: r.allowed_method,
            allowed_headers: r.allowed_header,
            expose_headers: r.expose_header,
            max_age_seconds: r.max_age_seconds,
        })
        .collect())
}

/// 生成 `PutBucketCors` 的请求体 XML。
fn build_cors_xml(rules: &[CorsRule]) -> String {
    let mut body = String::from("<CORSConfiguration>");
    for r in rules {
        body.push_str("<CORSRule>");
        if let Some(id) = &r.id {
            body.push_str("<ID>");
            body.push_str(&xml_escape(id));
            body.push_str("</ID>");
        }
        for o in &r.allowed_origins {
            body.push_str("<AllowedOrigin>");
            body.push_str(&xml_escape(o));
            body.push_str("</AllowedOrigin>");
        }
        for m in &r.allowed_methods {
            body.push_str("<AllowedMethod>");
            body.push_str(&xml_escape(m));
            body.push_str("</AllowedMethod>");
        }
        for h in &r.allowed_headers {
            body.push_str("<AllowedHeader>");
            body.push_str(&xml_escape(h));
            body.push_str("</AllowedHeader>");
        }
        for h in &r.expose_headers {
            body.push_str("<ExposeHeader>");
            body.push_str(&xml_escape(h));
            body.push_str("</ExposeHeader>");
        }
        if let Some(secs) = r.max_age_seconds {
            body.push_str(&format!("<MaxAgeSeconds>{secs}</MaxAgeSeconds>"));
        }
        body.push_str("</CORSRule>");
    }
    body.push_str("</CORSConfiguration>");
    body
}

/// 静态网站托管配置(本 crate 的本地表示;适配层负责映射到上层统一模型)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteConfig {
    pub index_document: String,
    pub error_document: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WebsiteConfigurationXml {
    index_document: IndexDocumentXml,
    #[serde(default)]
    error_document: Option<ErrorDocumentXml>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct IndexDocumentXml {
    suffix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ErrorDocumentXml {
    key: String,
}

/// 解析 `GetBucketWebsite` 的 XML 响应。
fn parse_website(xml: &str) -> Result<WebsiteConfig> {
    let doc: WebsiteConfigurationXml = quick_xml::de::from_str(xml)
        .map_err(|e| S3Error::Core(CoreError::InvalidResponse(e.to_string())))?;
    Ok(WebsiteConfig {
        index_document: doc.index_document.suffix,
        error_document: doc.error_document.map(|e| e.key),
    })
}

/// 生成 `PutBucketWebsite` 的请求体 XML。
fn build_website_xml(config: &WebsiteConfig) -> String {
    let mut body = String::from("<WebsiteConfiguration><IndexDocument><Suffix>");
    body.push_str(&xml_escape(&config.index_document));
    body.push_str("</Suffix></IndexDocument>");
    if let Some(err) = &config.error_document {
        body.push_str("<ErrorDocument><Key>");
        body.push_str(&xml_escape(err));
        body.push_str("</Key></ErrorDocument>");
    }
    body.push_str("</WebsiteConfiguration>");
    body
}

/// 构造 ListObjectsV2 的查询参数(未编码;签名器内部会排序 + 编码)。
fn list_query(prefix: Option<&str>, delimiter: Option<&str>, token: &str) -> Vec<(String, String)> {
    let mut q = vec![
        ("list-type".to_string(), "2".to_string()),
        ("max-keys".to_string(), MAX_KEYS.to_string()),
    ];
    if let Some(p) = prefix.filter(|p| !p.is_empty()) {
        q.push(("prefix".to_string(), p.to_string()));
    }
    if let Some(d) = delimiter {
        q.push(("delimiter".to_string(), d.to_string()));
    }
    if !token.is_empty() {
        q.push(("continuation-token".to_string(), token.to_string()));
    }
    q
}

/// 下一页游标:截断时取 NextContinuationToken,否则无。
fn next_token(result: &ListBucketResult) -> Option<String> {
    if !result.is_truncated {
        return None;
    }
    result
        .next_continuation_token
        .clone()
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_sets_v2_and_optional_params() {
        let q = list_query(Some("photos/"), Some("/"), "");
        assert!(q.contains(&("list-type".to_string(), "2".to_string())));
        assert!(q.contains(&("prefix".to_string(), "photos/".to_string())));
        assert!(q.contains(&("delimiter".to_string(), "/".to_string())));
        assert!(!q.iter().any(|(k, _)| k == "continuation-token"));

        let q2 = list_query(None, None, "TOK");
        assert!(!q2.iter().any(|(k, _)| k == "prefix"));
        assert!(q2.contains(&("continuation-token".to_string(), "TOK".to_string())));
    }

    #[test]
    fn parses_list_v2_with_common_prefixes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>b</Name>
  <IsTruncated>true</IsTruncated>
  <NextContinuationToken>NEXT==</NextContinuationToken>
  <Contents>
    <Key>photos/cover.jpg</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"E1"</ETag>
    <Size>100</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
  <CommonPrefixes><Prefix>photos/2024/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let parsed: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.contents.len(), 1);
        assert_eq!(parsed.contents[0].size, 100);
        assert_eq!(parsed.common_prefixes[0].prefix, "photos/2024/");
        assert_eq!(next_token(&parsed).as_deref(), Some("NEXT=="));
    }

    #[test]
    fn next_token_none_when_not_truncated() {
        let parsed = ListBucketResult {
            is_truncated: false,
            next_continuation_token: Some("x".into()),
            contents: vec![],
            common_prefixes: vec![],
        };
        assert!(next_token(&parsed).is_none());
    }

    #[test]
    fn parses_list_all_buckets() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult>
  <Buckets>
    <Bucket><Name>my-bucket</Name><CreationDate>2020-01-01T00:00:00.000Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;
        let parsed: ListAllMyBucketsResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(parsed.buckets.bucket.len(), 1);
        assert_eq!(parsed.buckets.bucket[0].name, "my-bucket");
    }

    #[test]
    fn lifecycle_xml_round_trips() {
        let rules = vec![
            LifecycleRule {
                id: "archive-logs".into(),
                prefix: "logs/".into(),
                enabled: true,
                expiration_days: Some(365),
                transitions: vec![(30, "GLACIER".into()), (90, "DEEP_ARCHIVE".into())],
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
    fn parses_lifecycle_with_legacy_prefix_and_no_transitions() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<LifecycleConfiguration>
  <Rule>
    <ID>expire-tmp</ID>
    <Prefix>tmp/</Prefix>
    <Status>Enabled</Status>
    <Expiration><Days>7</Days></Expiration>
  </Rule>
</LifecycleConfiguration>"#;
        let parsed = parse_lifecycle(xml).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].prefix, "tmp/");
        assert_eq!(parsed[0].expiration_days, Some(7));
        assert!(parsed[0].transitions.is_empty());
    }

    #[test]
    fn empty_rules_build_valid_empty_configuration() {
        let xml = build_lifecycle_xml(&[]);
        assert_eq!(xml, "<LifecycleConfiguration></LifecycleConfiguration>");
    }

    #[test]
    fn versioning_configuration_parses_status() {
        let enabled: VersioningConfigurationXml = quick_xml::de::from_str(
            "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
        )
        .unwrap();
        assert_eq!(enabled.status.as_deref(), Some("Enabled"));

        // 从未配置过:空元素,没有 Status。
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
        // photo.jpg.bak 应被过滤掉(前缀匹配但 key 不精确相等)。
        assert_eq!(versions.len(), 2);
        // 按修改时间从新到旧:删除标记(3 月)在前,旧版本(1 月)在后。
        assert!(versions[0].is_delete_marker);
        assert_eq!(versions[0].version_id, "v-deleted");
        assert!(versions[0].is_latest);
        assert!(!versions[1].is_delete_marker);
        assert_eq!(versions[1].version_id, "v-old");
        assert_eq!(versions[1].size, 100);
        assert_eq!(versions[1].etag.as_deref(), Some("\"E1\""));
    }

    #[test]
    fn cors_xml_round_trips_multiple_rules() {
        let rules = vec![
            CorsRule {
                id: Some("allow-all-get".into()),
                allowed_origins: vec!["*".into()],
                allowed_methods: vec!["GET".into(), "HEAD".into()],
                allowed_headers: vec!["*".into()],
                expose_headers: vec!["ETag".into()],
                max_age_seconds: Some(3600),
            },
            CorsRule {
                id: None,
                allowed_origins: vec!["https://a.example".into(), "https://b.example".into()],
                allowed_methods: vec!["PUT".into()],
                allowed_headers: vec![],
                expose_headers: vec![],
                max_age_seconds: None,
            },
        ];
        let xml = build_cors_xml(&rules);
        let parsed = parse_cors(&xml).unwrap();
        assert_eq!(parsed, rules);
    }

    #[test]
    fn empty_cors_rules_build_valid_empty_configuration() {
        let xml = build_cors_xml(&[]);
        assert_eq!(xml, "<CORSConfiguration></CORSConfiguration>");
    }

    #[test]
    fn website_xml_round_trips_with_and_without_error_document() {
        let with_error = WebsiteConfig {
            index_document: "index.html".into(),
            error_document: Some("error.html".into()),
        };
        let xml = build_website_xml(&with_error);
        assert_eq!(parse_website(&xml).unwrap(), with_error);

        let without_error = WebsiteConfig {
            index_document: "home.htm".into(),
            error_document: None,
        };
        let xml2 = build_website_xml(&without_error);
        assert_eq!(parse_website(&xml2).unwrap(), without_error);
    }
}
