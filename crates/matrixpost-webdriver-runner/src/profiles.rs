use std::time::Duration;

use matrixpost_core::Platform;
use url::Url;

pub(crate) const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";
pub(crate) const ELEMENT_POLL_ATTEMPTS: usize = 3;
pub(crate) const ELEMENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const ACKNOWLEDGEMENT_ATTEMPTS: usize = 60;
pub(crate) const ACKNOWLEDGEMENT_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const DEBUGGER_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
pub(crate) const VISIBLE_SCRIPT: &str = r#"const e=arguments[0];const s=getComputedStyle(e);const r=e.getBoundingClientRect();return !(e.getAttribute('aria-hidden')==='true'||s.display==='none'||s.visibility==='hidden'||Number(s.opacity)===0||r.width===0||r.height===0);"#;
pub(crate) const CODEMIRROR_WRITE_SCRIPT: &str = r#"const root=arguments[0],text=arguments[1];const editor=root.closest('.cm-editor')||root;const view=editor.cmView?.view||root.cmView?.view;if(view){view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});return view.state.doc.toString()===text;}root.focus();root.textContent=text;root.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:text}));return root.textContent===text;"#;
pub(crate) const MAX_ARTICLE_BODY_BYTES: usize = 1_000_000;
pub(crate) const MAX_ARTICLE_TITLE_BYTES: usize = 200;
pub(crate) const MAX_ARTICLE_CATEGORY_BYTES: usize = 64;
pub(crate) const MAX_ARTICLE_TAGS: usize = 10;
pub(crate) const MAX_ARTICLE_TAG_BYTES: usize = 32;
pub(crate) const MAX_ARTICLE_SUMMARY_BYTES: usize = 500;
pub(crate) const MAX_ARTICLE_COVER_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const ARTICLE_TEXT_EXTENSIONS: &[&str] = &["md", "txt"];
pub(crate) const ARTICLE_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
/// Remote videos are staged only into the user-selected directory before a
/// browser session is created.  Two GiB is large enough for ordinary platform
/// uploads while keeping the local runner's disk and network exposure bounded.
pub(crate) const MAX_REMOTE_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Accepted MIME types are deliberately finite: the runner stages videos, not
/// arbitrary remote objects. Parameters such as `charset` remain accepted by
/// the core policy's prefix comparison.
pub(crate) const REMOTE_VIDEO_CONTENT_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];
/// A remote source is untrusted input. Keep every execution failure at the
/// provider boundary intentionally opaque so neither its URL nor its staged
/// local path can be reflected to callers.
pub(crate) const REMOTE_MEDIA_EXECUTION_REJECTION: &str = "remote media publication failed";
pub(crate) const FANQIE_VIDEO_LIST_URL: &str =
    "https://pugc.yueduwuxian.com/fqvideo/home/video-list";
pub(crate) const FANQIE_REVIEW_SCROLL_ATTEMPTS: usize = 6;
pub(crate) const FANQIE_REVIEW_SCROLL_INTERVAL: Duration = Duration::from_millis(250);
/// This script is deliberately a one-way classifier. It receives a bounded
/// title query but returns only one fixed status token, never card/page text,
/// URLs, identifiers, or selectors.
pub(crate) const FANQIE_REVIEW_STATUS_SCRIPT: &str = r#"const n=v=>String(v||'').replace(/\s+/g,'').trim();const q=n(arguments[0]);for(const card of document.querySelectorAll('.video-card')){const t=n(card.querySelector('.video-card-title')?.textContent);if(!t||!t.includes(q))continue;const s=n(card.querySelector('.video-status')?.textContent);if(/审核未通过|未通过|驳回|违规|失败/.test(s))return'rejected';if(/审核中|待审核/.test(s))return'under_review';if(/已发布|发布成功|已上线|公开|正常/.test(s))return'published';return'under_review';}window.scrollBy(0,Math.max(window.innerHeight,800));return null;"#;
/// Every shadow-root metadata phase has a fixed deadline. The runner never
/// waits for unbounded UI activity in an attached user browser.
pub(crate) const WECHAT_SHADOW_ACTION_POLL_ATTEMPTS: usize = 30;
pub(crate) const WECHAT_SHADOW_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// These scripts return only booleans. Product names, identifiers, and DOM
/// content deliberately never leave the attached page through WebDriver.
pub(crate) const WECHAT_PRODUCT_TYPE_READY_SCRIPT: &str = r#"const app=document.querySelector('wujie-app.wujie_iframe');const root=app?.shadowRoot;if(!root)return false;const link=root.querySelector('.post-with-link');if(!link)return false;const selected=String(link.querySelector('.choosen-link-wrap span')?.textContent||'').trim();const chooser=link.querySelector('.post-component-choose-wrap .content-wrap');if(selected==='商品'&&chooser)return true;const menu=link.querySelector('.link-list-options');const visible=menu&&getComputedStyle(menu).display!=='none';if(!visible){link.querySelector('.link-display-wrap')?.click();return false;}const product=Array.from(menu.querySelectorAll('.link-option-item')).find(item=>String(item.textContent||'').replace(/\s+/g,'')==='商品');product?.click();return false;"#;
pub(crate) const WECHAT_PRODUCT_OPEN_CHOOSER_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const chooser=root?.querySelector('.post-with-link .post-component-choose-wrap .content-wrap');if(!chooser)return false;chooser.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>String(dialog.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(dialog).display!=='none'));"#;
pub(crate) const WECHAT_PRODUCT_SEARCH_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const input=dialog?.querySelector('input[placeholder="请输入商品名称/编码搜索"]');const button=dialog?.querySelector('.search-btn button');if(!input||!button)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set;if(!set)return false;set.call(input,id);input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));button.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_EXACT_ROW_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Array.from(root?.querySelectorAll('.weui-desktop-dialog tr[data-row-key]')||[]).some(row=>row.getAttribute('data-row-key')===id);"#;
pub(crate) const WECHAT_PRODUCT_SELECT_EXACT_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const row=Array.from(root?.querySelectorAll('.weui-desktop-dialog tr[data-row-key]')||[]).find(item=>item.getAttribute('data-row-key')===id);const radio=Array.from(row?.querySelectorAll('input.ant-radio-input')||[]).find(item=>item.value===id);if(!radio)return false;radio.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_ADD_READY_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const button=Array.from(dialog?.querySelectorAll('.weui-desktop-btn_primary')||[]).find(item=>/^添加(?:\(\d+\))?$/.test(String(item.textContent||'').replace(/\s+/g,'')));return Boolean(button&&!button.disabled&&!button.classList.contains('weui-desktop-btn_disabled'));"#;
pub(crate) const WECHAT_PRODUCT_ADD_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const button=Array.from(dialog?.querySelectorAll('.weui-desktop-btn_primary')||[]).find(item=>/^添加(?:\(\d+\))?$/.test(String(item.textContent||'').replace(/\s+/g,'')));if(!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;button.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_ATTACHED_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const name=root?.querySelector('.post-with-link .post-component-choose-wrap .choose-content .name');return Boolean(String(name?.textContent||'').trim());"#;
/// These scripts accept only a known creative-statement label and return a
/// boolean. They never expose page content to the runner.
pub(crate) const WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const trigger=root?.querySelector('.post-with-mark-tag .select-display');if(!trigger)return false;trigger.click();return true;"#;
pub(crate) const WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const option=Array.from(root?.querySelectorAll('.post-with-mark-tag .mark-tag-option')||[]).find(item=>String(item.querySelector('.option-main')?.textContent||'').trim()===expected);if(!option)return false;option.click();return true;"#;
/// Original-declaration scripts deliberately return only whether each local
/// page action is available or completed. The declaration itself is attempted
/// only for an explicit WeChat publication request.
pub(crate) const WECHAT_ORIGINAL_ENTRY_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const entry=root?.querySelector('.declare-original-checkbox .ant-checkbox-wrapper');if(!entry)return false;entry.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_ANY_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>getComputedStyle(dialog).display!=='none'&&(dialog.querySelector('.weui-desktop-dialog__bd .protocol-text')||dialog.matches('.declare-original-dialog .weui-desktop-dialog'))));"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>getComputedStyle(dialog).display!=='none'&&dialog.querySelector('.weui-desktop-dialog__bd .protocol-text')));"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>getComputedStyle(item).display!=='none'&&item.querySelector('.weui-desktop-dialog__bd .protocol-text'));const protocol=dialog?.querySelector('.weui-desktop-dialog__bd .protocol-text');const button=Array.from(dialog?.querySelectorAll('button.weui-desktop-btn_primary')||[]).find(item=>String(item.textContent||'').trim().includes('声明原创'));if(!protocol||!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;protocol.click();button.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_DIALOG_GONE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return !Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).some(dialog=>getComputedStyle(dialog).display!=='none'&&dialog.querySelector('.weui-desktop-dialog__bd .protocol-text'));"#;
pub(crate) const WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');return Boolean(dialog&&getComputedStyle(dialog).display!=='none');"#;
pub(crate) const WECHAT_ORIGINAL_CONFIRM_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');const check=dialog?.querySelector('label.ant-checkbox-wrapper');const button=Array.from(dialog?.querySelectorAll('button.weui-desktop-btn_primary')||[]).find(item=>String(item.textContent||'').trim().includes('声明原创'));if(!check||!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;check.click();button.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_DECLARATION_DIALOG_GONE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');return !dialog||getComputedStyle(dialog).display==='none';"#;

pub(crate) fn normalize_review_title_query(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct AcknowledgementPolicy {
    pub(crate) attempts: usize,
    pub(crate) interval: Duration,
}

impl AcknowledgementPolicy {
    pub(crate) const fn production() -> Self {
        Self {
            attempts: ACKNOWLEDGEMENT_ATTEMPTS,
            interval: ACKNOWLEDGEMENT_INTERVAL,
        }
    }
}

/// The UI profile is deliberately data, not per-platform automation code.
pub(crate) struct PlatformProfile {
    pub(crate) platform: Platform,
    pub(crate) upload_url: &'static str,
    pub(crate) file: &'static [&'static str],
    pub(crate) title: &'static [&'static str],
    pub(crate) short_title: Option<&'static [&'static str]>,
    pub(crate) description: &'static [&'static str],
    pub(crate) submit: &'static [&'static str],
    pub(crate) draft: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}

pub(crate) struct ArticleProfile {
    pub(crate) editor_url: &'static str,
    pub(crate) title: &'static [&'static str],
    pub(crate) content: &'static [&'static str],
    pub(crate) cover: &'static [&'static str],
    pub(crate) category: &'static [&'static str],
    pub(crate) tags: &'static [&'static str],
    pub(crate) summary: &'static [&'static str],
    pub(crate) publish_panel: &'static [&'static str],
    pub(crate) confirm: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}

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
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "textarea[placeholder*='描述']",
            "div[contenteditable='true']",
        ],
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

pub(crate) fn sensitive(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
        "profile",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn local_webdriver_endpoint(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "WebDriver endpoint must be an absolute URL".to_owned())?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("WebDriver endpoint must be credential-free loopback HTTP".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if loopback && !sensitive(url.path()) {
        Ok(url)
    } else {
        Err("WebDriver endpoint must be credential-free loopback HTTP".into())
    }
}
