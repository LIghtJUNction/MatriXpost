use super::super::Platform;
use serde::Serialize;

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
