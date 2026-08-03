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
use reqwest::{Method, Request, Response, Url};
use serde::Deserialize;

use crate::client::ObsClient;
use crate::error::{ObsError, Result};
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
        let resp = self.send_list(bucket, prefix, &marker, None, &date).await?;
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
        let resp = self
            .send_list(bucket, prefix, &marker, Some("/"), &date)
            .await?;
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

    /// 发送一次列举请求;若被服务端要求换 endpoint(跨区域),从错误里学到正确 endpoint、
    /// 缓存后重试一次,自举区域缓存(即便本会话没先列过桶列表)。
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

    /// 读取一个 bucket 的生命周期规则:`GET /{bucket}/?lifecycle`。未配置生命周期时
    /// 服务端返回 `NoSuchLifecycleConfiguration`,视为空规则列表。
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
            Err(ObsError::Api { code, .. }) if code == "NoSuchLifecycleConfiguration" => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_lifecycle(&body)
    }

    /// 设置一个 bucket 的生命周期规则(整套替换):`PUT /{bucket}/?lifecycle`。空列表时
    /// 改发 `DELETE /{bucket}/?lifecycle`(OBS 不接受没有任何 `Rule` 的配置)。
    /// OBS 的 V2 签名不对 body 取哈希,带 XML body 不影响签名(建桶的 `Location` body
    /// 已印证过这一点)。
    pub async fn set_bucket_lifecycle(&self, bucket: &str, rules: &[LifecycleRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_lifecycle(bucket).await;
        }
        let date = now_gmt();
        let body = bytes::Bytes::from(build_lifecycle_xml(rules));
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key: "",
                subresources: &[("lifecycle", None)],
                content_type: Some("application/xml"),
                content_md5: None,
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

    /// 读取一个 bucket 的 CORS 规则:`GET /{bucket}/?cors`。未配置时服务端返回
    /// `NoSuchCORSConfiguration`,视为空规则列表。
    pub async fn get_bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::GET,
                key: "",
                subresources: &[("cors", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(ObsError::Api { code, .. }) if code == "NoSuchCORSConfiguration" => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_cors(&body)
    }

    /// 设置一个 bucket 的 CORS 规则(整套替换):`PUT /{bucket}/?cors`。空列表时改发
    /// `DELETE /{bucket}/?cors`(OBS 不接受没有任何 `CORSRule` 的配置)。
    pub async fn set_bucket_cors(&self, bucket: &str, rules: &[CorsRule]) -> Result<()> {
        if rules.is_empty() {
            return self.delete_bucket_cors(bucket).await;
        }
        let date = now_gmt();
        let body = bytes::Bytes::from(build_cors_xml(rules));
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key: "",
                subresources: &[("cors", None)],
                content_type: Some("application/xml"),
                content_md5: None,
                body: Some(body),
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::DELETE,
                key: "",
                subresources: &[("cors", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    /// 读取一个 bucket 的静态网站托管配置:`GET /{bucket}/?website`。未配置时,有的服务端
    /// 返回 `NoSuchWebsiteConfiguration` 错误,有的直接 200 返回一个没有 `IndexDocument`
    /// 的空 `<WebsiteConfiguration/>`——两种情况都视为 `None`。
    pub async fn get_bucket_website(&self, bucket: &str) -> Result<Option<WebsiteConfig>> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::GET,
                key: "",
                subresources: &[("website", None)],
                content_type: None,
                content_md5: None,
                body: None,
            },
            &date,
        )?;
        let resp = match check_status(self.http().execute(request).await?).await {
            Ok(resp) => resp,
            Err(ObsError::Api { code, .. }) if code == "NoSuchWebsiteConfiguration" => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        let body = resp.text().await.map_err(CoreError::from)?;
        parse_website(&body)
    }

    /// 设置(`Some`)或取消(`None`,发 `DELETE`)一个 bucket 的静态网站托管配置:
    /// `PUT`/`DELETE /{bucket}/?website`。
    pub async fn set_bucket_website(
        &self,
        bucket: &str,
        config: Option<&WebsiteConfig>,
    ) -> Result<()> {
        let Some(config) = config else {
            return self.delete_bucket_website(bucket).await;
        };
        let date = now_gmt();
        let body = bytes::Bytes::from(build_website_xml(config));
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::PUT,
                key: "",
                subresources: &[("website", None)],
                content_type: Some("application/xml"),
                content_md5: None,
                body: Some(body),
            },
            &date,
        )?;
        check_status(self.http().execute(request).await?).await?;
        Ok(())
    }

    async fn delete_bucket_website(&self, bucket: &str) -> Result<()> {
        let date = now_gmt();
        let request = self.build_part_request(
            bucket,
            PartRequest {
                method: Method::DELETE,
                key: "",
                subresources: &[("website", None)],
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
            .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
            .map(|b| {
                // 缓存桶 → 区域 endpoint(据 Location 推导),之后访问该桶自动路由到正确区域。
                if !b.location.is_empty() {
                    let endpoint = if b.location.contains(".myhuaweicloud.com") {
                        b.location.clone()
                    } else {
                        format!("obs.{}.myhuaweicloud.com", b.location)
                    };
                    self.cache_bucket_endpoint(&b.name, &endpoint);
                }
                BucketSummary {
                    name: b.name,
                    location: b.location,
                    creation_date: b.creation_date,
                }
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

/// 从跨区域错误里取出正确的区域 endpoint(去掉可能的 `{bucket}.` 前缀)。非该类错误返回 None。
fn redirect_endpoint(err: &ObsError, bucket: &str) -> Option<String> {
    match err {
        ObsError::Api {
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
    filter: Option<FilterXml>,
    // 兼容直接把 Prefix 放在 Rule 下(不经 Filter 包一层)的响应。
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
        .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
        .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WebsiteConfigurationXml {
    #[serde(default)]
    index_document: Option<IndexDocumentXml>,
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

/// 解析 `GetBucketWebsite` 的 XML 响应。没有 `IndexDocument` 视为未配置(`None`)——
/// 部分服务端不发错误码,直接 200 返回一个空的 `<WebsiteConfiguration/>`。
fn parse_website(xml: &str) -> Result<Option<WebsiteConfig>> {
    let doc: WebsiteConfigurationXml = quick_xml::de::from_str(xml)
        .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
    Ok(doc.index_document.map(|idx| WebsiteConfig {
        index_document: idx.suffix,
        error_document: doc.error_document.map(|e| e.key),
    }))
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
/// `Version` 与 `DeleteMarker` 交错出现,quick_xml 的 struct+Vec 反序列化处理不了这种
/// 交错,改用事件流手动扫描顶层子元素、逐个片段反序列化(见 s3-core 的同名函数注释)。
fn parse_object_versions(xml: &str, key: &str) -> Result<Vec<ObjectVersion>> {
    use quick_xml::events::Event;
    use quick_xml::name::QName;
    use quick_xml::Reader;

    let to_err = |e: quick_xml::Error| ObsError::Core(CoreError::InvalidResponse(e.to_string()));
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
                    .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
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
                    .map_err(|e| ObsError::Core(CoreError::InvalidResponse(e.to_string())))?;
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

    #[test]
    fn lifecycle_xml_round_trips() {
        let rules = vec![
            LifecycleRule {
                id: "archive-logs".into(),
                prefix: "logs/".into(),
                enabled: true,
                expiration_days: Some(365),
                transitions: vec![(30, "WARM".into()), (90, "COLD".into())],
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
    fn parses_lifecycle_with_legacy_prefix() {
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
        assert_eq!(parse_website(&xml).unwrap(), Some(with_error));

        let without_error = WebsiteConfig {
            index_document: "home.htm".into(),
            error_document: None,
        };
        let xml2 = build_website_xml(&without_error);
        assert_eq!(parse_website(&xml2).unwrap(), Some(without_error));
    }

    #[test]
    fn website_not_configured_parses_to_none() {
        assert_eq!(parse_website("<WebsiteConfiguration/>").unwrap(), None);
    }
}
