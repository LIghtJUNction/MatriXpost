use super::super::{AccountSelection, Platform};
use super::{LocalSchedule, MediaSource, PlatformOverride, PublishRequest, WechatLink};
use crate::error::DomainError;
use serde::Deserialize;
use std::{collections::BTreeMap, str::FromStr};
use url::Url;

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
