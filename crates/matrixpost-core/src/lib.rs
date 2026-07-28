//! Portable, side-effect-free publication domain model and durable state ports.
//!
//! This crate deliberately stores account *metadata* only. Browser sessions and
//! passwords belong to provider implementations and are never serialised here.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
    time::Duration,
};

use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// One of the eight platforms supported by the upstream wire protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "dy", alias = "douyin", alias = "Douyin", alias = "抖音")]
    Douyin,
    #[serde(
        rename = "sph",
        alias = "wechat_channels",
        alias = "wechat",
        alias = "wechat-channels",
        alias = "视频号",
        alias = "微信视频号"
    )]
    WechatChannels,
    #[serde(
        rename = "blbl",
        alias = "bilibili",
        alias = "Bilibili",
        alias = "哔哩哔哩",
        alias = "b站"
    )]
    Bilibili,
    #[serde(rename = "bjh", alias = "baijiahao", alias = "百家号")]
    Baijiahao,
    #[serde(rename = "tt", alias = "toutiao", alias = "今日头条", alias = "头条")]
    Toutiao,
    #[serde(rename = "ks", alias = "kuaishou", alias = "快手")]
    Kuaishou,
    #[serde(rename = "xhs", alias = "xiaohongshu", alias = "小红书")]
    Xiaohongshu,
    #[serde(
        rename = "fqsp",
        alias = "fanqie_video",
        alias = "fanqie",
        alias = "fanqie-video",
        alias = "fq",
        alias = "番茄视频"
    )]
    FanqieVideo,
}

impl Platform {
    /// The complete, fixed upstream platform set.
    pub const ALL: [Self; 8] = [
        Self::Douyin,
        Self::WechatChannels,
        Self::Bilibili,
        Self::Baijiahao,
        Self::Toutiao,
        Self::Kuaishou,
        Self::Xiaohongshu,
        Self::FanqieVideo,
    ];

    /// Returns the lossless canonical upstream code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Douyin => "dy",
            Self::WechatChannels => "sph",
            Self::Bilibili => "blbl",
            Self::Baijiahao => "bjh",
            Self::Toutiao => "tt",
            Self::Kuaishou => "ks",
            Self::Xiaohongshu => "xhs",
            Self::FanqieVideo => "fqsp",
        }
    }
}

impl FromStr for Platform {
    type Err = DomainError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dy" | "douyin" | "抖音" => Ok(Self::Douyin),
            "sph" | "wechat_channels" | "wechat-channels" | "wechat" | "视频号" | "微信视频号" => {
                Ok(Self::WechatChannels)
            }
            "blbl" | "bilibili" | "哔哩哔哩" | "b站" => Ok(Self::Bilibili),
            "bjh" | "baijiahao" | "百家号" => Ok(Self::Baijiahao),
            "tt" | "toutiao" | "今日头条" | "头条" => Ok(Self::Toutiao),
            "ks" | "kuaishou" | "快手" => Ok(Self::Kuaishou),
            "xhs" | "xiaohongshu" | "小红书" => Ok(Self::Xiaohongshu),
            "fqsp" | "fanqie_video" | "fanqie-video" | "fanqie" | "fq" | "番茄视频" => {
                Ok(Self::FanqieVideo)
            }
            _ => Err(DomainError::UnknownPlatform(value.to_owned())),
        }
    }
}

/// Authentication visibility without retaining session credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    LoggedIn,
    Expired,
    LoggedOut,
    Unavailable,
}

/// A safe account summary. `id` is stable and contains no session material.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub platform: Platform,
    pub display_name: String,
    pub status: AccountStatus,
    /// Non-secret account route used by upstream list APIs.
    pub phone: String,
    /// Non-secret session-partition label; must start with `persist:`.
    pub partition: String,
}

/// The article platform represented separately from the fixed eight-video set.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticlePlatform {
    Juejin,
}

/// Credential-free visibility for an article account.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleAccountStatus {
    LoggedIn,
    Expired,
    LoggedOut,
    Unavailable,
}

/// Safe article-account metadata. It never contains cookies, tokens, or sessions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleAccount {
    pub id: String,
    pub platform: ArticlePlatform,
    pub display_name: String,
    pub status: ArticleAccountStatus,
    /// Non-secret account route used by upstream list APIs.
    pub phone: String,
    /// Non-secret session-partition label; must start with `persist:`.
    pub partition: String,
}

/// Account routing requested by MatrixMedia's phone/partition fields.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,
}

impl AccountSelection {
    /// Returns true when this selection carries no account-routing data.
    pub const fn is_empty(&self) -> bool {
        self.phone.is_none() && self.partition.is_none()
    }
}

/// A local wall-clock schedule. It intentionally has no implicit timezone conversion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalSchedule(pub String);

impl LocalSchedule {
    /// Parses the only accepted upstream local timestamp form.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| DomainError::InvalidSchedule(value.to_owned()))?;
        Ok(Self(value.to_owned()))
    }
    /// Parses this schedule into a typed local timestamp without performing I/O.
    pub fn as_naive(&self) -> Result<NaiveDateTime, DomainError> {
        NaiveDateTime::parse_from_str(&self.0, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| DomainError::InvalidSchedule(self.0.clone()))
    }
}
impl FromStr for LocalSchedule {
    type Err = DomainError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Media input accepted by a publication request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MediaSource {
    LocalFile(PathBuf),
    RemoteUrl(Url),
}

/// WeChat Channels product/link metadata from the upstream request format.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WechatLink {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_value: Option<String>,
}

/// A platform-specific value that overrides the common publication fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlatformOverride {
    pub platform: Platform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creative_statement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wechat_link: Option<WechatLink>,
}

/// A complete upstream-compatible publication command. Validation has no side effects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublishRequest {
    pub source: MediaSource,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub draft: bool,
    /// Upstream `bt2` compatibility toggle, retained losslessly for adapters.
    #[serde(default)]
    pub bt2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<LocalSchedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(default, skip_serializing_if = "AccountSelection::is_empty")]
    pub account: AccountSelection,
    #[serde(default)]
    pub wechat_link: WechatLink,
    #[serde(default)]
    pub overrides: Vec<PlatformOverride>,
    pub targets: Vec<Platform>,
}

/// External MatrixMedia JSON compatibility input, mapped before durable state.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamPublishDto {
    #[serde(default)]
    pub platform: Option<UpstreamTargets>,
    #[serde(default)]
    pub platforms: Option<UpstreamTargets>,
    pub file: String,
    pub title: String,
    #[serde(default)]
    pub tags: UpstreamTags,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub partition: Option<String>,
    #[serde(default)]
    pub publish_at: Option<String>,
    #[serde(default)]
    pub sph_product_id: Option<String>,
    #[serde(default)]
    pub sph_link: Option<UpstreamSphLink>,
    #[serde(default)]
    pub platform_options: Option<UpstreamPlatformOptions>,
    #[serde(default)]
    pub creative_statement: Option<String>,
    #[serde(default)]
    pub creative_statements: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub bt2: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub short_title: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum UpstreamTargets {
    Name(String),
    Names(Vec<String>),
    Target(UpstreamPlatformOption),
    Targets(Vec<UpstreamPlatformOption>),
}
impl UpstreamTargets {
    fn into_options(self) -> Vec<UpstreamPlatformOption> {
        match self {
            Self::Name(platform) => vec![UpstreamPlatformOption {
                platform,
                ..Default::default()
            }],
            Self::Names(items) => items
                .into_iter()
                .map(|platform| UpstreamPlatformOption {
                    platform,
                    ..Default::default()
                })
                .collect(),
            Self::Target(item) => vec![item],
            Self::Targets(items) => items,
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
pub enum UpstreamTags {
    Text(String),
    Values(Vec<String>),
    #[default]
    Empty,
}
impl UpstreamTags {
    fn values(self) -> Vec<String> {
        match self {
            Self::Text(value) => value
                .split([' ', ','])
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect(),
            Self::Values(value) => value,
            Self::Empty => Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum UpstreamPlatformOptions {
    Map(BTreeMap<String, UpstreamPlatformOption>),
    Values(Vec<UpstreamPlatformOption>),
}
impl UpstreamPlatformOptions {
    fn into_options(self) -> Vec<UpstreamPlatformOption> {
        match self {
            Self::Values(value) => value,
            Self::Map(value) => value
                .into_iter()
                .map(|(platform, mut option)| {
                    if option.platform.is_empty() {
                        option.platform = platform;
                    }
                    option
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamSphLink {
    #[serde(default, alias = "type")]
    pub link_type: Option<String>,
    #[serde(default, alias = "value")]
    pub link_value: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamPlatformOption {
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub partition: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub short_title: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub creative_statement: Option<String>,
    #[serde(default)]
    pub link: Option<UpstreamSphLink>,
}
impl TryFrom<UpstreamPublishDto> for PublishRequest {
    type Error = DomainError;
    fn try_from(value: UpstreamPublishDto) -> Result<Self, Self::Error> {
        let mut options = value
            .platform
            .into_iter()
            .chain(value.platforms)
            .flat_map(UpstreamTargets::into_options)
            .collect::<Vec<_>>();
        if let Some(platform_options) = value.platform_options {
            options.extend(platform_options.into_options());
        }
        let _parsed_targets = options
            .iter()
            .map(|item| Platform::from_str(&item.platform))
            .collect::<Result<Vec<_>, _>>()?;
        let mut overrides = options
            .into_iter()
            .map(|item| {
                Ok(PlatformOverride {
                    platform: Platform::from_str(&item.platform)?,
                    title: item.title,
                    short_title: item.short_title,
                    tags: item.tags,
                    creative_statement: item.creative_statement,
                    account: Some(AccountSelection {
                        phone: item.phone.or_else(|| value.phone.clone()),
                        partition: item.partition.or_else(|| value.partition.clone()),
                    }),
                    wechat_link: item.link.map(|link| WechatLink {
                        product_id: None,
                        link_type: link.link_type,
                        link_value: link.link_value,
                    }),
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        for (name, statement) in value.creative_statements.unwrap_or_default() {
            let platform = Platform::from_str(&name)?;
            if let Some(target) = overrides
                .iter_mut()
                .find(|target| target.platform == platform)
            {
                target.creative_statement = Some(statement);
            } else {
                overrides.push(PlatformOverride {
                    platform,
                    title: None,
                    short_title: None,
                    tags: None,
                    creative_statement: Some(statement),
                    account: None,
                    wechat_link: None,
                });
            }
        }
        let mut normalized = BTreeMap::<Platform, PlatformOverride>::new();
        for override_value in overrides {
            normalized
                .entry(override_value.platform)
                .and_modify(|current| {
                    if override_value.title.is_some() {
                        current.title = override_value.title.clone();
                    }
                    if override_value.short_title.is_some() {
                        current.short_title = override_value.short_title.clone();
                    }
                    if override_value.tags.is_some() {
                        current.tags = override_value.tags.clone();
                    }
                    if override_value.creative_statement.is_some() {
                        current.creative_statement = override_value.creative_statement.clone();
                    }
                    if override_value.account.is_some() {
                        current.account = override_value.account.clone();
                    }
                    if override_value.wechat_link.is_some() {
                        current.wechat_link = override_value.wechat_link.clone();
                    }
                })
                .or_insert(override_value);
        }
        let overrides = normalized.into_values().collect::<Vec<_>>();
        let targets = overrides
            .iter()
            .map(|item| item.platform)
            .collect::<Vec<_>>();
        let source = match Url::parse(&value.file) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
            Ok(url) => return Err(DomainError::UnsupportedRemoteScheme(url.scheme().into())),
            Err(_) => MediaSource::LocalFile(value.file.into()),
        };
        let option_link = overrides
            .iter()
            .find(|item| item.platform == Platform::WechatChannels)
            .and_then(|item| item.wechat_link.clone());
        let product_link_type = value.sph_product_id.as_ref().map(|_| "product".to_owned());
        let mut request = PublishRequest {
            source,
            title: value.title,
            short_title: value.short_title,
            tags: value.tags.values(),
            address: value.address,
            draft: value.draft,
            bt2: value.bt2,
            scheduled_at: value
                .publish_at
                .as_deref()
                .map(LocalSchedule::parse)
                .transpose()?,
            task_name: value.name,
            account: AccountSelection {
                phone: value.phone,
                partition: value.partition,
            },
            wechat_link: WechatLink {
                product_id: value.sph_product_id,
                link_type: value
                    .sph_link
                    .as_ref()
                    .and_then(|item| item.link_type.clone())
                    .or_else(|| option_link.as_ref().and_then(|item| item.link_type.clone()))
                    .or(product_link_type),
                link_value: value.sph_link.and_then(|item| item.link_value).or_else(|| {
                    option_link
                        .as_ref()
                        .and_then(|item| item.link_value.clone())
                }),
            },
            overrides,
            targets,
        };
        if let Some(statement) = value.creative_statement {
            for override_value in &mut request.overrides {
                if override_value.creative_statement.is_none() {
                    override_value.creative_statement = Some(statement.clone());
                }
            }
        }
        request.validate()?;
        Ok(request)
    }
}

/// Display metadata for the upstream API; provider automation remains unavailable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlatformMetadata {
    pub code: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub automation_available: bool,
}
impl Platform {
    pub const fn metadata(self) -> PlatformMetadata {
        match self {
            Self::Douyin => PlatformMetadata {
                code: "dy",
                name: "抖音",
                aliases: &["douyin", "抖音"],
                automation_available: false,
            },
            Self::WechatChannels => PlatformMetadata {
                code: "sph",
                name: "微信视频号",
                aliases: &["wechat", "视频号"],
                automation_available: false,
            },
            Self::Bilibili => PlatformMetadata {
                code: "blbl",
                name: "哔哩哔哩",
                aliases: &["bilibili", "b站"],
                automation_available: false,
            },
            Self::Baijiahao => PlatformMetadata {
                code: "bjh",
                name: "百家号",
                aliases: &["baijiahao", "百家号"],
                automation_available: false,
            },
            Self::Toutiao => PlatformMetadata {
                code: "tt",
                name: "今日头条",
                aliases: &["toutiao", "头条"],
                automation_available: false,
            },
            Self::Kuaishou => PlatformMetadata {
                code: "ks",
                name: "快手",
                aliases: &["kuaishou", "快手"],
                automation_available: false,
            },
            Self::Xiaohongshu => PlatformMetadata {
                code: "xhs",
                name: "小红书",
                aliases: &["xiaohongshu", "小红书"],
                automation_available: false,
            },
            Self::FanqieVideo => PlatformMetadata {
                code: "fqsp",
                name: "番茄视频",
                aliases: &["fanqie", "fq", "番茄视频"],
                automation_available: false,
            },
        }
    }
}

impl PublishRequest {
    /// Returns a copy safe to cross a local runner boundary.
    ///
    /// Account routing is resolved by an embedding before dispatch and is
    /// deliberately never exposed to a runner or browser adapter.
    pub fn runner_safe(&self) -> Self {
        let mut safe = self.clone();
        safe.account = AccountSelection::default();
        for override_value in &mut safe.overrides {
            override_value.account = None;
        }
        safe
    }

    /// Returns true if this request still carries account routing.
    pub fn has_account_routing(&self) -> bool {
        !self.account.is_empty()
            || self
                .overrides
                .iter()
                .filter_map(|override_value| override_value.account.as_ref())
                .any(|account| !account.is_empty())
    }

    /// Rejects malformed data before repository, provider, or network interaction.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.title.trim().is_empty() {
            return Err(DomainError::EmptyTitle);
        }
        if self
            .short_title
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DomainError::EmptyShortTitle);
        }
        if self
            .task_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DomainError::EmptyTaskName);
        }
        if self.targets.is_empty() {
            return Err(DomainError::MissingTargets);
        }
        if self.targets.iter().copied().collect::<BTreeSet<_>>().len() != self.targets.len() {
            return Err(DomainError::DuplicateTargets);
        }
        if self
            .overrides
            .iter()
            .map(|item| item.platform)
            .collect::<BTreeSet<_>>()
            .len()
            != self.overrides.len()
        {
            return Err(DomainError::DuplicateOverrides);
        }
        if self
            .overrides
            .iter()
            .any(|item| !self.targets.contains(&item.platform))
        {
            return Err(DomainError::OverrideOutsideTargets);
        }
        if let Some(schedule) = &self.scheduled_at {
            schedule.as_naive()?;
        }
        match &self.source {
            MediaSource::LocalFile(path) if path.as_os_str().is_empty() => {
                Err(DomainError::EmptyLocalPath)
            }
            MediaSource::RemoteUrl(url) if !matches!(url.scheme(), "http" | "https") => Err(
                DomainError::UnsupportedRemoteScheme(url.scheme().to_owned()),
            ),
            _ => Ok(()),
        }
    }
}

/// Typed article command retained independently from video publication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishArticleRequest {
    pub platform: String,
    #[serde(default, skip_serializing_if = "AccountSelection::is_empty")]
    pub account: AccountSelection,
    pub title: String,
    pub content: Option<String>,
    pub file: Option<PathBuf>,
    pub cover: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub scheduled_at: Option<LocalSchedule>,
}
impl PublishArticleRequest {
    /// Returns a copy safe to cross a local runner boundary.
    pub fn runner_safe(&self) -> Self {
        let mut safe = self.clone();
        safe.account = AccountSelection::default();
        safe
    }

    /// Returns true if this request still carries account-routing data.
    pub const fn has_account_routing(&self) -> bool {
        !self.account.is_empty()
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.title.trim().is_empty() {
            return Err(DomainError::EmptyTitle);
        }
        if self
            .content
            .as_deref()
            .is_none_or(|item| item.trim().is_empty())
            && self.file.is_none()
        {
            return Err(DomainError::EmptyArticleContent);
        }
        if let Some(schedule) = &self.scheduled_at {
            schedule.as_naive()?;
        }
        self.article_platform()?;
        Ok(())
    }

    /// Returns the only article platform supported by this protocol.
    pub fn article_platform(&self) -> Result<ArticlePlatform, DomainError> {
        match self.platform.trim().to_ascii_lowercase().as_str() {
            "juejin" | "掘金" => Ok(ArticlePlatform::Juejin),
            _ => Err(DomainError::UnknownPlatform(self.platform.clone())),
        }
    }
}

/// Durable state for one publication job.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishState {
    Draft,
    Queued,
    Dispatching,
    Published,
    Failed,
    Unavailable,
}
impl PublishState {
    /// The finite transition graph enforced by the scheduler.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Queued | Self::Unavailable)
                | (
                    Self::Queued,
                    Self::Dispatching | Self::Failed | Self::Unavailable
                )
                | (
                    Self::Dispatching,
                    Self::Published | Self::Failed | Self::Unavailable
                )
        )
    }
    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                from: self,
                to: next,
            })
        }
    }
    fn db(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }
    fn from_db(value: &str) -> Result<Self, DomainError> {
        match value {
            "draft" => Ok(Self::Draft),
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "published" => Ok(Self::Published),
            "failed" => Ok(Self::Failed),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(DomainError::CorruptState(value.to_owned())),
        }
    }
}

/// An immutable publication-history entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub request: PublishRequest,
    pub state: PublishState,
    pub recorded_at: DateTime<Utc>,
    pub detail: Option<String>,
}

/// The publication states accepted by the upstream history query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryStatus {
    Success,
    Failed,
    Publishing,
    Scheduled,
}

impl FromStr for HistoryStatus {
    type Err = HistoryFilterError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "publishing" => Ok(Self::Publishing),
            "scheduled" => Ok(Self::Scheduled),
            _ => Err(HistoryFilterError::InvalidStatus(value.to_owned())),
        }
    }
}

/// A validated, deterministic local publication-history query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryFilter {
    cutoff: Option<DateTime<Utc>>,
    platform: Option<Platform>,
    status: Option<HistoryStatus>,
}

impl HistoryFilter {
    /// Builds a query from the upstream trailing-days form using a caller-supplied clock.
    pub fn from_query(
        days: Option<u16>,
        all: bool,
        platform: Option<Platform>,
        status: Option<HistoryStatus>,
        now: DateTime<Utc>,
    ) -> Result<Self, HistoryFilterError> {
        let cutoff = if all {
            None
        } else {
            let days = days.unwrap_or(7);
            if days == 0 {
                return Err(HistoryFilterError::NonPositiveDays);
            }
            Some(now - ChronoDuration::days(i64::from(days)))
        };
        Ok(Self {
            cutoff,
            platform,
            status,
        })
    }

    /// Retains matching records in their original order.
    pub fn filter(&self, history: Vec<HistoryRecord>) -> Vec<HistoryRecord> {
        history
            .into_iter()
            .filter(|record| self.matches(record))
            .collect()
    }

    /// Tests one record without changing the query or record.
    pub fn matches(&self, record: &HistoryRecord) -> bool {
        self.cutoff
            .is_none_or(|cutoff| record.recorded_at >= cutoff)
            && self
                .platform
                .is_none_or(|platform| record.request.targets.contains(&platform))
            && self
                .status
                .is_none_or(|status| status.matches(record.state))
    }
}

impl HistoryStatus {
    fn matches(self, state: PublishState) -> bool {
        match self {
            Self::Success => state == PublishState::Published,
            Self::Failed => state == PublishState::Failed,
            Self::Publishing => state == PublishState::Dispatching,
            Self::Scheduled => state == PublishState::Queued,
        }
    }
}

/// Errors raised while constructing a history query from user input.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HistoryFilterError {
    #[error("days must be greater than zero unless all is true")]
    NonPositiveDays,
    #[error("status must be success, failed, publishing, or scheduled")]
    InvalidStatus(String),
}

/// A scheduled durable job; `revision` makes transitions deterministic under retries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub request: PublishRequest,
    pub state: PublishState,
    pub due_at: Option<LocalSchedule>,
    pub revision: u64,
    pub updated_at: DateTime<Utc>,
}

/// Durable account/history/job storage boundary.
pub trait Repository: Send + Sync {
    fn save_account(&self, account: &Account) -> Result<(), DomainError>;
    fn accounts(&self) -> Result<Vec<Account>, DomainError>;
    fn save_article_account(&self, account: &ArticleAccount) -> Result<(), DomainError>;
    fn article_accounts(&self) -> Result<Vec<ArticleAccount>, DomainError>;
    fn append_history(&self, record: &HistoryRecord) -> Result<(), DomainError>;
    fn history(&self) -> Result<Vec<HistoryRecord>, DomainError>;
    fn insert_job(&self, job: &ScheduledJob) -> Result<(), DomainError>;
    fn job(&self, id: &str) -> Result<Option<ScheduledJob>, DomainError>;
    fn transition_job(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
    fn set_config(&self, key: &str, value: &str) -> Result<(), DomainError>;
    fn config(&self, key: &str) -> Result<Option<String>, DomainError>;
    fn delete_config(&self, key: &str) -> Result<bool, DomainError>;
}

/// Queue semantics separated from persistence so schedulers are replaceable.
pub trait PublicationQueue: Send + Sync {
    fn enqueue(
        &self,
        request: &PublishRequest,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
    fn advance(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
}

/// SQLite repository with schema migrations and transactional optimistic transitions.
pub struct SqliteRepository {
    connection: Mutex<Connection>,
}
impl SqliteRepository {
    /// Opens (or creates) a database and applies all forward migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DomainError> {
        let connection = Connection::open(path).map_err(DomainError::database)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    /// Opens an in-memory repository for deterministic tests and embedded use.
    pub fn in_memory() -> Result<Self, DomainError> {
        let connection = Connection::open_in_memory().map_err(DomainError::database)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    fn migrate(connection: &Connection) -> Result<(), DomainError> {
        connection.execute_batch("BEGIN; CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY); CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE IF NOT EXISTS article_accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE IF NOT EXISTS history (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, due_at TEXT, revision INTEGER NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS job_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); INSERT OR IGNORE INTO schema_migrations(version) VALUES (2); INSERT OR IGNORE INTO schema_migrations(version) VALUES (3); COMMIT;").map_err(DomainError::database)?;
        let version_four: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=4)",
                [],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !version_four {
            connection.execute_batch("BEGIN; ALTER TABLE accounts ADD COLUMN phone TEXT NOT NULL DEFAULT ''; ALTER TABLE accounts ADD COLUMN partition TEXT NOT NULL DEFAULT ''; ALTER TABLE article_accounts ADD COLUMN phone TEXT NOT NULL DEFAULT ''; ALTER TABLE article_accounts ADD COLUMN partition TEXT NOT NULL DEFAULT ''; INSERT INTO schema_migrations(version) VALUES (4); COMMIT;").map_err(DomainError::database)?;
        }
        Ok(())
    }
    fn locked(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DomainError> {
        self.connection
            .lock()
            .map_err(|_| DomainError::RepositoryPoisoned)
    }
    fn allocate_job_id(&self) -> Result<String, DomainError> {
        let connection = self.locked()?;
        connection
            .execute("INSERT INTO job_sequence DEFAULT VALUES", [])
            .map_err(DomainError::database)?;
        Ok(format!("job-{}", connection.last_insert_rowid()))
    }
}
impl Repository for SqliteRepository {
    fn save_account(&self, account: &Account) -> Result<(), DomainError> {
        let connection = self.locked()?;
        validate_account_route(&account.phone, &account.partition)?;
        connection.execute("INSERT INTO accounts(id, platform, display_name, status, phone, partition) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET platform=excluded.platform, display_name=excluded.display_name, status=excluded.status, phone=excluded.phone, partition=excluded.partition", params![account.id, account.platform.as_str(), account.display_name, account_status_db(account.status), account.phone, account.partition]).map_err(DomainError::database)?;
        Ok(())
    }
    fn accounts(&self) -> Result<Vec<Account>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection
            .prepare("SELECT id, platform, display_name, status, phone, partition FROM accounts ORDER BY id")
            .map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, platform, display_name, status, phone, partition) =
                    row.map_err(DomainError::database)?;
                Ok(Account {
                    id,
                    platform: Platform::from_str(&platform)?,
                    display_name,
                    status: account_status_from_db(&status)?,
                    phone,
                    partition,
                })
            })
            .collect()
    }
    fn save_article_account(&self, account: &ArticleAccount) -> Result<(), DomainError> {
        let connection = self.locked()?;
        validate_account_route(&account.phone, &account.partition)?;
        connection.execute("INSERT INTO article_accounts(id, platform, display_name, status, phone, partition) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET platform=excluded.platform, display_name=excluded.display_name, status=excluded.status, phone=excluded.phone, partition=excluded.partition", params![account.id, article_platform_db(account.platform), account.display_name, article_account_status_db(account.status), account.phone, account.partition]).map_err(DomainError::database)?;
        Ok(())
    }
    fn article_accounts(&self) -> Result<Vec<ArticleAccount>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection
            .prepare("SELECT id, platform, display_name, status, phone, partition FROM article_accounts ORDER BY id")
            .map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, platform, display_name, status, phone, partition) =
                    row.map_err(DomainError::database)?;
                Ok(ArticleAccount {
                    id,
                    platform: article_platform_from_db(&platform)?,
                    display_name,
                    status: article_account_status_from_db(&status)?,
                    phone,
                    partition,
                })
            })
            .collect()
    }
    fn append_history(&self, record: &HistoryRecord) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO history(id, request_json, state, recorded_at, detail) VALUES (?1, ?2, ?3, ?4, ?5)", params![record.id, json(&record.request)?, record.state.db(), record.recorded_at.to_rfc3339(), record.detail]).map_err(DomainError::database)?;
        Ok(())
    }
    fn history(&self) -> Result<Vec<HistoryRecord>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection.prepare("SELECT id, request_json, state, recorded_at, detail FROM history ORDER BY recorded_at, id").map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, request, state, time, detail) = row.map_err(DomainError::database)?;
                Ok(HistoryRecord {
                    id,
                    request: from_json(&request)?,
                    state: PublishState::from_db(&state)?,
                    recorded_at: parse_time(&time)?,
                    detail,
                })
            })
            .collect()
    }
    fn insert_job(&self, job: &ScheduledJob) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO jobs(id, request_json, state, due_at, revision, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![job.id, json(&job.request)?, job.state.db(), job.due_at.as_ref().map(|value| &value.0), job.revision, job.updated_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }
    fn job(&self, id: &str) -> Result<Option<ScheduledJob>, DomainError> {
        let connection = self.locked()?;
        load_job(&connection, id)
    }
    fn transition_job(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current =
            load_job_tx(&transaction, id)?.ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
        current.state.transition(next)?;
        if current.revision != expected_revision {
            return Err(DomainError::StaleJobRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let changed = transaction
            .execute(
                "UPDATE jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND revision=?5",
                params![
                    next.db(),
                    expected_revision + 1,
                    updated_at.to_rfc3339(),
                    id,
                    expected_revision
                ],
            )
            .map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(ScheduledJob {
            state: next,
            revision: expected_revision + 1,
            updated_at,
            ..current
        })
    }
    fn set_config(&self, key: &str, value: &str) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO config(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value]).map_err(DomainError::database)?;
        Ok(())
    }
    fn config(&self, key: &str) -> Result<Option<String>, DomainError> {
        let connection = self.locked()?;
        connection
            .query_row("SELECT value FROM config WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(DomainError::database)
    }
    fn delete_config(&self, key: &str) -> Result<bool, DomainError> {
        let connection = self.locked()?;
        Ok(connection
            .execute("DELETE FROM config WHERE key=?1", [key])
            .map_err(DomainError::database)?
            > 0)
    }
}

impl PublicationQueue for SqliteRepository {
    fn enqueue(
        &self,
        request: &PublishRequest,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        request.validate()?;
        let id = self.allocate_job_id()?;
        let state = if request.draft {
            PublishState::Draft
        } else {
            PublishState::Queued
        };
        let job = ScheduledJob {
            id,
            request: request.runner_safe(),
            state,
            due_at: request.scheduled_at.clone(),
            revision: 0,
            updated_at: now,
        };
        self.insert_job(&job)?;
        Ok(job)
    }
    fn advance(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        self.transition_job(id, expected_revision, next, now)
    }
}

/// Metadata policy used before an adapter stages a remote media object.
pub trait RemoteMediaPolicy: Send + Sync {
    fn max_bytes(&self) -> u64;
    fn allows_content_type(&self, content_type: Option<&str>) -> bool;
}
/// A bounded remote-media staging request. It does not perform a fetch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMediaRequest {
    pub url: Url,
    pub max_bytes: u64,
}
impl RemoteMediaRequest {
    pub fn new(url: Url, policy: &dyn RemoteMediaPolicy) -> Result<Self, DomainError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(DomainError::UnsupportedRemoteScheme(
                url.scheme().to_owned(),
            ));
        }
        Ok(Self {
            url,
            max_bytes: policy.max_bytes(),
        })
    }
}
/// A staged file is owned by its creator and cleaned up by calling `cleanup`.
pub trait StagedMedia: Send {
    fn path(&self) -> &Path;
    fn cleanup(self: Box<Self>) -> Result<(), DomainError>;
}
/// Boundary for bounded HTTP staging adapters.
pub trait RemoteMediaStager: Send + Sync {
    fn stage(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
    ) -> Result<Box<dyn StagedMedia>, DomainError>;
}

/// Concrete bounded policy used by the daemon and desktop adapters.
#[derive(Clone, Debug)]
pub struct MediaStagingPolicy {
    pub max_bytes: u64,
    pub allowed_content_types: Vec<String>,
}
impl RemoteMediaPolicy for MediaStagingPolicy {
    fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
    fn allows_content_type(&self, value: Option<&str>) -> bool {
        value.is_some_and(|item| {
            self.allowed_content_types
                .iter()
                .any(|allowed| item.starts_with(allowed))
        })
    }
}
/// HTTP-only staging implementation. It is deliberately not connected to providers.
pub struct HttpRemoteMediaStager {
    directory: PathBuf,
}

struct RemoteMediaResponse {
    content_type: Option<String>,
    content_length: Option<String>,
    body: Box<dyn Read>,
}

trait RemoteMediaTransport {
    fn get(&self, url: &Url) -> Result<RemoteMediaResponse, DomainError>;
}

struct UreqRemoteMediaTransport;

impl RemoteMediaTransport for UreqRemoteMediaTransport {
    fn get(&self, url: &Url) -> Result<RemoteMediaResponse, DomainError> {
        let response = ureq::get(url.as_str())
            .call()
            .map_err(|error| DomainError::RemoteMedia(error.to_string()))?;
        let content_type = response.header("content-type").map(str::to_owned);
        let content_length = response.header("content-length").map(str::to_owned);
        Ok(RemoteMediaResponse {
            content_type,
            content_length,
            body: Box::new(response.into_reader()),
        })
    }
}

trait StagingFilesystem {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Write>>;
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

struct OsStagingFilesystem;

impl StagingFilesystem for OsStagingFilesystem {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Write>> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|file| Box::new(file) as Box<dyn Write>)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

trait StagingNameSource {
    fn next_name(&mut self) -> String;
}

struct RandomStagingNameSource;

impl StagingNameSource for RandomStagingNameSource {
    fn next_name(&mut self) -> String {
        format!("matrixpost-stage-{:032x}", rand::random::<u128>())
    }
}
pub struct OwnedStagedMedia {
    path: PathBuf,
}
impl StagedMedia for OwnedStagedMedia {
    fn path(&self) -> &Path {
        &self.path
    }
    fn cleanup(self: Box<Self>) -> Result<(), DomainError> {
        fs::remove_file(&self.path).map_err(DomainError::io)
    }
}
impl HttpRemoteMediaStager {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn stage_with(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
        transport: &dyn RemoteMediaTransport,
        filesystem: &dyn StagingFilesystem,
        names: &mut dyn StagingNameSource,
    ) -> Result<Box<dyn StagedMedia>, DomainError> {
        let response = transport.get(&request.url)?;
        if !policy.allows_content_type(response.content_type.as_deref()) {
            return Err(DomainError::DisallowedContentType(
                response.content_type.unwrap_or_else(|| "missing".into()),
            ));
        }
        if let Some(length) = response.content_length.as_deref() {
            let parsed = length
                .parse::<u64>()
                .map_err(|_| DomainError::RemoteMedia("invalid content-length".into()))?;
            if parsed > request.max_bytes {
                return Err(DomainError::RemoteMediaTooLarge {
                    limit: request.max_bytes,
                    actual: parsed,
                });
            }
        }
        filesystem
            .create_dir_all(&self.directory)
            .map_err(DomainError::io)?;
        let (path, mut output) = (0..16)
            .find_map(|_| {
                let path = self.directory.join(names.next_name());
                match filesystem.create_new(&path) {
                    Ok(file) => Some(Ok((path, file))),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(DomainError::io(error))),
                }
            })
            .transpose()?
            .ok_or_else(|| {
                DomainError::RemoteMedia("could not allocate unique staging file".into())
            })?;
        let mut reader = response.body.take(request.max_bytes.saturating_add(1));
        let copied = match std::io::copy(&mut reader, &mut output).and_then(|value| {
            output.flush()?;
            Ok(value)
        }) {
            Ok(value) => value,
            Err(error) => {
                let _ = filesystem.remove_file(&path);
                return Err(DomainError::io(error));
            }
        };
        if copied > request.max_bytes {
            let _ = filesystem.remove_file(&path);
            return Err(DomainError::RemoteMediaTooLarge {
                limit: request.max_bytes,
                actual: copied,
            });
        }
        Ok(Box::new(OwnedStagedMedia { path }))
    }
}
impl RemoteMediaStager for HttpRemoteMediaStager {
    fn stage(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
    ) -> Result<Box<dyn StagedMedia>, DomainError> {
        self.stage_with(
            request,
            policy,
            &UreqRemoteMediaTransport,
            &OsStagingFilesystem,
            &mut RandomStagingNameSource,
        )
    }
}

/// Provider availability is explicit so no adapter can imply browser automation exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProviderAvailability {
    Available,
    Unavailable { reason: String },
}
/// Provider dispatch result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DispatchOutcome {
    Queued { job_id: String },
    Unavailable { reason: String },
    Rejected { reason: String },
}

/// Version of the credential-free, local runner HTTP protocol.
pub const PROVIDER_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Version of the credential-free local article runner HTTP protocol.
pub const ARTICLE_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Request sent to a local article runner.
///
/// The request contains only the validated article command. Browser profile,
/// session, and credential configuration remain outside this protocol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleRunnerRequest {
    pub version: u16,
    pub request: PublishArticleRequest,
}

/// Explicit result returned by a local article runner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArticleRunnerResponse {
    Queued {
        version: u16,
        platform: ArticlePlatform,
        job_id: String,
        automation_attempted: bool,
    },
    Unavailable {
        version: u16,
        platform: ArticlePlatform,
        reason: String,
        automation_attempted: bool,
    },
    Rejected {
        version: u16,
        platform: ArticlePlatform,
        reason: String,
        automation_attempted: bool,
    },
}

/// A validated article runner response, suitable for a future embedding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ArticleDispatchOutcome {
    Queued {
        job_id: String,
    },
    Unavailable {
        reason: String,
    },
    Rejected {
        reason: String,
        automation_attempted: bool,
    },
}

impl ArticleRunnerResponse {
    /// Converts only a response matching the version, platform, and outcome
    /// invariants of this protocol.
    pub fn into_dispatch(
        self,
        expected_platform: ArticlePlatform,
    ) -> Option<ArticleDispatchOutcome> {
        match self {
            Self::Queued {
                version,
                platform,
                job_id,
                automation_attempted: true,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION
                && platform == expected_platform
                && !job_id.trim().is_empty() =>
            {
                Some(ArticleDispatchOutcome::Queued { job_id })
            }
            Self::Unavailable {
                version,
                platform,
                reason,
                automation_attempted: false,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ArticleDispatchOutcome::Unavailable { reason })
            }
            Self::Rejected {
                version,
                platform,
                reason,
                automation_attempted,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ArticleDispatchOutcome::Rejected {
                    reason,
                    automation_attempted,
                })
            }
            _ => None,
        }
    }
}

/// Credential-free, loopback-only endpoint for the Juejin article runner.
///
/// This is deliberately separate from video provider runners: article
/// publication has a distinct protocol and is not scheduled by MatriXpost.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleRunner {
    pub address: SocketAddr,
}

/// Invalid article-runner configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ArticleRunnerConfigError {
    #[error("article runner TCP address must bind to loopback")]
    TcpMustBeLoopback,
    #[error("article runner endpoint must not contain credential-like data")]
    CredentialLikeEndpoint,
    #[error("article runner argument must use tcp:127.0.0.1:PORT")]
    InvalidArgument,
}

/// Injectable HTTP boundary for deterministic article-runner adapter tests.
pub trait ArticleRunnerHttpTransport {
    /// POSTs a JSON document and returns the HTTP status and response body.
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ArticleRunnerTransportError>;
}

/// Non-sensitive failure classification for the local article-runner transport.
///
/// This intentionally carries no endpoint, body, response, or credential data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArticleRunnerTransportError {
    /// The local HTTP request could not complete.
    RequestFailed,
    /// The local runner response could not be read.
    ResponseReadFailed,
}

struct UreqArticleRunnerHttpTransport;

impl ArticleRunnerHttpTransport for UreqArticleRunnerHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ArticleRunnerTransportError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| ArticleRunnerTransportError::RequestFailed)?;
        let status = response.status();
        let body = response
            .into_string()
            .map_err(|_| ArticleRunnerTransportError::ResponseReadFailed)?;
        Ok((status, body))
    }
}

impl ArticleRunner {
    /// Parses the explicit local form `tcp:127.0.0.1:PORT`.
    pub fn parse_cli(value: &str) -> Result<Self, ArticleRunnerConfigError> {
        reject_credential_like_endpoint(value)
            .map_err(|_| ArticleRunnerConfigError::CredentialLikeEndpoint)?;
        let address = value
            .strip_prefix("tcp:")
            .ok_or(ArticleRunnerConfigError::InvalidArgument)?
            .parse()
            .map_err(|_| ArticleRunnerConfigError::InvalidArgument)?;
        let runner = Self { address };
        runner.validate()?;
        Ok(runner)
    }

    /// Ensures the endpoint cannot target a remote host.
    pub fn validate(&self) -> Result<(), ArticleRunnerConfigError> {
        if self.address.ip().is_loopback() {
            Ok(())
        } else {
            Err(ArticleRunnerConfigError::TcpMustBeLoopback)
        }
    }

    fn rejected(automation_attempted: bool) -> ArticleDispatchOutcome {
        ArticleDispatchOutcome::Rejected {
            reason: "local article runner did not return a valid accepted response".into(),
            automation_attempted,
        }
    }

    /// Dispatches an unscheduled article through the versioned local protocol.
    ///
    /// A queued response only proves local runner completion; it never confirms
    /// that Juejin processed or published the article.
    pub fn dispatch(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<ArticleDispatchOutcome, DomainError> {
        self.dispatch_with(request, &UreqArticleRunnerHttpTransport)
    }

    /// Same as [`Self::dispatch`] with an injected HTTP transport.
    pub fn dispatch_with<T: ArticleRunnerHttpTransport>(
        &self,
        request: &PublishArticleRequest,
        transport: &T,
    ) -> Result<ArticleDispatchOutcome, DomainError> {
        request.validate()?;
        if request.scheduled_at.is_some() {
            return Ok(ArticleDispatchOutcome::Rejected {
                reason: "scheduled article dispatch is not supported".into(),
                automation_attempted: false,
            });
        }
        let expected_platform = request.article_platform()?;
        let endpoint = format!("http://{}/v1/publish-article", self.address);
        let payload = ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: request.runner_safe(),
        };
        let payload = serde_json::to_string(&payload).map_err(DomainError::serialization)?;
        let (status, body) = match transport.post_json(&endpoint, &payload) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected(true)),
        };
        if status != 200 {
            return Ok(Self::rejected(true));
        }
        let response: ArticleRunnerResponse = match serde_json::from_str(&body) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected(true)),
        };
        Ok(response
            .into_dispatch(expected_platform)
            .unwrap_or_else(|| Self::rejected(true)))
    }
}

/// Request sent from an embedding to its configured local runner.
///
/// The endpoint is deliberately separate from WebDriver. It carries only a
/// validated publication request and never browser/session configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRunnerRequest {
    pub version: u16,
    pub platform: Platform,
    pub request: PublishRequest,
}

/// Response accepted from a local runner.
///
/// `Queued` means the runner completed its configured WebDriver phases; it is
/// not a claim that the remote platform has finished processing the media.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderRunnerResponse {
    Queued {
        version: u16,
        platform: Platform,
        job_id: String,
    },
    Unavailable {
        version: u16,
        platform: Platform,
        reason: String,
    },
    Rejected {
        version: u16,
        platform: Platform,
        reason: String,
    },
}

impl ProviderRunnerResponse {
    fn into_dispatch(self, expected_platform: Platform) -> Option<DispatchOutcome> {
        match self {
            Self::Queued {
                version,
                platform,
                job_id,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION
                && platform == expected_platform
                && !job_id.trim().is_empty() =>
            {
                Some(DispatchOutcome::Queued { job_id })
            }
            Self::Unavailable {
                version,
                platform,
                reason,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(DispatchOutcome::Unavailable { reason })
            }
            Self::Rejected {
                version,
                platform,
                reason,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(DispatchOutcome::Rejected { reason })
            }
            _ => None,
        }
    }
}
/// Boundary implemented by opt-in platform adapters.
pub trait PublishProvider: Send + Sync {
    fn platform(&self) -> Platform;
    fn availability(&self) -> ProviderAvailability;
    fn enqueue(&self, request: &PublishRequest) -> Result<DispatchOutcome, DomainError>;
}

/// Provider which invokes the versioned protocol on one loopback TCP runner.
///
/// It never contacts a platform or WebDriver directly. Transport and response
/// failures become explicit rejected outcomes so callers cannot mistake a
/// malformed local response for publication acceptance.
struct TcpRunnerProvider {
    platform: Platform,
    address: SocketAddr,
}

trait RunnerHttpTransport {
    fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()>;
}

struct UreqRunnerHttpTransport;

impl RunnerHttpTransport for UreqRunnerHttpTransport {
    fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| ())?;
        let status = response.status();
        let body = response.into_string().map_err(|_| ())?;
        Ok((status, body))
    }
}

impl TcpRunnerProvider {
    fn rejected() -> DispatchOutcome {
        DispatchOutcome::Rejected {
            reason: "local provider runner did not return a valid accepted response".into(),
        }
    }

    fn enqueue_with<T: RunnerHttpTransport>(
        &self,
        request: &PublishRequest,
        transport: &T,
    ) -> Result<DispatchOutcome, DomainError> {
        let endpoint = format!("http://{}/v1/publish", self.address);
        let payload = ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
            request: request.runner_safe(),
        };
        let payload = serde_json::to_string(&payload).map_err(DomainError::serialization)?;
        let (status, body) = match transport.post_json(&endpoint, &payload) {
            Ok(response) => response,
            Err(()) => return Ok(Self::rejected()),
        };
        if status != 200 {
            return Ok(Self::rejected());
        }
        let response: ProviderRunnerResponse = match serde_json::from_str(&body) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected()),
        };
        Ok(response
            .into_dispatch(self.platform)
            .unwrap_or_else(Self::rejected))
    }
}

impl PublishProvider for TcpRunnerProvider {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn enqueue(&self, request: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        self.enqueue_with(request, &UreqRunnerHttpTransport)
    }
}

/// A credential-free description of a local runner owned by an embedding.
///
/// MatriXpost never launches a runner. A loopback-TCP declaration installs the
/// stable v1 HTTP adapter and opens the endpoint only when dispatching a valid
/// request; other declared transports remain visibility-only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRunner {
    pub platform: Platform,
    #[serde(flatten)]
    pub transport: ProviderRunnerTransport,
}

/// Local-only transports understood by a [`ProviderRunner`] declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum ProviderRunnerTransport {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
    Tcp { address: SocketAddr },
}

/// Invalid provider-runner configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderRunnerConfigError {
    #[error("provider runner platform is configured more than once: {platform:?}")]
    DuplicatePlatform { platform: Platform },
    #[error("provider runner unix socket path must be absolute")]
    UnixSocketPathMustBeAbsolute,
    #[error("provider runner named pipe must use the \\\\.\\pipe\\ namespace")]
    NamedPipeMustBeLocal,
    #[error("provider runner TCP address must bind to loopback")]
    TcpMustBeLoopback,
    #[error("provider runner endpoint must not contain credential-like data")]
    CredentialLikeEndpoint,
    #[error("provider runner argument must use PLATFORM=TRANSPORT:ENDPOINT")]
    InvalidArgument,
}

impl ProviderRunner {
    /// Validates that this is a local, credential-free runner declaration.
    pub fn validate(&self) -> Result<(), ProviderRunnerConfigError> {
        match &self.transport {
            ProviderRunnerTransport::UnixSocket { path } => {
                if !path.is_absolute() {
                    return Err(ProviderRunnerConfigError::UnixSocketPathMustBeAbsolute);
                }
                reject_credential_like_endpoint(&path.to_string_lossy())?;
            }
            ProviderRunnerTransport::NamedPipe { name } => {
                if !name.starts_with(r"\\.\pipe\") {
                    return Err(ProviderRunnerConfigError::NamedPipeMustBeLocal);
                }
                reject_credential_like_endpoint(name)?;
            }
            ProviderRunnerTransport::Tcp { address } => {
                if !address.ip().is_loopback() {
                    return Err(ProviderRunnerConfigError::TcpMustBeLoopback);
                }
            }
        }
        Ok(())
    }

    /// Parses the CLI form `PLATFORM=unix:/absolute/path`,
    /// `PLATFORM=pipe:\\\\.\\pipe\\name`, or `PLATFORM=tcp:127.0.0.1:PORT`.
    pub fn parse_cli(value: &str) -> Result<Self, ProviderRunnerConfigError> {
        let (platform, transport) = value
            .split_once('=')
            .ok_or(ProviderRunnerConfigError::InvalidArgument)?;
        let platform =
            Platform::from_str(platform).map_err(|_| ProviderRunnerConfigError::InvalidArgument)?;
        let transport = if let Some(path) = transport.strip_prefix("unix:") {
            ProviderRunnerTransport::UnixSocket {
                path: PathBuf::from(path),
            }
        } else if let Some(name) = transport.strip_prefix("pipe:") {
            ProviderRunnerTransport::NamedPipe {
                name: name.to_owned(),
            }
        } else if let Some(address) = transport.strip_prefix("tcp:") {
            ProviderRunnerTransport::Tcp {
                address: address
                    .parse()
                    .map_err(|_| ProviderRunnerConfigError::InvalidArgument)?,
            }
        } else {
            return Err(ProviderRunnerConfigError::InvalidArgument);
        };
        let runner = Self {
            platform,
            transport,
        };
        runner.validate()?;
        Ok(runner)
    }

    fn unavailable_reason(&self) -> String {
        let transport = match &self.transport {
            ProviderRunnerTransport::UnixSocket { .. } => "Unix socket",
            ProviderRunnerTransport::NamedPipe { .. } => "named pipe",
            ProviderRunnerTransport::Tcp { .. } => "loopback TCP",
        };
        format!(
            "{transport} runner configured for {}; no execution adapter is installed",
            self.platform.as_str()
        )
    }
}

fn reject_credential_like_endpoint(value: &str) -> Result<(), ProviderRunnerConfigError> {
    let lower = value.to_ascii_lowercase();
    if [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
        "@",
        "?",
        "#",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Err(ProviderRunnerConfigError::CredentialLikeEndpoint);
    }
    Ok(())
}

/// Deterministic failure returned when a platform is registered more than once.
///
/// A registry never replaces an existing provider implicitly: replacing an
/// adapter can change the side-effecting backend used for a publication.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProviderRegistrationError {
    #[error("provider already registered for platform: {platform:?}")]
    Duplicate { platform: Platform },
}

/// Per-platform results from one multi-target provider dispatch.
///
/// The map is ordered by [`Platform`], rather than registration or request
/// order, so callers get stable output across runs.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderDispatchReport {
    pub outcomes: BTreeMap<Platform, DispatchOutcome>,
}

/// Explicit registry for installed publication providers.
///
/// The registry contains no browser sessions or credentials. It only owns
/// provider implementations supplied by the embedding application. An absent
/// platform is a normal, explicit unavailable result, never an implied
/// automation capability.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<Platform, Box<dyn PublishProvider>>,
    runners: BTreeMap<Platform, ProviderRunner>,
}

impl ProviderRegistry {
    /// Creates an empty provider registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a registry with validated local runner declarations.
    ///
    /// Loopback-TCP declarations install the stable local runner adapter.
    /// Unix sockets and Windows named pipes remain declared-but-unavailable
    /// until those transports receive an audited implementation.
    pub fn from_runners(
        runners: impl IntoIterator<Item = ProviderRunner>,
    ) -> Result<Self, ProviderRunnerConfigError> {
        let mut registry = Self::new();
        for runner in runners {
            runner.validate()?;
            let platform = runner.platform;
            if registry.runners.insert(platform, runner).is_some() {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
        }
        for (platform, address) in
            registry
                .runners
                .values()
                .filter_map(|runner| match runner.transport {
                    ProviderRunnerTransport::Tcp { address } => Some((runner.platform, address)),
                    _ => None,
                })
        {
            if registry.providers.contains_key(&platform) {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
            registry
                .providers
                .insert(platform, Box::new(TcpRunnerProvider { platform, address }));
        }
        Ok(registry)
    }

    /// Registers a provider without allowing an implicit replacement.
    pub fn register(
        &mut self,
        provider: Box<dyn PublishProvider>,
    ) -> Result<(), ProviderRegistrationError> {
        let platform = provider.platform();
        if self.providers.contains_key(&platform) {
            return Err(ProviderRegistrationError::Duplicate { platform });
        }
        self.providers.insert(platform, provider);
        Ok(())
    }

    /// Returns an installed provider's declared availability.
    pub fn availability(&self, platform: Platform) -> ProviderAvailability {
        self.providers
            .get(&platform)
            .map(|provider| provider.availability())
            .unwrap_or_else(|| ProviderAvailability::Unavailable {
                reason: self.unregistered_reason(platform),
            })
    }

    /// Returns every known platform's availability in canonical platform order.
    pub fn availability_report(&self) -> BTreeMap<Platform, ProviderAvailability> {
        Platform::ALL
            .iter()
            .copied()
            .map(|platform| (platform, self.availability(platform)))
            .collect()
    }

    /// Dispatches one target after proving that target belongs to the request.
    ///
    /// Provider errors are retained as errors. Missing providers and providers
    /// that declare themselves unavailable are ordinary dispatch outcomes, so
    /// callers can safely aggregate partial multi-target results.
    pub fn dispatch(
        &self,
        platform: Platform,
        request: &PublishRequest,
    ) -> Result<DispatchOutcome, DomainError> {
        request.validate()?;
        if !request.targets.contains(&platform) {
            return Err(DomainError::ProviderPlatformNotTarget { platform });
        }

        let Some(provider) = self.providers.get(&platform) else {
            return Ok(DispatchOutcome::Unavailable {
                reason: self.unregistered_reason(platform),
            });
        };

        match provider.availability() {
            ProviderAvailability::Available => provider.enqueue(request),
            ProviderAvailability::Unavailable { reason } => {
                Ok(DispatchOutcome::Unavailable { reason })
            }
        }
    }

    /// Dispatches every requested target and preserves one outcome per platform.
    ///
    /// A malformed request is rejected before any provider is touched. Once the
    /// request is valid, an individual provider failure becomes that target's
    /// rejected outcome and cannot prevent the remaining targets from running.
    pub fn dispatch_all(
        &self,
        request: &PublishRequest,
    ) -> Result<ProviderDispatchReport, DomainError> {
        request.validate()?;
        let mut outcomes = BTreeMap::new();
        for platform in request.targets.iter().copied() {
            let outcome = match self.dispatch(platform, request) {
                Ok(outcome) => outcome,
                Err(error) => DispatchOutcome::Rejected {
                    reason: error.to_string(),
                },
            };
            outcomes.insert(platform, outcome);
        }
        Ok(ProviderDispatchReport { outcomes })
    }

    fn unregistered_reason(&self, platform: Platform) -> String {
        self.runners
            .get(&platform)
            .map(ProviderRunner::unavailable_reason)
            .unwrap_or_else(|| format!("no provider registered for {}", platform.as_str()))
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, DomainError> {
    serde_json::to_string(value).map_err(DomainError::serialization)
}
fn from_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, DomainError> {
    serde_json::from_str(value).map_err(DomainError::serialization)
}
fn parse_time(value: &str) -> Result<DateTime<Utc>, DomainError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| DomainError::CorruptState(value.to_owned()))
}
fn account_status_db(value: AccountStatus) -> &'static str {
    match value {
        AccountStatus::LoggedIn => "logged_in",
        AccountStatus::Expired => "expired",
        AccountStatus::LoggedOut => "logged_out",
        AccountStatus::Unavailable => "unavailable",
    }
}
fn account_status_from_db(value: &str) -> Result<AccountStatus, DomainError> {
    match value {
        "logged_in" => Ok(AccountStatus::LoggedIn),
        "expired" => Ok(AccountStatus::Expired),
        "logged_out" => Ok(AccountStatus::LoggedOut),
        "unavailable" => Ok(AccountStatus::Unavailable),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn article_platform_db(value: ArticlePlatform) -> &'static str {
    match value {
        ArticlePlatform::Juejin => "juejin",
    }
}
fn article_platform_from_db(value: &str) -> Result<ArticlePlatform, DomainError> {
    match value {
        "juejin" => Ok(ArticlePlatform::Juejin),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn article_account_status_db(value: ArticleAccountStatus) -> &'static str {
    match value {
        ArticleAccountStatus::LoggedIn => "logged_in",
        ArticleAccountStatus::Expired => "expired",
        ArticleAccountStatus::LoggedOut => "logged_out",
        ArticleAccountStatus::Unavailable => "unavailable",
    }
}
fn article_account_status_from_db(value: &str) -> Result<ArticleAccountStatus, DomainError> {
    match value {
        "logged_in" => Ok(ArticleAccountStatus::LoggedIn),
        "expired" => Ok(ArticleAccountStatus::Expired),
        "logged_out" => Ok(ArticleAccountStatus::LoggedOut),
        "unavailable" => Ok(ArticleAccountStatus::Unavailable),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn validate_account_route(phone: &str, partition: &str) -> Result<(), DomainError> {
    if phone.trim().is_empty() || partition.trim().is_empty() || !partition.starts_with("persist:")
    {
        return Err(DomainError::InvalidAccountRoute);
    }
    Ok(())
}
fn load_job(connection: &Connection, id: &str) -> Result<Option<ScheduledJob>, DomainError> {
    connection
        .query_row(
            "SELECT id, request_json, state, due_at, revision, updated_at FROM jobs WHERE id=?1",
            [id],
            row_to_job,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}
fn load_job_tx(
    transaction: &Transaction<'_>,
    id: &str,
) -> Result<Option<ScheduledJob>, DomainError> {
    transaction
        .query_row(
            "SELECT id, request_json, state, due_at, revision, updated_at FROM jobs WHERE id=?1",
            [id],
            row_to_job,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}
fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ScheduledJob, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let request = row.get::<_, String>(1)?;
    let state = row.get::<_, String>(2)?;
    let due_at = row.get::<_, Option<String>>(3)?;
    let revision = row.get::<_, u64>(4)?;
    let updated = row.get::<_, String>(5)?;
    Ok((|| {
        Ok(ScheduledJob {
            id,
            request: from_json(&request)?,
            state: PublishState::from_db(&state)?,
            due_at: due_at.as_deref().map(LocalSchedule::parse).transpose()?,
            revision,
            updated_at: parse_time(&updated)?,
        })
    })())
}

/// Typed failures returned at domain and persistence boundaries.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    #[error("unknown platform: {0}")]
    UnknownPlatform(String),
    #[error("publish title must not be empty")]
    EmptyTitle,
    #[error("account phone must be non-empty and partition must start with persist:")]
    InvalidAccountRoute,
    #[error("short title must not be empty")]
    EmptyShortTitle,
    #[error("task name must not be empty")]
    EmptyTaskName,
    #[error("article content or file is required")]
    EmptyArticleContent,
    #[error("at least one platform target is required")]
    MissingTargets,
    #[error("platform targets must be unique")]
    DuplicateTargets,
    #[error("platform overrides must be unique")]
    DuplicateOverrides,
    #[error("platform override is not among targets")]
    OverrideOutsideTargets,
    #[error("provider platform is not among request targets: {platform:?}")]
    ProviderPlatformNotTarget { platform: Platform },
    #[error("local file path must not be empty")]
    EmptyLocalPath,
    #[error("remote source scheme is not supported: {0}")]
    UnsupportedRemoteScheme(String),
    #[error("scheduled time must use YYYY-MM-DD HH:mm:ss: {0}")]
    InvalidSchedule(String),
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: PublishState,
        to: PublishState,
    },
    #[error("unknown scheduled job: {0}")]
    UnknownJob(String),
    #[error("stale job revision for {id}: expected {expected}, actual {actual}")]
    StaleJobRevision {
        id: String,
        expected: u64,
        actual: u64,
    },
    #[error("concurrent job update: {0}")]
    ConcurrentJobUpdate(String),
    #[error("corrupt durable state: {0}")]
    CorruptState(String),
    #[error("repository mutex was poisoned")]
    RepositoryPoisoned,
    #[error("database error: {0}")]
    Database(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("remote media error: {0}")]
    RemoteMedia(String),
    #[error("remote media content type is not allowed: {0}")]
    DisallowedContentType(String),
    #[error("remote media is too large: {actual} bytes exceeds {limit}")]
    RemoteMediaTooLarge { limit: u64, actual: u64 },
}
impl DomainError {
    fn database(error: rusqlite::Error) -> Self {
        Self::Database(error.to_string())
    }
    fn serialization(error: serde_json::Error) -> Self {
        Self::Serialization(error.to_string())
    }
    fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{self, Cursor, Read},
        sync::{
            Arc,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
    };

    static STAGING_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestTransport(Mutex<Option<RemoteMediaResponse>>);

    impl TestTransport {
        fn response(
            content_type: Option<&str>,
            content_length: Option<&str>,
            body: impl Read + 'static,
        ) -> Self {
            Self(Mutex::new(Some(RemoteMediaResponse {
                content_type: content_type.map(str::to_owned),
                content_length: content_length.map(str::to_owned),
                body: Box::new(body),
            })))
        }
    }

    impl RemoteMediaTransport for TestTransport {
        fn get(&self, _: &Url) -> Result<RemoteMediaResponse, DomainError> {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| DomainError::RemoteMedia("test response reused".into()))
        }
    }

    #[derive(Clone, Copy)]
    enum TestOutput {
        File,
        FailWrite,
        FailFlush,
    }

    struct TestFilesystem {
        output: TestOutput,
        created: AtomicU64,
    }

    impl TestFilesystem {
        fn file() -> Self {
            Self {
                output: TestOutput::File,
                created: AtomicU64::new(0),
            }
        }

        fn failing(output: TestOutput) -> Self {
            Self {
                output,
                created: AtomicU64::new(0),
            }
        }

        fn created_count(&self) -> u64 {
            self.created.load(Ordering::Relaxed)
        }
    }

    impl StagingFilesystem for TestFilesystem {
        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            fs::create_dir_all(path)
        }

        fn create_new(&self, path: &Path) -> io::Result<Box<dyn Write>> {
            let file = OpenOptions::new().write(true).create_new(true).open(path)?;
            self.created.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(TestWriter {
                file,
                failure: self.output,
            }))
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }
    }

    struct TestWriter {
        file: fs::File,
        failure: TestOutput,
    }

    impl Write for TestWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if matches!(self.failure, TestOutput::FailWrite) {
                return Err(io::Error::other("injected write failure"));
            }
            self.file.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            if matches!(self.failure, TestOutput::FailFlush) {
                return Err(io::Error::other("injected flush failure"));
            }
            self.file.flush()
        }
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected read failure"))
        }
    }

    struct TestNames(VecDeque<String>);

    impl TestNames {
        fn one(name: &str) -> Self {
            Self(VecDeque::from([name.to_owned()]))
        }
    }

    impl StagingNameSource for TestNames {
        fn next_name(&mut self) -> String {
            self.0.pop_front().expect("test supplied enough names")
        }
    }

    fn staging_directory(label: &str) -> PathBuf {
        let sequence = STAGING_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "matrixpost-core-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn staging_policy(max_bytes: u64) -> MediaStagingPolicy {
        MediaStagingPolicy {
            max_bytes,
            allowed_content_types: vec!["video/".into()],
        }
    }

    fn staging_request(policy: &MediaStagingPolicy) -> RemoteMediaRequest {
        RemoteMediaRequest::new(
            Url::parse("https://example.invalid/movie.mp4").unwrap(),
            policy,
        )
        .unwrap()
    }

    fn assert_empty_directory(path: &Path) {
        assert!(path.exists());
        assert_eq!(fs::read_dir(path).unwrap().count(), 0);
    }

    fn assert_staging_error_leaves_no_file(
        label: &str,
        policy: MediaStagingPolicy,
        transport: TestTransport,
        filesystem: &TestFilesystem,
    ) {
        let directory = staging_directory(label);
        fs::create_dir_all(&directory).unwrap();
        let stager = HttpRemoteMediaStager::new(directory.clone());
        let mut names = TestNames::one("output");
        assert!(
            stager
                .stage_with(
                    &staging_request(&policy),
                    &policy,
                    &transport,
                    filesystem,
                    &mut names,
                )
                .is_err()
        );
        assert_empty_directory(&directory);
        fs::remove_dir_all(directory).unwrap();
    }
    fn request() -> PublishRequest {
        PublishRequest {
            source: MediaSource::LocalFile("video.mp4".into()),
            title: "title".into(),
            short_title: Some("short".into()),
            tags: vec!["tag".into()],
            address: Some("address".into()),
            draft: false,
            bt2: None,
            scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
            task_name: Some("task".into()),
            account: AccountSelection {
                phone: Some("masked".into()),
                partition: Some("main".into()),
            },
            wechat_link: WechatLink {
                product_id: Some("product".into()),
                ..Default::default()
            },
            overrides: vec![PlatformOverride {
                platform: Platform::Douyin,
                title: None,
                short_title: None,
                tags: None,
                creative_statement: Some("original".into()),
                account: None,
                wechat_link: None,
            }],
            targets: vec![Platform::Douyin],
        }
    }

    struct TestProvider {
        platform: Platform,
        availability: ProviderAvailability,
        outcome: DispatchOutcome,
        error: Option<String>,
        calls: Arc<AtomicUsize>,
    }

    struct CapturingRunnerTransport(Mutex<Option<(String, String)>>);

    impl RunnerHttpTransport for CapturingRunnerTransport {
        fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()> {
            *self.0.lock().unwrap() = Some((endpoint.into(), body.into()));
            Ok((
                200,
                r#"{"outcome":"queued","version":1,"platform":"dy","job_id":"safe-job"}"#.into(),
            ))
        }
    }

    struct CapturingArticleRunnerTransport {
        captured: Mutex<Option<(String, String)>>,
        response: (u16, String),
    }

    struct FailingArticleRunnerTransport;

    impl ArticleRunnerHttpTransport for FailingArticleRunnerTransport {
        fn post_json(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(u16, String), ArticleRunnerTransportError> {
            Err(ArticleRunnerTransportError::RequestFailed)
        }
    }

    impl ArticleRunnerHttpTransport for CapturingArticleRunnerTransport {
        fn post_json(
            &self,
            endpoint: &str,
            body: &str,
        ) -> Result<(u16, String), ArticleRunnerTransportError> {
            *self.captured.lock().unwrap() = Some((endpoint.into(), body.into()));
            Ok(self.response.clone())
        }
    }

    impl PublishProvider for TestProvider {
        fn platform(&self) -> Platform {
            self.platform
        }

        fn availability(&self) -> ProviderAvailability {
            self.availability.clone()
        }

        fn enqueue(&self, _: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.error
                .as_ref()
                .map(|reason| Err(DomainError::RemoteMedia(reason.clone())))
                .unwrap_or_else(|| Ok(self.outcome.clone()))
        }
    }

    fn test_provider(
        platform: Platform,
        availability: ProviderAvailability,
        outcome: DispatchOutcome,
        error: Option<&str>,
    ) -> (Box<dyn PublishProvider>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Box::new(TestProvider {
                platform,
                availability,
                outcome,
                error: error.map(str::to_owned),
                calls: Arc::clone(&calls),
            }),
            calls,
        )
    }

    #[test]
    fn provider_registry_rejects_duplicate_platforms_deterministically() {
        let mut registry = ProviderRegistry::new();
        let (first, _) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "first".into(),
            },
            None,
        );
        let (second, _) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "second".into(),
            },
            None,
        );
        registry.register(first).unwrap();
        assert_eq!(
            registry.register(second),
            Err(ProviderRegistrationError::Duplicate {
                platform: Platform::Douyin,
            })
        );
        assert_eq!(
            registry.dispatch(Platform::Douyin, &request()).unwrap(),
            DispatchOutcome::Queued {
                job_id: "first".into(),
            }
        );
    }

    #[test]
    fn provider_registry_requires_the_dispatch_target_to_be_requested() {
        assert_eq!(
            ProviderRegistry::new().dispatch(Platform::Bilibili, &request()),
            Err(DomainError::ProviderPlatformNotTarget {
                platform: Platform::Bilibili,
            })
        );
    }

    #[test]
    fn provider_registry_makes_missing_and_declared_unavailable_explicit() {
        let mut registry = ProviderRegistry::new();
        let (provider, calls) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Unavailable {
                reason: "browser login required".into(),
            },
            DispatchOutcome::Queued {
                job_id: "must-not-run".into(),
            },
            None,
        );
        registry.register(provider).unwrap();

        assert_eq!(
            registry.dispatch(Platform::Douyin, &request()).unwrap(),
            DispatchOutcome::Unavailable {
                reason: "browser login required".into(),
            }
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            registry.availability(Platform::Bilibili),
            ProviderAvailability::Unavailable {
                reason: "no provider registered for blbl".into(),
            }
        );
    }

    #[test]
    fn provider_registry_aggregates_every_target_without_stopping_on_rejection() {
        let mut request = request();
        request.targets = vec![Platform::Bilibili, Platform::Douyin];
        request.overrides.clear();
        let mut registry = ProviderRegistry::new();
        let (provider, calls) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "must-not-run".into(),
            },
            Some("adapter failed"),
        );
        registry.register(provider).unwrap();

        let report = registry.dispatch_all(&request).unwrap();
        assert_eq!(
            report.outcomes,
            BTreeMap::from([
                (
                    Platform::Douyin,
                    DispatchOutcome::Rejected {
                        reason: "remote media error: adapter failed".into(),
                    },
                ),
                (
                    Platform::Bilibili,
                    DispatchOutcome::Unavailable {
                        reason: "no provider registered for blbl".into(),
                    },
                ),
            ])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parses_canonical_and_alias_platforms() {
        assert_eq!(Platform::from_str("dy"), Ok(Platform::Douyin));
        assert_eq!(Platform::from_str("抖音"), Ok(Platform::Douyin));
        assert_eq!(
            serde_json::to_string(&Platform::WechatChannels).unwrap(),
            "\"sph\""
        );
        assert_eq!(Platform::ALL.len(), 8);
    }
    #[test]
    fn validation_models_all_publish_fields() {
        assert!(request().validate().is_ok());
        let mut invalid = request();
        invalid.overrides.push(invalid.overrides[0].clone());
        assert_eq!(invalid.validate(), Err(DomainError::DuplicateOverrides));
    }
    #[test]
    fn local_schedule_is_exact() {
        assert!(LocalSchedule::parse("2026-01-02 03:04:05").is_ok());
        assert!(LocalSchedule::parse("2026-01-02T03:04:05Z").is_err());
    }
    #[test]
    fn sqlite_persists_and_transitions_deterministically() {
        let repository = SqliteRepository::in_memory().unwrap();
        let now = Utc::now();
        let job = repository.enqueue(&request(), now).unwrap();
        let dispatched = repository
            .advance(&job.id, 0, PublishState::Dispatching, now)
            .unwrap();
        assert_eq!(dispatched.revision, 1);
        assert!(matches!(
            repository.advance(&job.id, 0, PublishState::Published, now),
            Err(DomainError::StaleJobRevision { .. })
        ));
        assert_eq!(repository.job(&job.id).unwrap(), Some(dispatched));
    }
    #[test]
    fn sqlite_persists_safe_accounts_and_history() {
        let repository = SqliteRepository::in_memory().unwrap();
        let account = Account {
            id: "dy-primary".into(),
            platform: Platform::Douyin,
            display_name: "Primary".into(),
            status: AccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:dy".into(),
        };
        repository.save_account(&account).unwrap();
        assert_eq!(repository.accounts().unwrap(), vec![account]);
        let record = HistoryRecord {
            id: "history-1".into(),
            request: request(),
            state: PublishState::Queued,
            recorded_at: Utc::now(),
            detail: None,
        };
        repository.append_history(&record).unwrap();
        assert_eq!(repository.history().unwrap(), vec![record]);
    }
    #[test]
    fn history_filter_includes_its_cutoff_and_intersects_platform_and_status() {
        let now = Utc::now();
        let cutoff = now - ChronoDuration::days(7);
        let filter = HistoryFilter::from_query(
            Some(7),
            false,
            Some(Platform::Douyin),
            Some(HistoryStatus::Scheduled),
            now,
        )
        .unwrap();
        let mut matching = request();
        matching.targets = vec![Platform::Douyin];
        let mut wrong_platform = matching.clone();
        wrong_platform.targets = vec![Platform::Bilibili];
        let history = vec![
            HistoryRecord {
                id: "inclusive".into(),
                request: matching.clone(),
                state: PublishState::Queued,
                recorded_at: cutoff,
                detail: None,
            },
            HistoryRecord {
                id: "draft-is-not-scheduled".into(),
                request: matching.clone(),
                state: PublishState::Draft,
                recorded_at: now,
                detail: None,
            },
            HistoryRecord {
                id: "wrong-platform".into(),
                request: wrong_platform,
                state: PublishState::Queued,
                recorded_at: now,
                detail: None,
            },
            HistoryRecord {
                id: "wrong-state".into(),
                request: matching,
                state: PublishState::Published,
                recorded_at: now,
                detail: None,
            },
        ];
        assert_eq!(
            filter
                .filter(history)
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec!["inclusive"]
        );
    }
    #[test]
    fn sqlite_persists_safe_article_accounts_without_session_material() {
        let repository = SqliteRepository::in_memory().unwrap();
        let account = ArticleAccount {
            id: "juejin-primary".into(),
            platform: ArticlePlatform::Juejin,
            display_name: "Primary".into(),
            status: ArticleAccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:juejin".into(),
        };
        repository.save_article_account(&account).unwrap();
        assert_eq!(repository.article_accounts().unwrap(), vec![account]);
    }
    #[test]
    fn migration_adds_safe_account_routes_to_a_version_three_database() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY); INSERT INTO schema_migrations(version) VALUES (3); CREATE TABLE accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE article_accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL);").unwrap();
        SqliteRepository::migrate(&connection).unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(accounts)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"phone".into()));
        assert!(columns.contains(&"partition".into()));
    }
    #[test]
    fn compatibility_dto_preserves_remote_url_and_per_target_account() {
        let dto: UpstreamPublishDto = serde_json::from_str(r#"{"platforms":[{"platform":"dy","phone":"one"}],"file":"https://example.invalid/video.mp4","title":"T","bt2":"legacy","tags":"one, two three","creativeStatements":{"抖音":"original"}}"#).unwrap();
        let request = PublishRequest::try_from(dto).unwrap();
        assert!(matches!(request.source, MediaSource::RemoteUrl(_)));
        assert_eq!(request.bt2.as_deref(), Some("legacy"));
        assert_eq!(
            request.overrides[0]
                .account
                .as_ref()
                .and_then(|item| item.phone.as_deref()),
            Some("one")
        );
        assert_eq!(request.tags, vec!["one", "two", "three"]);
    }
    #[test]
    fn compatibility_dto_accepts_single_platform_and_sph_option_map() {
        let dto: UpstreamPublishDto = serde_json::from_str(r#"{"platform":"sph","file":"movie.mp4","title":"T","tags":["a"],"sphProductId":"product","platformOptions":{"sph":{"link":{"type":"product","value":"value"}}}}"#).unwrap();
        let request = PublishRequest::try_from(dto).unwrap();
        assert_eq!(request.targets, vec![Platform::WechatChannels]);
        assert_eq!(request.wechat_link.product_id.as_deref(), Some("product"));
        assert_eq!(request.wechat_link.link_type.as_deref(), Some("product"));
        assert_eq!(request.wechat_link.link_value.as_deref(), Some("value"));
    }
    #[test]
    fn article_request_preserves_all_fields() {
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("p".into()),
                partition: Some("q".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: Some("cover".into()),
            category: Some("rust".into()),
            tags: vec!["tag".into()],
            summary: Some("summary".into()),
            scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn runner_safe_requests_omit_all_account_routing_from_serialized_payloads() {
        let mut video = request();
        video.overrides[0].account = Some(AccountSelection {
            phone: Some("override-phone".into()),
            partition: Some("persist:override".into()),
        });
        let serialized = serde_json::to_string(&ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            request: video.runner_safe(),
        })
        .unwrap();
        assert!(!serialized.contains("masked"));
        assert!(!serialized.contains("override-phone"));
        assert!(!serialized.contains("partition"));
        let article = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("article-phone".into()),
                partition: Some("persist:article".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        let serialized = serde_json::to_string(&ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: article.runner_safe(),
        })
        .unwrap();
        assert!(!serialized.contains("article-phone"));
        assert!(!serialized.contains("partition"));
    }

    #[test]
    fn tcp_runner_provider_serializes_only_runner_safe_routing_data() {
        let mut routed = request();
        routed.account.phone = Some("top-level-phone".into());
        routed.account.partition = Some("persist:top-level".into());
        routed.overrides[0].account = Some(AccountSelection {
            phone: Some("override-phone".into()),
            partition: Some("persist:override".into()),
        });
        let provider = TcpRunnerProvider {
            platform: Platform::Douyin,
            address: "127.0.0.1:39001".parse().unwrap(),
        };
        let transport = CapturingRunnerTransport(Mutex::new(None));
        assert_eq!(
            provider.enqueue_with(&routed, &transport).unwrap(),
            DispatchOutcome::Queued {
                job_id: "safe-job".into(),
            }
        );
        let (endpoint, payload) = transport.0.lock().unwrap().take().unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:39001/v1/publish");
        assert!(!payload.contains("top-level-phone"));
        assert!(!payload.contains("override-phone"));
        assert!(!payload.contains("partition"));
        assert!(!payload.contains("phone"));
    }

    #[test]
    fn article_runner_protocol_rejects_unknown_fields_and_non_juejin_requests() {
        assert!(serde_json::from_str::<ArticleRunnerRequest>(
            r#"{"version":1,"request":{"platform":"juejin","account":{},"title":"T","content":"body","file":null,"cover":null,"category":null,"tags":[],"summary":null,"scheduled_at":null},"token":"forbidden"}"#,
        )
        .is_err());
        assert!(serde_json::from_str::<ArticleRunnerRequest>(
            r#"{"version":1,"request":{"platform":"juejin","account":{"session":"forbidden"},"title":"T","content":"body","file":null,"cover":null,"category":null,"tags":[],"summary":null,"scheduled_at":null}}"#,
        )
        .is_err());
        let request = PublishArticleRequest {
            platform: "dy".into(),
            account: Default::default(),
            title: "T".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        assert_eq!(
            request.validate(),
            Err(DomainError::UnknownPlatform("dy".into()))
        );
    }

    #[test]
    fn article_runner_response_requires_matching_version_platform_and_job() {
        let accepted = ArticleRunnerResponse::Queued {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            job_id: "article-1".into(),
            automation_attempted: true,
        };
        assert_eq!(
            accepted.into_dispatch(ArticlePlatform::Juejin),
            Some(ArticleDispatchOutcome::Queued {
                job_id: "article-1".into(),
            })
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION + 1,
                platform: ArticlePlatform::Juejin,
                job_id: "article-1".into(),
                automation_attempted: true,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
        assert!(
            serde_json::from_str::<ArticleRunnerResponse>(
                r#"{"outcome":"queued","version":1,"platform":"juejin","job_id":"article-1"}"#,
            )
            .is_err()
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id: "article-1".into(),
                automation_attempted: false,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id: " ".into(),
                automation_attempted: true,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
    }

    #[test]
    fn article_runner_adapter_strips_routing_and_uses_only_the_article_endpoint() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("article-phone".into()),
                partition: Some("persist:article".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (
                200,
                r#"{"outcome":"queued","version":1,"platform":"juejin","job_id":"article-job","automation_attempted":true}"#
                    .into(),
            ),
        };
        assert_eq!(
            runner.dispatch_with(&request, &transport).unwrap(),
            ArticleDispatchOutcome::Queued {
                job_id: "article-job".into(),
            }
        );
        let (endpoint, payload) = transport.captured.lock().unwrap().take().unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:39002/v1/publish-article");
        assert!(!payload.contains("article-phone"));
        assert!(!payload.contains("persist:article"));
        assert!(!payload.contains("partition"));
    }

    #[test]
    fn article_runner_adapter_rejects_unsafe_endpoints_and_malformed_responses() {
        assert_eq!(
            ArticleRunner::parse_cli("tcp:192.0.2.1:39002"),
            Err(ArticleRunnerConfigError::TcpMustBeLoopback)
        );
        assert_eq!(
            ArticleRunner::parse_cli("tcp:127.0.0.1:39002?token=forbidden"),
            Err(ArticleRunnerConfigError::CredentialLikeEndpoint)
        );
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (200, "not json".into()),
        };
        assert!(matches!(
            runner
                .dispatch_with(
                    &PublishArticleRequest {
                        platform: "juejin".into(),
                        account: Default::default(),
                        title: "title".into(),
                        content: Some("body".into()),
                        file: None,
                        cover: None,
                        category: None,
                        tags: Vec::new(),
                        summary: None,
                        scheduled_at: None,
                    },
                    &transport,
                )
                .unwrap(),
            ArticleDispatchOutcome::Rejected { .. }
        ));
    }

    #[test]
    fn article_runner_adapter_marks_transport_failure_as_attempted() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let outcome = runner
            .dispatch_with(
                &PublishArticleRequest {
                    platform: "juejin".into(),
                    account: Default::default(),
                    title: "title".into(),
                    content: Some("body".into()),
                    file: None,
                    cover: None,
                    category: None,
                    tags: Vec::new(),
                    summary: None,
                    scheduled_at: None,
                },
                &FailingArticleRunnerTransport,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            ArticleDispatchOutcome::Rejected {
                automation_attempted: true,
                ..
            }
        ));
    }

    #[test]
    fn article_runner_adapter_rejects_schedules_without_transport_dispatch() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (200, String::new()),
        };
        let outcome = runner
            .dispatch_with(
                &PublishArticleRequest {
                    platform: "juejin".into(),
                    account: Default::default(),
                    title: "title".into(),
                    content: Some("body".into()),
                    file: None,
                    cover: None,
                    category: None,
                    tags: Vec::new(),
                    summary: None,
                    scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
                },
                &transport,
            )
            .unwrap();
        assert!(matches!(outcome, ArticleDispatchOutcome::Rejected { .. }));
        assert!(transport.captured.lock().unwrap().is_none());
    }
    #[test]
    fn stager_rejects_missing_content_type_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "missing-content-type",
            staging_policy(3),
            TestTransport::response(None, Some("3"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_disallowed_content_type_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "disallowed-content-type",
            staging_policy(3),
            TestTransport::response(Some("text/plain"), Some("3"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_invalid_content_length_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "invalid-length",
            staging_policy(3),
            TestTransport::response(
                Some("video/mp4"),
                Some("not-a-number"),
                Cursor::new(b"abc".to_vec()),
            ),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_declared_too_large_content_length_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "declared-too-large",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), Some("4"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_removes_created_file_when_stream_exceeds_limit() {
        let filesystem = TestFilesystem::file();
        assert_staging_error_leaves_no_file(
            "stream-too-large",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abcd".to_vec())),
            &filesystem,
        );
        assert_eq!(filesystem.created_count(), 1);
    }

    #[test]
    fn stager_removes_created_file_when_reader_fails() {
        assert_staging_error_leaves_no_file(
            "read-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, FailingReader),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_removes_created_file_when_writer_fails() {
        assert_staging_error_leaves_no_file(
            "write-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abc".to_vec())),
            &TestFilesystem::failing(TestOutput::FailWrite),
        );
    }

    #[test]
    fn stager_removes_created_file_when_flush_fails() {
        assert_staging_error_leaves_no_file(
            "flush-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abc".to_vec())),
            &TestFilesystem::failing(TestOutput::FailFlush),
        );
    }

    #[test]
    fn stager_retries_name_collisions_without_overwriting_existing_file() {
        let directory = staging_directory("collision");
        fs::create_dir_all(&directory).unwrap();
        let existing = directory.join("taken");
        fs::write(&existing, b"preserved").unwrap();
        let policy = staging_policy(3);
        let stager = HttpRemoteMediaStager::new(directory.clone());
        let transport =
            TestTransport::response(Some("video/mp4"), Some("3"), Cursor::new(b"abc".to_vec()));
        let mut names = TestNames(VecDeque::from(["taken".into(), "fresh".into()]));
        let staged = stager
            .stage_with(
                &staging_request(&policy),
                &policy,
                &transport,
                &TestFilesystem::file(),
                &mut names,
            )
            .unwrap();
        assert_eq!(fs::read(&existing).unwrap(), b"preserved");
        assert_eq!(staged.path(), directory.join("fresh"));
        assert_eq!(fs::read(staged.path()).unwrap(), b"abc");
        staged.cleanup().unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn configured_tcp_runner_installs_the_versioned_execution_adapter() {
        let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        let registry = ProviderRegistry::from_runners([runner]).unwrap();
        assert_eq!(
            registry.availability(Platform::Douyin),
            ProviderAvailability::Available,
        );
    }

    #[test]
    fn runner_response_requires_matching_version_and_platform() {
        let accepted = ProviderRunnerResponse::Queued {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            job_id: "job".into(),
        };
        assert_eq!(
            accepted.into_dispatch(Platform::Douyin),
            Some(DispatchOutcome::Queued {
                job_id: "job".into()
            })
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION + 1,
                platform: Platform::Douyin,
                job_id: "job".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Douyin,
                job_id: "   ".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
        assert!(
            serde_json::from_str::<ProviderRunnerResponse>(
                r#"{"outcome":"queued","version":1,"platform":"dy","job_id":"job","extra":true}"#,
            )
            .is_err()
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Kuaishou,
                job_id: "job".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
    }

    #[test]
    fn runner_configuration_rejects_nonlocal_duplicate_and_credential_like_endpoints() {
        assert_eq!(
            ProviderRunner::parse_cli("dy=tcp:192.0.2.1:39001"),
            Err(ProviderRunnerConfigError::TcpMustBeLoopback)
        );
        assert_eq!(
            ProviderRunner::parse_cli("dy=unix:/run/matrixpost/token.sock"),
            Err(ProviderRunnerConfigError::CredentialLikeEndpoint)
        );
        let runner = ProviderRunner::parse_cli("dy=unix:/run/matrixpost/dy.sock").unwrap();
        assert!(matches!(
            ProviderRegistry::from_runners([runner.clone(), runner]),
            Err(ProviderRunnerConfigError::DuplicatePlatform {
                platform: Platform::Douyin,
            })
        ));
    }
}
