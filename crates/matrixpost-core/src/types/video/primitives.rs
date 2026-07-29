use super::super::{AccountSelection, Platform};
use crate::error::DomainError;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};
use url::Url;

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
