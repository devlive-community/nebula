//! 桶级操作。当前提供列举对象(GET Bucket / List Objects,V1)。
//!
//! V1 的 `prefix` / `marker` / `max-keys` / `delimiter` 都是普通查询参数,不参与
//! 签名(CanonicalizedResource 只是 `/{bucket}/`),因此签名逻辑很简单。翻页借助
//! [`cloud_core::paginate`]:游标即 `marker`,截断时以 `NextMarker` 或本页最后一个
//! Key 作为下一页游标。

use cloud_core::{paginate, CoreError, Page};
use futures::Stream;
use reqwest::header::{AUTHORIZATION, DATE};
use reqwest::{Method, Request, Response, Url};
use serde::Deserialize;

use crate::client::OssClient;
use crate::error::{OssError, Result};
use crate::multipart::{xml_escape, PartRequest};
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
        let resp = self.send_list(bucket, prefix, &marker, None, &date).await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
    ///
    /// 适合文件管理器逐层浏览;`prefix` 应以 `/` 结尾(如 `photos/`),根为 `None`。
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
        let resp = self
            .send_list(bucket, prefix, &marker, Some("/"), &date)
            .await?;
        let body = resp.text().await.map_err(CoreError::from)?;

        let parsed: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
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

    /// 发送一次列举请求;若被服务端要求换 endpoint(跨区域「must be addressed using the
    /// specified endpoint」),从错误里学到正确 endpoint、缓存后重试一次,自举区域缓存——
    /// 即便本会话没先列过桶列表,首次进某桶也能自动纠正到正确区域。
    async fn send_list(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        marker: &str,
        delimiter: Option<&str>,
        date: &str,
    ) -> Result<Response> {
        let request = self.build_list_request(bucket, prefix, marker, delimiter, date)?;
        match check_status(self.http().execute(request).await?).await {
            Ok(resp) => Ok(resp),
            Err(e) => match redirect_endpoint(&e, bucket) {
                Some(endpoint) => {
                    self.cache_bucket_endpoint(bucket, &endpoint);
                    let retry = self.build_list_request(bucket, prefix, marker, delimiter, date)?;
                    check_status(self.http().execute(retry).await?).await
                }
                None => Err(e),
            },
        }
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

    /// 读取一个 bucket 的生命周期规则:`GET /{bucket}/?lifecycle`。未配置生命周期时
    /// 服务端返回 `NoSuchLifecycle`,视为空规则列表。
    pub async fn get_bucket_lifecycle(&self, bucket: &str) -> Result<Vec<LifecycleRule>> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::GET,
                key: "",
                subresources: &[("lifecycle", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(OssError::Api { code, .. }) if code == "NoSuchLifecycle" => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_lifecycle(&body)
    }

    /// 设置一个 bucket 的生命周期规则(整套替换):`PUT /{bucket}/?lifecycle`。空列表时
    /// 改发 `DELETE /{bucket}/?lifecycle`(OSS 不接受没有任何 `Rule` 的配置)。
    pub async fn set_bucket_lifecycle(&self, bucket: &str, rules: &[LifecycleRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_lifecycle(bucket).await;
        }
        let date = now_gmt();
        let body = bytes::Bytes::from(build_lifecycle_xml(rules));
        let content_md5 = cloud_core::crypto::content_md5(&body);
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key: "",
                subresources: &[("lifecycle", None)],
                content_type: Some("application/xml"),
                content_md5: Some(&content_md5),
                body: Some(body),
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::DELETE,
                key: "",
                subresources: &[("lifecycle", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 查询一个 bucket 是否已启用版本控制:`GET /{bucket}/?versioning`。从未配置过时
    /// 响应是空的 `<VersioningConfiguration/>`(没有 `Status` 子元素),视为未启用。
    pub async fn get_bucket_versioning(&self, bucket: &str) -> Result<bool> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::GET,
                key: "",
                subresources: &[("versioning", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        let parsed: VersioningConfigurationXml = quick_xml::de::from_str(&body)
            .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
        Ok(parsed.status.as_deref() == Some("Enabled"))
    }

    /// 启用或暂停一个 bucket 的版本控制:`PUT /{bucket}/?versioning`。
    pub async fn set_bucket_versioning(&self, bucket: &str, enabled: bool) -> Result<()> {
        let date = now_gmt();
        let status = if enabled { "Enabled" } else { "Suspended" };
        let body =
            format!("<VersioningConfiguration><Status>{status}</Status></VersioningConfiguration>");
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key: "",
                subresources: &[("versioning", None)],
                content_type: Some("application/xml"),
                content_md5: None,
                body: Some(body.into()),
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 列出一个对象的全部历史版本(含删除标记):`GET /{bucket}/?versions&prefix={key}`。
    /// 只取第一页,按 `key` 精确匹配过滤,按修改时间从新到旧排序。
    pub async fn list_object_versions(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Vec<ObjectVersion>> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::GET,
                key: "",
                subresources: &[
                    ("versions", None),
                    ("prefix", Some(key)),
                    ("max-keys", Some(MAX_KEYS)),
                ],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = check_status(self.http().execute(request).await?).await?;
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_object_versions(&body, key)
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
            .map(|b| {
                // 缓存桶 → 区域 endpoint(优先外网 endpoint,否则据 Location 推导),
                // 之后访问该桶自动路由到正确区域,避免「必须用指定 endpoint 访问」的错误。
                let endpoint = if !b.extranet_endpoint.is_empty() {
                    b.extranet_endpoint.clone()
                } else if !b.location.is_empty() {
                    format!("{}.aliyuncs.com", b.location)
                } else {
                    String::new()
                };
                self.cache_bucket_endpoint(&b.name, &endpoint);
                BucketSummary {
                    name: b.name,
                    location: b.location,
                    creation_date: b.creation_date,
                    storage_class: b.storage_class,
                    endpoint: b.extranet_endpoint,
                }
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

/// URL 解码(配合 `encoding-type=url`),非法字节按 lossy 处理。
fn url_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// 从「必须用指定 endpoint 访问」的错误里取出正确的区域 endpoint(去掉可能的 `{bucket}.`
/// 前缀,得到不含桶名的区域 endpoint)。非该类错误返回 None。
fn redirect_endpoint(err: &OssError, bucket: &str) -> Option<String> {
    match err {
        OssError::Api {
            endpoint: Some(ep), ..
        } if !ep.is_empty() => {
            let prefix = format!("{bucket}.");
            Some(ep.strip_prefix(&prefix).unwrap_or(ep).to_string())
        }
        _ => None,
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
    prefix: String,
    status: String,
    #[serde(default, rename = "Transition")]
    transition: Vec<TransitionXml>,
    #[serde(default)]
    expiration: Option<ExpirationXml>,
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

/// 解析 `GetBucketLifecycle` 的 XML 响应(OSS 的 `Rule` 下 `Prefix` 是直接子元素,
/// 不像 S3 那样包一层 `Filter`)。
fn parse_lifecycle(xml: &str) -> Result<Vec<LifecycleRule>> {
    let doc: LifecycleConfigurationXml = quick_xml::de::from_str(xml)
        .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
    Ok(doc
        .rule
        .into_iter()
        .map(|r| LifecycleRule {
            id: r.id,
            prefix: r.prefix,
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

/// 生成 `PutBucketLifecycle` 的请求体 XML。
fn build_lifecycle_xml(rules: &[LifecycleRule]) -> String {
    let mut body = String::from("<LifecycleConfiguration>");
    for r in rules {
        body.push_str("<Rule><ID>");
        body.push_str(&xml_escape(&r.id));
        body.push_str("</ID><Prefix>");
        body.push_str(&xml_escape(&r.prefix));
        body.push_str("</Prefix><Status>");
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

/// 解析 `GetBucketVersions` 的 XML 响应:按 `key` 精确过滤,按修改时间从新到旧排序。
///
/// `Version` 与 `DeleteMarker` 在真实响应里是交错出现的,quick_xml 的 serde 支持在
/// "结构体里的重复元素字段被其它兄弟元素打断"时会报 `duplicate field`(即使那个兄弟
/// 元素不在结构体里、会被忽略也一样)。所以改用事件流手动扫描顶层子元素,一次只把
/// 单个 `<Version>`/`<DeleteMarker>` 片段丢给 `quick_xml::de` 反序列化。
fn parse_object_versions(xml: &str, key: &str) -> Result<Vec<ObjectVersion>> {
    use quick_xml::events::Event;
    use quick_xml::name::QName;
    use quick_xml::Reader;

    let to_err = |e: quick_xml::Error| OssError::Core(CoreError::InvalidResponse(e.to_string()));
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
                    .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
                    .map_err(|e| OssError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
    fn redirect_endpoint_strips_bucket_prefix() {
        let api = |endpoint: Option<&str>| OssError::Api {
            status: 403,
            code: "AccessDenied".into(),
            message: String::new(),
            request_id: None,
            endpoint: endpoint.map(str::to_string),
        };
        // 带桶前缀 → 去掉,得区域 endpoint。
        assert_eq!(
            redirect_endpoint(&api(Some("bkt.oss-cn-beijing.aliyuncs.com")), "bkt").as_deref(),
            Some("oss-cn-beijing.aliyuncs.com")
        );
        // 不含桶前缀 → 原样。
        assert_eq!(
            redirect_endpoint(&api(Some("oss-cn-beijing.aliyuncs.com")), "bkt").as_deref(),
            Some("oss-cn-beijing.aliyuncs.com")
        );
        // 无 endpoint → None。
        assert_eq!(redirect_endpoint(&api(None), "bkt"), None);
    }

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
            .build_list_request("oss-example", Some("photos/"), "cat.jpg", None, date)
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
            .build_list_request("b", Some(""), "", None, "date")
            .unwrap();
        let query = req.url().query().unwrap();
        assert!(query.contains("max-keys=1000"));
        assert!(!query.contains("prefix="));
        assert!(!query.contains("marker="));
        assert!(!query.contains("delimiter="));
    }

    #[test]
    fn list_request_sets_encoding_type() {
        let client = test_client();
        let req = client
            .build_list_request("b", None, "", None, "date")
            .unwrap();
        assert!(req.url().query().unwrap().contains("encoding-type=url"));
    }

    #[test]
    fn url_decode_restores_specials() {
        assert_eq!(url_decode("dir/%20a.mp4"), "dir/ a.mp4");
        assert_eq!(url_decode("%E5%9B%BE%E7%89%87.png"), "图片.png");
        assert_eq!(url_decode("plain.txt"), "plain.txt");
    }

    #[test]
    fn list_dir_request_sets_delimiter() {
        let client = test_client();
        let req = client
            .build_list_request("b", Some("photos/"), "", Some("/"), "date")
            .unwrap();
        let query = req.url().query().unwrap();
        assert!(query.contains("delimiter=%2F"));
        assert!(query.contains("prefix=photos%2F"));
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
    <StorageClass>Standard</StorageClass>
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
            common_prefixes: vec![],
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
            common_prefixes: vec![],
        };
        assert_eq!(next_marker(&fallback).as_deref(), Some("z.txt"));
    }

    #[test]
    fn lifecycle_xml_round_trips() {
        let rules = vec![
            LifecycleRule {
                id: "archive-logs".into(),
                prefix: "logs/".into(),
                enabled: true,
                expiration_days: Some(365),
                transitions: vec![(30, "Archive".into()), (180, "ColdArchive".into())],
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
    fn set_bucket_lifecycle_request_signs_subresource() {
        let client = test_client();
        // 生命周期请求走 build_part_request,和 initiate_multipart_upload 用的是同一个
        // 签名路径;这里只验证子资源确实进了 CanonicalizedResource(不匹配就会 403)。
        let date = "Thu, 17 Nov 2005 18:49:58 GMT";
        let req = client
            .build_part_request(
                "b",
                PartRequest {
                    method: Method::PUT,
                    key: "",
                    subresources: &[("lifecycle", None)],
                    content_type: Some("application/xml"),
                    content_md5: None,
                    body: Some(bytes::Bytes::from_static(
                        b"<LifecycleConfiguration></LifecycleConfiguration>",
                    )),
                },
                date,
            )
            .unwrap();
        let canonical = sign::canonicalized_resource("b", "", &[("lifecycle", None)]);
        assert_eq!(canonical, "/b/?lifecycle");
        let sts = sign::string_to_sign("PUT", "", "application/xml", date, "", &canonical);
        let expected =
            sign::authorization(client.access_key_id(), client.access_key_secret(), &sts);
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap().to_str().unwrap(),
            expected
        );
        assert!(req.url().as_str().ends_with("?lifecycle"));
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
