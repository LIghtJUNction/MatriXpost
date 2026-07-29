use matrixpost_core::Platform;

use super::{ArticleProfile, PlatformProfile};

pub(crate) const JUEJIN_PROFILE: ArticleProfile = ArticleProfile {
    editor_url: "https://juejin.cn/editor/drafts/new",
    title: &["input[placeholder*='标题']", "input[aria-label*='标题']"],
    content: &[
        "div.cm-content[contenteditable='true']",
        ".cm-editor div[contenteditable='true']",
    ],
    cover: &[
        "input[type='file'][accept*='image']",
        "input[data-testid='cover-upload']",
    ],
    category: &[
        "input[placeholder*='分类']",
        "button[data-testid='category']",
    ],
    tags: &[
        "input[placeholder*='标签']",
        "input[data-testid='tag-input']",
    ],
    summary: &[
        "textarea[placeholder*='摘要']",
        "input[placeholder*='摘要']",
    ],
    publish_panel: &[
        "button[data-testid='publish-article']",
        "button[class*='publish']",
    ],
    confirm: &[
        "button[data-testid='confirm-publish']",
        "button[class*='confirm']",
    ],
    success: &[
        "[data-testid='publish-success']",
        "[role='status'][data-status='success']",
    ],
};

pub(crate) const PROFILES: &[PlatformProfile] = &[
    PlatformProfile {
        platform: Platform::Douyin,
        upload_url: "https://creator.douyin.com/creator-micro/content/post/video?enter_from=publish_page",
        file: &["input[type='file']", "input.upload-input"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-e2e='publish-success']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::WechatChannels,
        upload_url: "https://channels.weixin.qq.com/platform/post/create",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: Some(&[
            "wujie-app.wujie_iframe input[placeholder='填写短标题有机会获得更多流量']",
        ]),
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-status='published']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Bilibili,
        upload_url: "https://member.bilibili.com/platform/upload/video/frame/",
        file: &["input[type='file']", "input[type='file'][accept*='video']"],
        title: &["input[placeholder*='标题']", "input[aria-label*='标题']"],
        short_title: None,
        description: &[
            "textarea[placeholder*='简介']",
            "div[contenteditable='true']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".success-wrap", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Baijiahao,
        upload_url: "https://baijiahao.baidu.com/builder/rc/edit?type=videoV2&is_from_cms=1",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
        short_title: None,
        description: &[
            "textarea[placeholder*='摘要']",
            "div[contenteditable='true']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-status='published']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Toutiao,
        upload_url: "https://mp.toutiao.com/profile_v4/xigua/upload-video",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='简介']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
    PlatformProfile {
        platform: Platform::Kuaishou,
        upload_url: "https://cp.kuaishou.com/article/publish/video?tabType=1",
        file: &["input[type='file']", "input.upload-file"],
        title: &["#work-description-edit", "input[placeholder*='标题']"],
        short_title: None,
        description: &["#work-description-edit", "textarea[placeholder*='描述']"],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-result-success", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Xiaohongshu,
        upload_url: "https://creator.xiaohongshu.com/publish/publish?from=menu&target=video",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='填写标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='正文']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
    PlatformProfile {
        platform: Platform::FanqieVideo,
        upload_url: "https://pugc.yueduwuxian.com/fqvideo/home/publish-video",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
];

pub(crate) fn profile(platform: Platform) -> Option<&'static PlatformProfile> {
    PROFILES.iter().find(|profile| profile.platform == platform)
}
