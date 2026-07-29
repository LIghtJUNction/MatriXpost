//! Deterministic compatibility metadata for the public HTTP surface.
//!
//! These values describe the upstream request vocabulary only. They are not a
//! provider probe and never report account, browser, or remote platform state.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Serialize)]
struct PlatformSpec {
    code: &'static str,
    name: &'static str,
    aliases: &'static [&'static str],
    automated: bool,
    note: Option<&'static str>,
    #[serde(rename = "hasConfig")]
    has_config: Option<bool>,
}

/// MatrixMedia-compatible platform vocabulary.
///
/// `automated` is a static compatibility capability, not a claim that a
/// provider is configured or available. Use `/providers` for that local state.
pub(crate) fn platforms_response() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "platforms": [
            platform("dy", "抖音", &["douyin", "抖音"], true, None),
            platform("sph", "视频号", &["视频号"], true, None),
            platform("blbl", "哔哩哔哩", &["bilibili", "哔哩哔哩"], true, None),
            platform("bjh", "百家号", &["百家号"], true, None),
            platform("tt", "头条", &["toutiao", "头条"], true, None),
            platform("ks", "快手", &["kuaishou", "快手"], true, None),
            platform("xhs", "小红书", &["xiaohongshu", "小红书"], true, None),
            platform(
                "fqsp",
                "番茄视频",
                &["fanqie", "fq", "番茄视频"],
                false,
                Some("配置已接入，自动发布流程待完善"),
            ),
        ]
    })
}

fn platform(
    code: &'static str,
    name: &'static str,
    aliases: &'static [&'static str],
    automated: bool,
    note: Option<&'static str>,
) -> PlatformSpec {
    PlatformSpec {
        code,
        name,
        aliases,
        automated,
        note,
        // MatrixMedia uses a GUI-held configuration map. The daemon has no
        // equivalent dynamic configuration, so it intentionally says unknown.
        has_config: None,
    }
}

#[derive(Serialize)]
struct CreativeStatementOption {
    value: &'static str,
    label: &'static str,
    #[serde(rename = "onlyPlatforms")]
    only_platforms: Option<&'static [&'static str]>,
}

#[derive(Serialize)]
struct PlatformCreativeStatements {
    name: &'static str,
    supports: bool,
    options: Vec<PlatformCreativeStatementOption>,
}

#[derive(Serialize)]
struct PlatformCreativeStatementOption {
    value: &'static str,
    label: &'static str,
}

/// MatrixMedia-compatible creative-statement form specification.
pub(crate) fn creative_statements_response() -> serde_json::Value {
    let mut platforms = BTreeMap::new();
    platforms.insert(
        "dy",
        platform_statements(
            "抖音",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
            ],
        ),
    );
    platforms.insert(
        "sph",
        platform_statements(
            "视频号",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
                "self_shot",
            ],
        ),
    );
    platforms.insert(
        "blbl",
        platform_statements(
            "哔哩哔哩",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
                "self_made_no_repost",
            ],
        ),
    );
    platforms.insert(
        "bjh",
        platform_statements(
            "百家号",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
            ],
        ),
    );
    platforms.insert(
        "tt",
        platform_statements("头条", true, &["ai_generated", "fiction", "repost"]),
    );
    platforms.insert(
        "ks",
        platform_statements(
            "快手",
            true,
            &["ai_generated", "fiction", "personal_opinion", "repost"],
        ),
    );
    platforms.insert(
        "xhs",
        platform_statements("小红书", true, &["ai_generated", "fiction", "marketing"]),
    );
    platforms.insert("fqsp", platform_statements("番茄视频", false, &[]));

    serde_json::json!({
        "success": true,
        "default": "none",
        "batchOptions": batch_options(),
        "platforms": platforms,
        "input": {
            "creativeStatement": "全局默认声明，等同 GUI「批量设置创作声明」；支持 value、中文 label 或各平台页面原文案",
            "creativeStatements": "按平台覆盖，key 可用 code（dy/blbl）或中文名；如 { \"dy\": \"ai_generated\", \"blbl\": \"fiction\" }",
            "perPlatform": "platforms 对象数组内可传 creativeStatement / cs 覆盖单平台",
            "fallback": "所选声明在某平台无对应选项时自动回退为 none（无标注）",
        }
    })
}

fn batch_options() -> [CreativeStatementOption; 8] {
    [
        CreativeStatementOption {
            value: "none",
            label: "无标注",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "ai_generated",
            label: "AI生成",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "fiction",
            label: "虚构演绎",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "marketing",
            label: "营销推广",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "personal_opinion",
            label: "个人观点",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "repost",
            label: "转载",
            only_platforms: None,
        },
        CreativeStatementOption {
            value: "self_shot",
            label: "自行拍摄",
            only_platforms: Some(&["sph"]),
        },
        CreativeStatementOption {
            value: "self_made_no_repost",
            label: "自制禁转载",
            only_platforms: Some(&["blbl"]),
        },
    ]
}

fn platform_statements(
    name: &'static str,
    supports: bool,
    values: &[&'static str],
) -> PlatformCreativeStatements {
    PlatformCreativeStatements {
        name,
        supports,
        options: values
            .iter()
            .map(|value| PlatformCreativeStatementOption {
                value,
                label: platform_label(name, value),
            })
            .collect(),
    }
}

fn platform_label(platform: &str, value: &str) -> &'static str {
    match (platform, value) {
        ("哔哩哔哩", "none") => "内容无需标注",
        ("抖音", "none") => "无需添加自主声明",
        ("百家号", "none") => "无需声明",
        ("视频号", "none") => "无需标注",
        ("哔哩哔哩" | "百家号" | "视频号", "ai_generated") => "含AI生成内容",
        ("抖音", "ai_generated") => "内容由AI生成",
        ("快手", "ai_generated") => "内容为AI生成",
        ("头条", "ai_generated") => "AI生成",
        ("小红书", "ai_generated") => "笔记含AI合成内容",
        ("哔哩哔哩" | "百家号", "fiction") => "含虚构演绎内容",
        ("抖音", "fiction") => "虚构演绎，仅供娱乐",
        ("快手", "fiction") => "演绎情节，仅供娱乐",
        ("头条", "fiction") => "虚构演绎，故事经历",
        ("小红书", "fiction") => "虚构演绎，仅供娱乐",
        ("视频号", "fiction") => "内容为虚构剧情，仅供娱乐",
        ("哔哩哔哩" | "百家号", "marketing") => "内容含营销信息",
        ("抖音", "marketing") => "内容含营销推广信息",
        ("小红书" | "视频号", "marketing") => "内容包含营销广告",
        ("哔哩哔哩" | "百家号" | "快手" | "视频号", "personal_opinion") => {
            "个人观点，仅供参考"
        }
        ("抖音", "personal_opinion") => "内容为个人观点或见解",
        ("哔哩哔哩" | "百家号" | "视频号", "repost") => "内容为转载",
        ("抖音", "repost") => "内容为转载信息",
        ("头条", "repost") => "取自站外",
        ("快手", "repost") => "素材来源于网络",
        ("视频号", "self_shot") => "内容为自行拍摄",
        ("哔哩哔哩", "self_made_no_repost") => "内容为自制：未经作者允许，禁止转载",
        _ => unreachable!("unsupported static statement tuple: {platform}/{value}"),
    }
}
