//! Platform, account, and publication data-transfer models.

use crate::error::DomainError;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    str::FromStr,
};
use url::Url;

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

/// Durable state for one scheduled article dispatch.  Article jobs are kept
/// separate from video jobs because their runner protocol and terminal history
/// are intentionally independent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleScheduledJob {
    pub id: String,
    pub request: PublishArticleRequest,
    pub state: PublishState,
    pub due_at: LocalSchedule,
    pub revision: u64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Immutable terminal evidence for a scheduled article's local runner
/// workflow. It is not evidence of remote Juejin publication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleHistoryRecord {
    pub id: String,
    /// The supported article platform. Account routing is intentionally absent.
    pub platform: ArticlePlatform,
    /// The requested article title. Body, files, runner endpoints, and account
    /// routes are deliberately excluded from durable history.
    pub title: String,
    pub state: PublishState,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// A fixed generic workflow outcome, never runner-provided diagnostics.
    pub detail: Option<String>,
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
    pub(crate) fn db(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }
    pub(crate) fn from_db(value: &str) -> Result<Self, DomainError> {
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
