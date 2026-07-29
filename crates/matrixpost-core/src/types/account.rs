use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

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
