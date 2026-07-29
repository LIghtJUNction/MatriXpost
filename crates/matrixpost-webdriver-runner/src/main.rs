//! Local-only bridge between MatriXpost and a separately managed WebDriver.
//!
//! This process neither reads browser profiles nor accepts credentials. A user
//! starts ChromeDriver (or another compatible WebDriver) separately with their
//! own local browser state, then explicitly points this runner at its loopback
//! endpoint.

use std::{
    fs::{self, File},
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use clap::Parser;
use matrixpost_core::{
    ARTICLE_RUNNER_PROTOCOL_VERSION, ArticlePlatform, ArticleRunnerRequest, ArticleRunnerResponse,
    HttpRemoteMediaStager, LOGIN_RUNNER_PROTOCOL_VERSION, LoginRunnerRequest, LoginRunnerResponse,
    MediaSource, MediaStagingPolicy, PROVIDER_RUNNER_PROTOCOL_VERSION, Platform,
    ProviderRunnerRequest, ProviderRunnerResponse, PublishArticleRequest, PublishRequest,
    RemoteMediaRequest, RemoteMediaStager, StagedMedia,
};
use serde_json::{Value, json};
use url::Url;

const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";
const ELEMENT_POLL_ATTEMPTS: usize = 3;
const ELEMENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
const ACKNOWLEDGEMENT_ATTEMPTS: usize = 60;
const ACKNOWLEDGEMENT_INTERVAL: Duration = Duration::from_secs(5);
const DEBUGGER_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
const VISIBLE_SCRIPT: &str = r#"const e=arguments[0];const s=getComputedStyle(e);const r=e.getBoundingClientRect();return !(e.getAttribute('aria-hidden')==='true'||s.display==='none'||s.visibility==='hidden'||Number(s.opacity)===0||r.width===0||r.height===0);"#;
const CODEMIRROR_WRITE_SCRIPT: &str = r#"const root=arguments[0],text=arguments[1];const editor=root.closest('.cm-editor')||root;const view=editor.cmView?.view||root.cmView?.view;if(view){view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});return view.state.doc.toString()===text;}root.focus();root.textContent=text;root.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:text}));return root.textContent===text;"#;
const MAX_ARTICLE_BODY_BYTES: usize = 1_000_000;
const MAX_ARTICLE_TITLE_BYTES: usize = 200;
const MAX_ARTICLE_CATEGORY_BYTES: usize = 64;
const MAX_ARTICLE_TAGS: usize = 10;
const MAX_ARTICLE_TAG_BYTES: usize = 32;
const MAX_ARTICLE_SUMMARY_BYTES: usize = 500;
const MAX_ARTICLE_COVER_BYTES: u64 = 5 * 1024 * 1024;
const ARTICLE_TEXT_EXTENSIONS: &[&str] = &["md", "txt"];
const ARTICLE_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
/// Remote videos are staged only into the user-selected directory before a
/// browser session is created.  Two GiB is large enough for ordinary platform
/// uploads while keeping the local runner's disk and network exposure bounded.
const MAX_REMOTE_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Accepted MIME types are deliberately finite: the runner stages videos, not
/// arbitrary remote objects. Parameters such as `charset` remain accepted by
/// the core policy's prefix comparison.
const REMOTE_VIDEO_CONTENT_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];
/// A remote source is untrusted input. Keep every execution failure at the
/// provider boundary intentionally opaque so neither its URL nor its staged
/// local path can be reflected to callers.
const REMOTE_MEDIA_EXECUTION_REJECTION: &str = "remote media publication failed";

#[derive(Clone, Copy)]
struct AcknowledgementPolicy {
    attempts: usize,
    interval: Duration,
}

impl AcknowledgementPolicy {
    const fn production() -> Self {
        Self {
            attempts: ACKNOWLEDGEMENT_ATTEMPTS,
            interval: ACKNOWLEDGEMENT_INTERVAL,
        }
    }
}

/// The UI profile is deliberately data, not per-platform automation code.
struct PlatformProfile {
    platform: Platform,
    upload_url: &'static str,
    file: &'static [&'static str],
    title: &'static [&'static str],
    description: &'static [&'static str],
    submit: &'static [&'static str],
    draft: &'static [&'static str],
    success: &'static [&'static str],
}

struct ArticleProfile {
    editor_url: &'static str,
    title: &'static [&'static str],
    content: &'static [&'static str],
    cover: &'static [&'static str],
    category: &'static [&'static str],
    tags: &'static [&'static str],
    summary: &'static [&'static str],
    publish_panel: &'static [&'static str],
    confirm: &'static [&'static str],
    success: &'static [&'static str],
}

const JUEJIN_PROFILE: ArticleProfile = ArticleProfile {
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

const PROFILES: &[PlatformProfile] = &[
    PlatformProfile {
        platform: Platform::Douyin,
        upload_url: "https://creator.douyin.com/creator-micro/content/upload",
        file: &["input[type='file']", "input.upload-input"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
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
        upload_url: "https://member.bilibili.com/platform/upload/video/frame",
        file: &["input[type='file']", "input[type='file'][accept*='video']"],
        title: &["input[placeholder*='标题']", "input[aria-label*='标题']"],
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
        upload_url: "https://baijiahao.baidu.com/builder/rc/edit?type=video",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
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
        upload_url: "https://mp.toutiao.com/profile_v4/graphic/publish",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
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
        upload_url: "https://cp.kuaishou.com/article/publish/video",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
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
        upload_url: "https://creator.xiaohongshu.com/publish/publish",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='填写标题']",
            "textarea[placeholder*='标题']",
        ],
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
        upload_url: "https://creator.fanqie.com/content/post",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
];

fn profile(platform: Platform) -> Option<&'static PlatformProfile> {
    PROFILES.iter().find(|profile| profile.platform == platform)
}

fn sensitive(value: &str) -> bool {
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

fn local_webdriver_endpoint(value: &str) -> Result<Url, String> {
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

trait WebDriverTransport: Send + Sync {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String>;
}

struct HttpWebDriver {
    endpoint: Url,
}

impl WebDriverTransport for HttpWebDriver {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String> {
        let endpoint = format!("{}{}", self.endpoint.as_str().trim_end_matches('/'), path);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(45))
            .build();
        let request = match method {
            "POST" => agent.post(&endpoint),
            "DELETE" => agent.delete(&endpoint),
            _ => return Err("unsupported WebDriver method".into()),
        };
        let body = serde_json::to_string(&body)
            .map_err(|_| "WebDriver request could not be serialized".to_owned())?;
        request
            .set("content-type", "application/json")
            .send_string(&body)
            .map_err(|_| "WebDriver request failed".to_owned())?
            .into_string()
            .map_err(|_| "WebDriver response was not valid JSON".to_owned())
            .and_then(|body| {
                serde_json::from_str(&body)
                    .map_err(|_| "WebDriver response was not valid JSON".to_owned())
            })
    }
}

trait PublicationExecutor: Send + Sync {
    fn publish(&self, platform: Platform, request: &PublishRequest) -> Result<String, String>;
}

/// Opens an existing attached browser at a platform page for a user-driven
/// login. It does not inspect session state or assert that login completed.
trait LoginNavigationExecutor: Send + Sync {
    fn open_manual_login(&self, platform: Platform) -> Result<(), String>;
}

#[derive(Debug)]
struct ArticleExecutionError {
    reason: String,
    automation_attempted: bool,
}

impl ArticleExecutionError {
    fn local(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: false,
        }
    }

    fn attempted(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: true,
        }
    }
}

trait ArticlePublicationExecutor: Send + Sync {
    fn publish_article(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<String, ArticleExecutionError>;
}

struct WebDriverPublisher<T> {
    transport: T,
    browser_debugger_address: SocketAddr,
    acknowledgement: AcknowledgementPolicy,
    next_job: AtomicU64,
}

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn webdriver_value(reply: Value) -> Result<Value, String> {
        reply
            .get("value")
            .cloned()
            .ok_or_else(|| "WebDriver response omitted value".into())
    }

    fn session(&self) -> Result<String, String> {
        let debugger_address = self.browser_debugger_address.to_string();
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            "/session",
            json!({"capabilities":{"alwaysMatch":{"goog:chromeOptions":{"debuggerAddress":debugger_address}}}}),
        )?)?;
        value
            .get("sessionId")
            .and_then(Value::as_str)
            .or_else(|| value.get("session_id").and_then(Value::as_str))
            .map(str::to_owned)
            .ok_or_else(|| "WebDriver did not create a session".into())
    }

    fn find_once(&self, session: &str, selector: &str) -> Option<String> {
        let reply = self
            .transport
            .request(
                "POST",
                &format!("/session/{session}/element"),
                json!({"using":"css selector","value":selector}),
            )
            .ok()?;
        let value = Self::webdriver_value(reply).ok()?;
        value
            .get(ELEMENT_KEY)
            .and_then(Value::as_str)
            .or_else(|| value.get("ELEMENT").and_then(Value::as_str))
            .map(str::to_owned)
    }

    fn find(&self, session: &str, selectors: &[&str]) -> Result<String, String> {
        for attempt in 0..ELEMENT_POLL_ATTEMPTS {
            for selector in selectors {
                if let Some(element) = self.find_once(session, selector) {
                    return Ok(element);
                }
            }
            if attempt + 1 < ELEMENT_POLL_ATTEMPTS {
                std::thread::sleep(ELEMENT_POLL_INTERVAL);
            }
        }
        Err("no supported selector matched the current platform page".into())
    }

    fn navigate(&self, session: &str, url: &str) -> Result<(), String> {
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/url"),
            json!({"url":url}),
        )?)
        .map(|_| ())
    }

    fn input(&self, session: &str, selectors: &[&str], text: &str) -> Result<(), String> {
        let element = self.find(session, selectors)?;
        Self::webdriver_value(self.transport.request("POST", &format!("/session/{session}/element/{element}/value"), json!({"text":text,"value":text.chars().map(|item| item.to_string()).collect::<Vec<_>>() }))?).map(|_| ())
    }

    fn click(&self, session: &str, selectors: &[&str]) -> Result<(), String> {
        let element = self.find(session, selectors)?;
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/element/{element}/click"),
            json!({}),
        )?)
        .map(|_| ())
    }

    fn delete_session(&self, session: &str) -> Result<(), String> {
        Self::webdriver_value(self.transport.request(
            "DELETE",
            &format!("/session/{session}"),
            json!({}),
        )?)
        .map(|_| ())
    }

    fn is_visible(&self, session: &str, element: &str) -> Result<bool, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":VISIBLE_SCRIPT,"args":[{"element-6066-11e4-a52e-4f735466cecf":element}]}),
        )?)?;
        value
            .as_bool()
            .ok_or_else(|| "WebDriver visibility script did not return a boolean".into())
    }

    fn success_marker_visible(
        &self,
        session: &str,
        profile: &PlatformProfile,
    ) -> Result<bool, String> {
        for selector in profile.success {
            if let Some(element) = self.find_once(session, selector) {
                if self.is_visible(session, &element)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn wait_for_success_transition(
        &self,
        session: &str,
        profile: &PlatformProfile,
    ) -> Result<(), String> {
        for attempt in 0..self.acknowledgement.attempts {
            if self.success_marker_visible(session, profile)? {
                return Ok(());
            }
            if attempt + 1 < self.acknowledgement.attempts {
                std::thread::sleep(self.acknowledgement.interval);
            }
        }
        Err(
            "post-click success acknowledgement did not become visibly present before deadline"
                .into(),
        )
    }

    fn article_success_marker_visible(&self, session: &str) -> Result<bool, String> {
        for selector in JUEJIN_PROFILE.success {
            if let Some(element) = self.find_once(session, selector) {
                if self.is_visible(session, &element)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn wait_for_article_success_transition(&self, session: &str) -> Result<(), String> {
        for attempt in 0..self.acknowledgement.attempts {
            if self.article_success_marker_visible(session)? {
                return Ok(());
            }
            if attempt + 1 < self.acknowledgement.attempts {
                std::thread::sleep(self.acknowledgement.interval);
            }
        }
        Err(
            "post-click article acknowledgement did not become visibly present before deadline"
                .into(),
        )
    }

    fn write_codemirror(&self, session: &str, text: &str) -> Result<(), String> {
        let element = self.find(session, JUEJIN_PROFILE.content)?;
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":CODEMIRROR_WRITE_SCRIPT,"args":[{"element-6066-11e4-a52e-4f735466cecf":element},text]}),
        )?)?;
        if value.as_bool() == Some(true) {
            Ok(())
        } else {
            Err("CodeMirror content write could not be verified".into())
        }
    }

    fn bounded_text(name: &str, value: &str, maximum: usize) -> Result<(), String> {
        if value.trim().is_empty() {
            return Err(format!("{name} must not be empty"));
        }
        if value.len() > maximum {
            return Err(format!("{name} exceeds {maximum} bytes"));
        }
        Ok(())
    }

    fn bounded_optional_text(
        name: &str,
        value: Option<&str>,
        maximum: usize,
    ) -> Result<(), String> {
        value
            .map(|value| Self::bounded_text(name, value, maximum))
            .transpose()
            .map(|_| ())
    }

    fn allowed_extension(path: &Path, allowed: &[&str]) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| allowed.contains(&extension.to_ascii_lowercase().as_str()))
    }

    fn regular_local_file(path: &Path, allowed: &[&str], maximum: u64) -> Result<(), String> {
        if !Self::allowed_extension(path, allowed) {
            return Err("local file extension is not supported".into());
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "local file could not be inspected".to_owned())?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err("local file must be a regular non-symlink file".into());
        }
        if metadata.len() == 0 {
            return Err("local file must not be empty".into());
        }
        if metadata.len() > maximum {
            return Err(format!("local file exceeds {maximum} bytes"));
        }
        Ok(())
    }

    fn article_body(request: &PublishArticleRequest) -> Result<String, String> {
        if let Some(content) = request
            .content
            .as_deref()
            .filter(|item| !item.trim().is_empty())
        {
            Self::bounded_text("article body", content, MAX_ARTICLE_BODY_BYTES)?;
            return Ok(content.to_owned());
        }
        let file = request
            .file
            .as_deref()
            .ok_or_else(|| "article content or local file is required".to_owned())?;
        Self::regular_local_file(file, ARTICLE_TEXT_EXTENSIONS, MAX_ARTICLE_BODY_BYTES as u64)?;
        let mut bytes = Vec::new();
        File::open(file)
            .map_err(|_| "article content file could not be opened".to_owned())?
            .take((MAX_ARTICLE_BODY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "article content file could not be read".to_owned())?;
        if bytes.len() > MAX_ARTICLE_BODY_BYTES {
            return Err(format!(
                "article body exceeds {MAX_ARTICLE_BODY_BYTES} bytes"
            ));
        }
        let body = String::from_utf8(bytes)
            .map_err(|_| "article content file must contain UTF-8 text".to_owned())?;
        Self::bounded_text("article body", &body, MAX_ARTICLE_BODY_BYTES)?;
        Ok(body)
    }

    fn validate_article_request(request: &PublishArticleRequest) -> Result<String, String> {
        request.validate().map_err(|error| error.to_string())?;
        if request.has_account_routing() {
            return Err("account routing is not accepted by the runner".into());
        }
        Self::bounded_text("article title", &request.title, MAX_ARTICLE_TITLE_BYTES)?;
        Self::bounded_optional_text(
            "article category",
            request.category.as_deref(),
            MAX_ARTICLE_CATEGORY_BYTES,
        )?;
        Self::bounded_optional_text(
            "article summary",
            request.summary.as_deref(),
            MAX_ARTICLE_SUMMARY_BYTES,
        )?;
        if request.tags.len() > MAX_ARTICLE_TAGS {
            return Err(format!("article tags exceed {MAX_ARTICLE_TAGS} entries"));
        }
        for tag in &request.tags {
            Self::bounded_text("article tag", tag, MAX_ARTICLE_TAG_BYTES)?;
        }
        if let Some(cover) = request.cover.as_deref() {
            if Url::parse(cover).is_ok() {
                return Err("article cover must be a local file path".into());
            }
            Self::regular_local_file(
                Path::new(cover),
                ARTICLE_COVER_EXTENSIONS,
                MAX_ARTICLE_COVER_BYTES,
            )?;
        }
        Self::article_body(request)
    }

    fn description(platform: Platform, request: &PublishRequest) -> String {
        let override_value = request
            .overrides
            .iter()
            .find(|item| item.platform == platform);
        let tags = override_value
            .and_then(|item| item.tags.as_ref())
            .unwrap_or(&request.tags);
        let mut fields = tags.iter().map(|tag| format!("#{tag}")).collect::<Vec<_>>();
        if let Some(address) = &request.address {
            fields.push(address.clone());
        }
        if let Some(statement) = override_value.and_then(|item| item.creative_statement.as_ref()) {
            fields.push(statement.clone());
        }
        fields.join(" ")
    }

    fn title(platform: Platform, request: &PublishRequest) -> &str {
        request
            .overrides
            .iter()
            .find(|item| item.platform == platform)
            .and_then(|item| item.title.as_deref())
            .unwrap_or(&request.title)
    }
}

impl<T: WebDriverTransport> LoginNavigationExecutor for WebDriverPublisher<T> {
    fn open_manual_login(&self, platform: Platform) -> Result<(), String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let session = self.session()?;
        let outcome = self.navigate(&session, profile.upload_url);
        let cleanup = self.delete_session(&session);
        outcome?;
        cleanup
    }
}

impl<T: WebDriverTransport> PublicationExecutor for WebDriverPublisher<T> {
    fn publish(&self, platform: Platform, request: &PublishRequest) -> Result<String, String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let MediaSource::LocalFile(file) = &request.source else {
            return Err("WebDriver runner accepts only local media files".into());
        };
        let file = file
            .to_str()
            .ok_or_else(|| "local media path is not valid Unicode".to_owned())?;
        let session = self.session()?;
        let outcome: Result<(), String> = (|| {
            self.navigate(&session, profile.upload_url)?;
            self.input(&session, profile.file, file)?;
            self.input(&session, profile.title, Self::title(platform, request))?;
            let description = Self::description(platform, request);
            if !description.is_empty() {
                self.input(&session, profile.description, &description)?;
            }
            if self.success_marker_visible(&session, profile)? {
                return Err(
                    "a success marker was already visibly present before the publish action".into(),
                );
            }
            self.click(
                &session,
                if request.draft {
                    profile.draft
                } else {
                    profile.submit
                },
            )?;
            self.wait_for_success_transition(&session, profile)?;
            Ok(())
        })();
        let cleanup = self.delete_session(&session);
        outcome?;
        cleanup?;
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        Ok(format!("webdriver-{}-{job}", platform.as_str()))
    }
}

impl<T: WebDriverTransport> ArticlePublicationExecutor for WebDriverPublisher<T> {
    fn publish_article(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<String, ArticleExecutionError> {
        if request
            .article_platform()
            .map_err(|error| ArticleExecutionError::local(error.to_string()))?
            != ArticlePlatform::Juejin
        {
            return Err(ArticleExecutionError::local(
                "no WebDriver profile is installed for article platform",
            ));
        }
        let body = Self::validate_article_request(request).map_err(ArticleExecutionError::local)?;
        let session = self.session().map_err(ArticleExecutionError::attempted)?;
        let outcome: Result<(), String> = (|| {
            self.navigate(&session, JUEJIN_PROFILE.editor_url)?;
            self.input(&session, JUEJIN_PROFILE.title, &request.title)?;
            self.write_codemirror(&session, &body)?;
            if let Some(cover) = request
                .cover
                .as_deref()
                .filter(|item| !item.trim().is_empty())
            {
                self.input(&session, JUEJIN_PROFILE.cover, cover)?;
            }
            if let Some(category) = request
                .category
                .as_deref()
                .filter(|item| !item.trim().is_empty())
            {
                self.input(&session, JUEJIN_PROFILE.category, category)?;
            }
            if !request.tags.is_empty() {
                self.input(&session, JUEJIN_PROFILE.tags, &request.tags.join(","))?;
            }
            if let Some(summary) = request
                .summary
                .as_deref()
                .filter(|item| !item.trim().is_empty())
            {
                self.input(&session, JUEJIN_PROFILE.summary, summary)?;
            }
            if self.article_success_marker_visible(&session)? {
                return Err(
                    "an article success marker was already visibly present before confirmation"
                        .into(),
                );
            }
            self.click(&session, JUEJIN_PROFILE.publish_panel)?;
            self.click(&session, JUEJIN_PROFILE.confirm)?;
            self.wait_for_article_success_transition(&session)
        })();
        let cleanup = self.delete_session(&session);
        outcome.map_err(ArticleExecutionError::attempted)?;
        cleanup.map_err(ArticleExecutionError::attempted)?;
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        Ok(format!("webdriver-juejin-{job}"))
    }
}

struct RemoteMediaSupport {
    policy: MediaStagingPolicy,
    stager: Arc<dyn RemoteMediaStager>,
}

impl RemoteMediaSupport {
    fn configured(directory: PathBuf) -> Self {
        Self {
            policy: MediaStagingPolicy {
                max_bytes: MAX_REMOTE_VIDEO_BYTES,
                allowed_content_types: REMOTE_VIDEO_CONTENT_TYPES
                    .iter()
                    .map(|item| (*item).to_owned())
                    .collect(),
            },
            stager: Arc::new(HttpRemoteMediaStager::new(directory)),
        }
    }

    fn stage(&self, source: &MediaSource) -> Result<Box<dyn StagedMedia>, String> {
        let MediaSource::RemoteUrl(url) = source else {
            return Err("remote media staging requires an HTTP(S) media URL".into());
        };
        let request = RemoteMediaRequest::new(url.clone(), &self.policy)
            .map_err(|_| "remote media URL is not supported".to_owned())?;
        self.stager
            .stage(&request, &self.policy)
            // Transport errors may contain the submitted URL. The local runner
            // must not reflect it through the provider response.
            .map_err(|_| "remote media staging failed".to_owned())
    }
}

struct RunnerService {
    executor: Option<Arc<dyn PublicationExecutor>>,
    login_executor: Option<Arc<dyn LoginNavigationExecutor>>,
    article_executor: Option<Arc<dyn ArticlePublicationExecutor>>,
    remote_media: Option<RemoteMediaSupport>,
    browser_debugger_address: Option<SocketAddr>,
    debugger_probe: Arc<dyn BrowserDebuggerProbe>,
}

trait BrowserDebuggerProbe: Send + Sync {
    fn is_ready(&self, address: SocketAddr) -> bool;
}

struct HttpBrowserDebuggerProbe;

impl BrowserDebuggerProbe for HttpBrowserDebuggerProbe {
    fn is_ready(&self, address: SocketAddr) -> bool {
        if !address.ip().is_loopback() {
            return false;
        }
        let endpoint = format!("http://{address}/json/version");
        ureq::AgentBuilder::new()
            .timeout(DEBUGGER_PROBE_TIMEOUT)
            .build()
            .get(&endpoint)
            .call()
            .ok()
            .and_then(|response| response.into_string().ok())
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .is_some_and(|response| valid_chrome_devtools_version(&response))
    }
}

fn valid_chrome_devtools_version(response: &Value) -> bool {
    response
        .get("Browser")
        .and_then(Value::as_str)
        .is_some_and(|browser| browser.starts_with("Chrome/"))
        && response
            .get("Protocol-Version")
            .and_then(Value::as_str)
            .is_some_and(|version| !version.is_empty())
        && response
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .and_then(|url| Url::parse(url).ok())
            .is_some_and(|url| matches!(url.scheme(), "ws" | "wss"))
}

async fn health(State(state): State<Arc<RunnerService>>) -> impl IntoResponse {
    let browser_debugger_configured = state.browser_debugger_address.is_some();
    let attached_browser = match (state.executor.is_some(), state.browser_debugger_address) {
        (true, Some(address)) => {
            let probe = Arc::clone(&state.debugger_probe);
            tokio::task::spawn_blocking(move || probe.is_ready(address))
                .await
                .unwrap_or(false)
        }
        _ => false,
    };
    Json(
        json!({"ok":true,"service":"matrixpost-webdriver-runner","protocol_version":PROVIDER_RUNNER_PROTOCOL_VERSION,"browser_debugger_configured":browser_debugger_configured,"attached_browser":attached_browser}),
    )
}

async fn publish(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<ProviderRunnerRequest>,
) -> impl IntoResponse {
    if body.version != PROVIDER_RUNNER_PROTOCOL_VERSION
        || !body.request.targets.contains(&body.platform)
        || body.request.validate().is_err()
        || body.request.has_account_routing()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "invalid version, platform, or publish request".into(),
            }),
        );
    }
    let remote_media_requested = matches!(&body.request.source, MediaSource::RemoteUrl(_));
    let response = match &state.executor {
        None => ProviderRunnerResponse::Unavailable {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
            reason: "browser debugger address is not configured; no browser session was started"
                .into(),
        },
        Some(executor) => match publish_with_staged_media(
            executor.as_ref(),
            state.remote_media.as_ref(),
            body.platform,
            &body.request,
        ) {
            Ok(job_id) if !job_id.trim().is_empty() => ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                job_id,
            },
            Ok(_) => ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: if remote_media_requested {
                    REMOTE_MEDIA_EXECUTION_REJECTION.into()
                } else {
                    "runner completed without a valid job identifier".into()
                },
            },
            Err(reason) => ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: if remote_media_requested {
                    REMOTE_MEDIA_EXECUTION_REJECTION.into()
                } else {
                    reason
                },
            },
        },
    };
    (StatusCode::OK, Json(response))
}

fn publish_with_staged_media(
    executor: &dyn PublicationExecutor,
    remote_media: Option<&RemoteMediaSupport>,
    platform: Platform,
    request: &PublishRequest,
) -> Result<String, String> {
    let MediaSource::RemoteUrl(_) = &request.source else {
        return executor.publish(platform, request);
    };
    let support = remote_media.ok_or_else(|| {
        "remote media staging is disabled; start the runner with --remote-media-dir".to_owned()
    })?;
    // Stage before invoking the executor, which in turn is the only code path
    // allowed to create a WebDriver session.
    let staged = support.stage(&request.source)?;
    let mut local_request = request.clone();
    local_request.source = MediaSource::LocalFile(staged.path().to_path_buf());
    let publish = executor.publish(platform, &local_request);
    let cleanup = staged
        .cleanup()
        .map_err(|_| "staged remote media cleanup failed".to_owned());
    match (publish, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(job_id), Ok(())) => Ok(job_id),
    }
}

async fn publish_article(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<ArticleRunnerRequest>,
) -> impl IntoResponse {
    let platform = body
        .request
        .article_platform()
        .unwrap_or(ArticlePlatform::Juejin);
    if body.version != ARTICLE_RUNNER_PROTOCOL_VERSION
        || body.request.validate().is_err()
        || body.request.has_account_routing()
        || body.request.scheduled_at.is_some()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform,
                reason: "invalid version, Juejin article request, or unsupported article schedule"
                    .into(),
                automation_attempted: false,
            }),
        );
    }
    let response = match &state.article_executor {
        None => ArticleRunnerResponse::Unavailable {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            reason: "browser debugger address is not configured; no browser session was started"
                .into(),
            automation_attempted: false,
        },
        Some(executor) => match executor.publish_article(&body.request) {
            Ok(job_id) if !job_id.trim().is_empty() => ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id,
                automation_attempted: true,
            },
            Ok(_) => ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                reason: "runner completed without a valid job identifier".into(),
                automation_attempted: true,
            },
            Err(error) => ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                reason: error.reason,
                automation_attempted: error.automation_attempted,
            },
        },
    };
    (StatusCode::OK, Json(response))
}

async fn login(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<LoginRunnerRequest>,
) -> impl IntoResponse {
    if body.version != LOGIN_RUNNER_PROTOCOL_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginRunnerResponse::Rejected {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "invalid manual login request version".into(),
            }),
        );
    }
    let response = match &state.login_executor {
        None => LoginRunnerResponse::Unavailable {
            version: LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
            reason: "manual login navigation is not enabled or no browser session is attached"
                .into(),
        },
        Some(executor) => match executor.open_manual_login(body.platform) {
            Ok(()) => LoginRunnerResponse::Opened {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                manual_login_required: true,
            },
            Err(_) => LoginRunnerResponse::Rejected {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "manual login navigation could not be completed".into(),
            },
        },
    };
    (StatusCode::OK, Json(response))
}

fn app(service: Arc<RunnerService>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/publish", post(publish))
        .route("/v1/publish-article", post(publish_article))
        .route("/v1/login", post(login))
        .with_state(service)
}

#[derive(Parser)]
#[command(
    name = "matrixpost-webdriver-runner",
    version,
    about = "Loopback-only MatriXpost WebDriver runner"
)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:39001")]
    bind: SocketAddr,
    #[arg(long)]
    webdriver_endpoint: Option<String>,
    /// Loopback Chrome remote-debugging address to attach through ChromeDriver.
    #[arg(long)]
    browser_debugger_address: Option<SocketAddr>,
    /// Permit the irreversible article publish confirmation endpoint.
    #[arg(long)]
    allow_article_publish: bool,
    /// Permit navigating an already-attached browser to a platform page for a
    /// user to complete login manually. This never reads or exports login data.
    #[arg(long)]
    allow_login_navigation: bool,
    /// Absolute directory used only for bounded HTTP(S) video staging before
    /// WebDriver upload. Remote media is rejected unless this is configured.
    #[arg(long, value_name = "ABSOLUTE_PATH")]
    remote_media_dir: Option<PathBuf>,
}

fn build_remote_media_support(
    directory: Option<PathBuf>,
) -> Result<Option<RemoteMediaSupport>, String> {
    match directory {
        None => Ok(None),
        Some(directory) if directory.is_absolute() => {
            Ok(Some(RemoteMediaSupport::configured(directory)))
        }
        Some(_) => Err("--remote-media-dir must be an absolute path".into()),
    }
}

fn build_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
) -> Result<Option<Arc<dyn PublicationExecutor>>, String> {
    match (endpoint, debugger_address) {
        (Some(endpoint), Some(address)) if address.ip().is_loopback() => {
            Ok(Some(Arc::new(WebDriverPublisher {
                transport: HttpWebDriver { endpoint },
                browser_debugger_address: address,
                acknowledgement: AcknowledgementPolicy::production(),
                next_job: AtomicU64::new(1),
            })))
        }
        (Some(_), Some(_)) => Err("browser debugger address must be loopback".into()),
        (None, Some(_)) => {
            Err("--webdriver-endpoint is required when --browser-debugger-address is set".into())
        }
        (_, None) => Ok(None),
    }
}

fn build_article_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_article_publish: bool,
) -> Result<Option<Arc<dyn ArticlePublicationExecutor>>, String> {
    if !allow_article_publish {
        return Ok(None);
    }
    match (endpoint, debugger_address) {
        (Some(endpoint), Some(address)) if address.ip().is_loopback() => {
            Ok(Some(Arc::new(WebDriverPublisher {
                transport: HttpWebDriver { endpoint },
                browser_debugger_address: address,
                acknowledgement: AcknowledgementPolicy::production(),
                next_job: AtomicU64::new(1),
            })))
        }
        (Some(_), Some(_)) => Err("browser debugger address must be loopback".into()),
        (None, Some(_)) => {
            Err("--webdriver-endpoint is required when --browser-debugger-address is set".into())
        }
        (_, None) => Ok(None),
    }
}

fn build_login_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_login_navigation: bool,
) -> Result<Option<Arc<dyn LoginNavigationExecutor>>, String> {
    if !allow_login_navigation {
        return Ok(None);
    }
    match (endpoint, debugger_address) {
        (Some(endpoint), Some(address)) if address.ip().is_loopback() => {
            Ok(Some(Arc::new(WebDriverPublisher {
                transport: HttpWebDriver { endpoint },
                browser_debugger_address: address,
                acknowledgement: AcknowledgementPolicy::production(),
                next_job: AtomicU64::new(1),
            })))
        }
        (Some(_), Some(_)) => Err("browser debugger address must be loopback".into()),
        (None, Some(_)) => {
            Err("--webdriver-endpoint is required when --browser-debugger-address is set".into())
        }
        (_, None) => Ok(None),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    if !args.bind.ip().is_loopback() {
        eprintln!("matrixpost-webdriver-runner bind must be loopback");
        return ExitCode::from(2);
    }
    let endpoint = match args.webdriver_endpoint {
        Some(endpoint) => match local_webdriver_endpoint(&endpoint) {
            Ok(endpoint) => Some(endpoint),
            Err(error) => {
                eprintln!("matrixpost-webdriver-runner configuration error: {error}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let executor = match build_executor(endpoint.clone(), args.browser_debugger_address) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let article_executor = match build_article_executor(
        endpoint.clone(),
        args.browser_debugger_address,
        args.allow_article_publish,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let login_executor = match build_login_executor(
        endpoint,
        args.browser_debugger_address,
        args.allow_login_navigation,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let remote_media = match build_remote_media_support(args.remote_media_dir) {
        Ok(support) => support,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let service = Arc::new(RunnerService {
        executor,
        login_executor,
        article_executor,
        remote_media,
        browser_debugger_address: args.browser_debugger_address,
        debugger_probe: Arc::new(HttpBrowserDebuggerProbe),
    });
    let listener = match tokio::net::TcpListener::bind(args.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner failed to bind: {error}");
            return ExitCode::from(4);
        }
    };
    match axum::serve(listener, app(service)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner stopped unexpectedly: {error}");
            ExitCode::from(4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::collections::VecDeque;
    use std::sync::{Mutex, atomic::AtomicBool};
    use tower::ServiceExt;

    struct ProfileFixture {
        platform: Platform,
        success_selector: &'static str,
    }
    const PROFILE_FIXTURES: &[ProfileFixture] = &[
        ProfileFixture {
            platform: Platform::Douyin,
            success_selector: "[data-e2e='publish-success']",
        },
        ProfileFixture {
            platform: Platform::WechatChannels,
            success_selector: "[data-status='published']",
        },
        ProfileFixture {
            platform: Platform::Bilibili,
            success_selector: ".success-wrap",
        },
        ProfileFixture {
            platform: Platform::Baijiahao,
            success_selector: "[data-status='published']",
        },
        ProfileFixture {
            platform: Platform::Toutiao,
            success_selector: ".publish-success",
        },
        ProfileFixture {
            platform: Platform::Kuaishou,
            success_selector: ".publish-result-success",
        },
        ProfileFixture {
            platform: Platform::Xiaohongshu,
            success_selector: ".publish-success",
        },
        ProfileFixture {
            platform: Platform::FanqieVideo,
            success_selector: ".publish-success",
        },
    ];

    struct MockWebDriver {
        replies: Mutex<VecDeque<Result<Value, String>>>,
        paths: Mutex<Vec<String>>,
        bodies: Mutex<Vec<Value>>,
    }
    impl MockWebDriver {
        fn new(replies: Vec<Result<Value, String>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                paths: Mutex::new(Vec::new()),
                bodies: Mutex::new(Vec::new()),
            }
        }
    }
    impl WebDriverTransport for MockWebDriver {
        fn request(&self, _: &str, path: &str, body: Value) -> Result<Value, String> {
            self.paths.lock().unwrap().push(path.into());
            self.bodies.lock().unwrap().push(body);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected HTTP request".into()))
        }
    }
    fn value(value: Value) -> Result<Value, String> {
        Ok(json!({"value":value}))
    }
    fn request() -> PublishRequest {
        PublishRequest {
            source: MediaSource::LocalFile("movie.mp4".into()),
            title: "Title".into(),
            short_title: None,
            tags: vec!["tag".into()],
            address: None,
            draft: false,
            bt2: None,
            scheduled_at: None,
            task_name: None,
            account: Default::default(),
            wechat_link: Default::default(),
            overrides: Vec::new(),
            targets: vec![Platform::Douyin],
        }
    }
    fn article_request() -> PublishArticleRequest {
        PublishArticleRequest {
            platform: "juejin".into(),
            account: Default::default(),
            title: "Article title".into(),
            content: Some("# Article body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        }
    }
    fn temporary_article_path(extension: &str) -> std::path::PathBuf {
        static NEXT_TEST_FILE: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "matrixpost-article-runner-{}-{}.{}",
            std::process::id(),
            NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed),
            extension
        ))
    }
    fn element(id: &str) -> Result<Value, String> {
        value(json!({ELEMENT_KEY:id}))
    }
    fn debugger_address() -> SocketAddr {
        "127.0.0.1:9222".parse().unwrap()
    }
    fn test_publisher(mock: MockWebDriver) -> WebDriverPublisher<MockWebDriver> {
        WebDriverPublisher {
            transport: mock,
            browser_debugger_address: debugger_address(),
            acknowledgement: AcknowledgementPolicy {
                attempts: 2,
                interval: Duration::ZERO,
            },
            next_job: AtomicU64::new(1),
        }
    }

    struct StaticBrowserDebuggerProbe(bool);

    impl BrowserDebuggerProbe for StaticBrowserDebuggerProbe {
        fn is_ready(&self, _: SocketAddr) -> bool {
            self.0
        }
    }

    fn runner_service(
        executor: Option<Arc<dyn PublicationExecutor>>,
        article_executor: Option<Arc<dyn ArticlePublicationExecutor>>,
    ) -> RunnerService {
        RunnerService {
            executor,
            login_executor: None,
            article_executor,
            remote_media: None,
            browser_debugger_address: None,
            debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
        }
    }

    fn runner_service_with_login(
        login_executor: Option<Arc<dyn LoginNavigationExecutor>>,
    ) -> RunnerService {
        RunnerService {
            executor: None,
            login_executor,
            article_executor: None,
            remote_media: None,
            browser_debugger_address: None,
            debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
        }
    }

    fn runner_service_with_probe(
        executor: Option<Arc<dyn PublicationExecutor>>,
        browser_debugger_address: Option<SocketAddr>,
        probe_ready: bool,
    ) -> RunnerService {
        RunnerService {
            executor,
            login_executor: None,
            article_executor: None,
            remote_media: None,
            browser_debugger_address,
            debugger_probe: Arc::new(StaticBrowserDebuggerProbe(probe_ready)),
        }
    }

    async fn health_status(service: RunnerService) -> Value {
        let response = app(Arc::new(service))
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn manual_login_protocol_rejects_invalid_versions_and_unknown_fields() {
        let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
            AcceptedLogin,
        )))));
        let invalid_version = json!({
            "version": LOGIN_RUNNER_PROTOCOL_VERSION + 1,
            "platform": "dy"
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/login")
                    .header("content-type", "application/json")
                    .body(Body::from(invalid_version.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let unknown = json!({
            "version": LOGIN_RUNNER_PROTOCOL_VERSION,
            "platform": "dy",
            "cookie": "forbidden"
        });
        let response = router
            .oneshot(
                Request::post("/v1/login")
                    .header("content-type", "application/json")
                    .body(Body::from(unknown.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn manual_login_protocol_is_unavailable_without_explicit_executor() {
        let router = app(Arc::new(runner_service_with_login(None)));
        let response = router
            .oneshot(
                Request::post("/v1/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LoginRunnerRequest {
                            version: LOGIN_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(matches!(
            serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap(),
            LoginRunnerResponse::Unavailable { .. }
        ));
    }

    #[tokio::test]
    async fn manual_login_protocol_opens_only_the_manual_login_page() {
        let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
            AcceptedLogin,
        )))));
        let response = router
            .oneshot(
                Request::post("/v1/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LoginRunnerRequest {
                            version: LOGIN_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap(),
            LoginRunnerResponse::Opened {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Douyin,
                manual_login_required: true,
            }
        );
    }

    #[tokio::test]
    async fn manual_login_protocol_rejects_executor_failures_without_exposing_them() {
        let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
            FailingLogin,
        )))));
        let response = router
            .oneshot(
                Request::post("/v1/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LoginRunnerRequest {
                            version: LOGIN_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response = serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap();
        assert!(matches!(response, LoginRunnerResponse::Rejected { .. }));
        assert!(
            !serde_json::to_string(&response)
                .unwrap()
                .contains("raw webdriver failure")
        );
    }

    #[tokio::test]
    async fn health_reports_configured_but_unreachable_browser_as_detached() {
        let status = health_status(runner_service_with_probe(
            Some(Arc::new(Accepted)),
            Some(debugger_address()),
            false,
        ))
        .await;

        assert_eq!(status["browser_debugger_configured"], true);
        assert_eq!(status["attached_browser"], false);
    }

    #[tokio::test]
    async fn health_reports_ready_configured_browser_as_attached() {
        let status = health_status(runner_service_with_probe(
            Some(Arc::new(Accepted)),
            Some(debugger_address()),
            true,
        ))
        .await;

        assert_eq!(status["browser_debugger_configured"], true);
        assert_eq!(status["attached_browser"], true);
    }

    #[tokio::test]
    async fn health_without_browser_debugger_address_is_detached() {
        let status = health_status(runner_service_with_probe(
            Some(Arc::new(Accepted)),
            None,
            true,
        ))
        .await;

        assert_eq!(status["browser_debugger_configured"], false);
        assert_eq!(status["attached_browser"], false);
    }

    #[test]
    fn devtools_version_probe_requires_chrome_protocol_evidence() {
        assert!(valid_chrome_devtools_version(&json!({
            "Browser": "Chrome/150.0.0.0",
            "Protocol-Version": "1.3",
            "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/browser/test"
        })));
        assert!(!valid_chrome_devtools_version(&json!({
            "Browser": "Chrome/150.0.0.0",
            "Protocol-Version": "1.3"
        })));
    }

    #[test]
    fn profiles_cover_the_exact_upstream_platform_set_with_ordered_fallbacks() {
        assert_eq!(PROFILES.len(), Platform::ALL.len());
        assert_eq!(PROFILE_FIXTURES.len(), Platform::ALL.len());
        for platform in Platform::ALL {
            let profile = profile(platform).unwrap();
            assert!(profile.upload_url.starts_with("https://"));
            assert!(
                profile.file.len() >= 2
                    && profile.title.len() >= 2
                    && profile.description.len() >= 2
                    && profile.submit.len() >= 2
                    && profile.draft.len() >= 2
                    && profile.success.len() >= 2
            );
            let fixture = PROFILE_FIXTURES
                .iter()
                .find(|fixture| fixture.platform == platform)
                .unwrap();
            assert!(profile.success.contains(&fixture.success_selector));
        }
    }
    #[test]
    fn webdriver_protocol_runs_each_phase_and_closes_the_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            Err("not visible before action".into()),
            Err("not visible before action".into()),
            element("submit"),
            value(json!(null)),
            element("success"),
            value(json!(true)),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert_eq!(
            publisher.publish(Platform::Douyin, &request()).unwrap(),
            "webdriver-dy-1"
        );
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .ends_with("/session/s")
        );
        assert_eq!(
            publisher.transport.bodies.lock().unwrap()[0],
            json!({"capabilities":{"alwaysMatch":{"goog:chromeOptions":{"debuggerAddress":"127.0.0.1:9222"}}}})
        );
    }
    #[test]
    fn missing_selector_fails_closed_and_still_closes_the_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            Err("not found".into()),
            Err("not found".into()),
            Err("not found".into()),
            Err("not found".into()),
            Err("not found".into()),
            Err("not found".into()),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert!(publisher.publish(Platform::Douyin, &request()).is_err());
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .ends_with("/session/s")
        );
    }
    #[test]
    fn webdriver_endpoint_rejects_remote_credentials_and_profile_paths() {
        for value in [
            "https://127.0.0.1:9515",
            "http://192.0.2.1:9515",
            "http://user:pass@127.0.0.1:9515",
            "http://127.0.0.1:9515/profile",
        ] {
            assert!(local_webdriver_endpoint(value).is_err(), "{value}");
        }
        assert!(local_webdriver_endpoint("http://127.0.0.1:9515/wd/hub").is_ok());
        assert!(local_webdriver_endpoint("http://[::1]:9515/wd/hub").is_ok());
    }
    #[test]
    fn success_timeout_is_rejected_and_session_is_closed() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            Err("not visible before action".into()),
            Err("not visible before action".into()),
            element("submit"),
            value(json!(null)),
            Err("not ready".into()),
            Err("not ready".into()),
            Err("not ready".into()),
            Err("not ready".into()),
            Err("not ready".into()),
            Err("not ready".into()),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert!(publisher.publish(Platform::Douyin, &request()).is_err());
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .ends_with("/session/s")
        );
    }
    #[test]
    fn hidden_success_marker_never_acknowledges_and_cleanup_runs() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            element("pre-hidden"),
            value(json!(false)),
            Err("not found".into()),
            element("submit"),
            value(json!(null)),
            element("post-hidden"),
            value(json!(false)),
            Err("not found".into()),
            element("post-hidden"),
            value(json!(false)),
            Err("not found".into()),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert!(publisher.publish(Platform::Douyin, &request()).is_err());
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .ends_with("/session/s")
        );
    }
    #[test]
    fn preexisting_visible_success_marker_rejects_before_click_and_cleans_up() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            element("already-successful"),
            value(json!(true)),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert!(publisher.publish(Platform::Douyin, &request()).is_err());
        let paths = publisher.transport.paths.lock().unwrap();
        assert!(!paths.iter().any(|path| path.ends_with("/click")));
        assert!(paths.last().unwrap().ends_with("/session/s"));
    }
    #[test]
    fn debugger_address_must_be_loopback() {
        let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
        assert!(
            build_executor(Some(endpoint.clone()), Some(debugger_address()))
                .unwrap()
                .is_some()
        );
        assert!(build_executor(Some(endpoint), Some("192.0.2.1:9222".parse().unwrap())).is_err());
    }
    #[test]
    fn article_executor_requires_explicit_startup_opt_in() {
        let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
        assert!(
            build_article_executor(Some(endpoint.clone()), Some(debugger_address()), false)
                .unwrap()
                .is_none()
        );
        assert!(
            build_article_executor(Some(endpoint), Some(debugger_address()), true)
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn login_executor_requires_explicit_startup_opt_in() {
        let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
        assert!(
            build_login_executor(Some(endpoint.clone()), Some(debugger_address()), false)
                .unwrap()
                .is_none()
        );
        assert!(
            build_login_executor(Some(endpoint), Some(debugger_address()), true)
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn manual_login_navigation_closes_the_temporary_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"login-session"})),
            value(json!(null)),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        publisher.open_manual_login(Platform::Douyin).unwrap();
        let paths = publisher.transport.paths.lock().unwrap();
        assert_eq!(paths[1], "/session/login-session/url");
        assert!(paths.last().unwrap().ends_with("/session/login-session"));
    }
    #[test]
    fn manual_login_navigation_failure_still_closes_the_temporary_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"login-session"})),
            Err("navigation failed".into()),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        assert!(publisher.open_manual_login(Platform::Douyin).is_err());
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .is_some_and(|path| path.ends_with("/session/login-session"))
        );
    }
    struct Accepted;
    impl PublicationExecutor for Accepted {
        fn publish(&self, _: Platform, _: &PublishRequest) -> Result<String, String> {
            Ok("job-1".into())
        }
    }
    impl ArticlePublicationExecutor for Accepted {
        fn publish_article(
            &self,
            _: &PublishArticleRequest,
        ) -> Result<String, ArticleExecutionError> {
            Ok("article-job-1".into())
        }
    }

    struct RecordingPublicationExecutor {
        calls: AtomicU64,
        local_paths: Mutex<Vec<PathBuf>>,
        fail: bool,
    }

    impl RecordingPublicationExecutor {
        fn new(fail: bool) -> Self {
            Self {
                calls: AtomicU64::new(0),
                local_paths: Mutex::new(Vec::new()),
                fail,
            }
        }
    }

    impl PublicationExecutor for RecordingPublicationExecutor {
        fn publish(&self, _: Platform, request: &PublishRequest) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let MediaSource::LocalFile(path) = &request.source else {
                return Err("remote media reached WebDriver executor".into());
            };
            self.local_paths.lock().unwrap().push(path.clone());
            if self.fail {
                Err("mock WebDriver upload failure".into())
            } else {
                Ok("job-1".into())
            }
        }
    }

    struct SentinelFailureExecutor;

    impl PublicationExecutor for SentinelFailureExecutor {
        fn publish(&self, _: Platform, _: &PublishRequest) -> Result<String, String> {
            Err(
                "webdriver failed for https://example.invalid/video.mp4 at /private/staging/video.mp4"
                    .into(),
            )
        }
    }

    struct TestStagedMedia {
        path: PathBuf,
        cleanup_attempted: Arc<AtomicBool>,
        cleanup_fails: bool,
    }

    impl StagedMedia for TestStagedMedia {
        fn path(&self) -> &Path {
            &self.path
        }

        fn cleanup(self: Box<Self>) -> Result<(), matrixpost_core::DomainError> {
            self.cleanup_attempted.store(true, Ordering::Relaxed);
            if self.cleanup_fails {
                Err(matrixpost_core::DomainError::RemoteMedia(
                    "mock cleanup failure".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    struct TestRemoteMediaStager {
        path: PathBuf,
        stage_calls: AtomicU64,
        cleanup_attempted: Arc<AtomicBool>,
        stage_fails: bool,
        cleanup_fails: bool,
    }

    impl TestRemoteMediaStager {
        fn succeeding(path: PathBuf) -> Self {
            Self {
                path,
                stage_calls: AtomicU64::new(0),
                cleanup_attempted: Arc::new(AtomicBool::new(false)),
                stage_fails: false,
                cleanup_fails: false,
            }
        }
    }

    impl RemoteMediaStager for TestRemoteMediaStager {
        fn stage(
            &self,
            _: &RemoteMediaRequest,
            _: &dyn matrixpost_core::RemoteMediaPolicy,
        ) -> Result<Box<dyn StagedMedia>, matrixpost_core::DomainError> {
            self.stage_calls.fetch_add(1, Ordering::Relaxed);
            if self.stage_fails {
                return Err(matrixpost_core::DomainError::RemoteMedia(
                    "raw remote URL must not escape".into(),
                ));
            }
            Ok(Box::new(TestStagedMedia {
                path: self.path.clone(),
                cleanup_attempted: Arc::clone(&self.cleanup_attempted),
                cleanup_fails: self.cleanup_fails,
            }))
        }
    }

    fn test_remote_media_support(stager: Arc<dyn RemoteMediaStager>) -> RemoteMediaSupport {
        RemoteMediaSupport {
            policy: MediaStagingPolicy {
                max_bytes: MAX_REMOTE_VIDEO_BYTES,
                allowed_content_types: REMOTE_VIDEO_CONTENT_TYPES
                    .iter()
                    .map(|item| (*item).to_owned())
                    .collect(),
            },
            stager,
        }
    }

    struct AcceptedLogin;

    impl LoginNavigationExecutor for AcceptedLogin {
        fn open_manual_login(&self, _: Platform) -> Result<(), String> {
            Ok(())
        }
    }

    struct FailingLogin;

    impl LoginNavigationExecutor for FailingLogin {
        fn open_manual_login(&self, _: Platform) -> Result<(), String> {
            Err("raw webdriver failure".into())
        }
    }

    struct FailingArticleExecutor;

    impl ArticlePublicationExecutor for FailingArticleExecutor {
        fn publish_article(
            &self,
            _: &PublishArticleRequest,
        ) -> Result<String, ArticleExecutionError> {
            Err(ArticleExecutionError::attempted("mock automation failure"))
        }
    }

    struct LocalValidationArticleExecutor;

    impl ArticlePublicationExecutor for LocalValidationArticleExecutor {
        fn publish_article(
            &self,
            _: &PublishArticleRequest,
        ) -> Result<String, ArticleExecutionError> {
            Err(ArticleExecutionError::local(
                "mock local validation failure",
            ))
        }
    }

    struct CountingArticleExecutor(AtomicU64);

    impl ArticlePublicationExecutor for CountingArticleExecutor {
        fn publish_article(
            &self,
            _: &PublishArticleRequest,
        ) -> Result<String, ArticleExecutionError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok("article-job-1".into())
        }
    }
    #[tokio::test]
    async fn protocol_accepts_only_versioned_targeted_requests() {
        let router = app(Arc::new(runner_service(Some(Arc::new(Accepted)), None)));
        let runner_request = ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            request: request(),
        };
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&runner_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap(),
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Douyin,
                job_id: "job-1".into()
            }
        );
        let mut invalid = serde_json::to_value(runner_request).unwrap();
        invalid["version"] = json!(999);
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let mut routed = request();
        routed.account.phone = Some("runner-forbidden".into());
        let response = router
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ProviderRunnerRequest {
                            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                            request: routed,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn no_debugger_address_returns_unavailable_without_starting_a_session() {
        let router = app(Arc::new(runner_service(None, None)));
        let request = ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            request: request(),
        };
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(matches!(
            serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap(),
            ProviderRunnerResponse::Unavailable { .. }
        ));
    }

    fn remote_request() -> PublishRequest {
        let mut value = request();
        value.source =
            MediaSource::RemoteUrl(Url::parse("https://media.example.invalid/movie.mp4").unwrap());
        value
    }

    #[test]
    fn remote_media_directory_must_be_explicit_and_absolute() {
        assert!(build_remote_media_support(None).unwrap().is_none());
        assert!(build_remote_media_support(Some(PathBuf::from("relative-staging"))).is_err());
        assert!(
            build_remote_media_support(Some(PathBuf::from("/explicit/staging")))
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn remote_media_without_configured_directory_rejects_before_webdriver_session() {
        let executor = Arc::new(RecordingPublicationExecutor::new(false));
        let service = RunnerService {
            executor: Some(executor.clone()),
            login_executor: None,
            article_executor: None,
            remote_media: None,
            browser_debugger_address: None,
            debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
        };
        let response = app(Arc::new(service))
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ProviderRunnerRequest {
                            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                            request: remote_request(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response = serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap();
        assert!(matches!(response, ProviderRunnerResponse::Rejected { .. }));
        assert_eq!(executor.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn remote_media_http_rejection_never_reflects_url_or_staged_path() {
        let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
            "/private/staging/video.mp4",
        )));
        let service = RunnerService {
            executor: Some(Arc::new(SentinelFailureExecutor)),
            login_executor: None,
            article_executor: None,
            remote_media: Some(test_remote_media_support(stager)),
            browser_debugger_address: None,
            debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
        };
        let mut request = request();
        request.source =
            MediaSource::RemoteUrl(Url::parse("https://example.invalid/video.mp4").unwrap());
        let response = app(Arc::new(service))
            .oneshot(
                Request::post("/v1/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ProviderRunnerRequest {
                            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                            platform: Platform::Douyin,
                            request,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let serialized = String::from_utf8(body.to_vec()).unwrap();
        assert!(!serialized.contains("https://example.invalid/video.mp4"));
        assert!(!serialized.contains("/private/staging/video.mp4"));
        assert_eq!(
            serde_json::from_str::<ProviderRunnerResponse>(&serialized).unwrap(),
            ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Douyin,
                reason: REMOTE_MEDIA_EXECUTION_REJECTION.into(),
            }
        );
    }

    #[test]
    fn configured_remote_media_stages_a_local_path_and_cleans_it_after_webdriver_outcome() {
        let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
            "/explicit/staging/movie.mp4",
        )));
        let executor = RecordingPublicationExecutor::new(false);
        let result = publish_with_staged_media(
            &executor,
            Some(&test_remote_media_support(stager.clone())),
            Platform::Douyin,
            &remote_request(),
        );
        assert_eq!(result.unwrap(), "job-1");
        assert_eq!(stager.stage_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            executor.local_paths.lock().unwrap().as_slice(),
            [PathBuf::from("/explicit/staging/movie.mp4")]
        );
        assert!(stager.cleanup_attempted.load(Ordering::Relaxed));

        let failed_executor = RecordingPublicationExecutor::new(true);
        let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
            "/explicit/staging/failing-movie.mp4",
        )));
        assert!(
            publish_with_staged_media(
                &failed_executor,
                Some(&test_remote_media_support(stager.clone())),
                Platform::Douyin,
                &remote_request(),
            )
            .is_err()
        );
        assert_eq!(failed_executor.calls.load(Ordering::Relaxed), 1);
        assert!(stager.cleanup_attempted.load(Ordering::Relaxed));
    }

    #[test]
    fn remote_staging_fails_closed_before_webdriver_and_cleanup_failure_is_rejected() {
        let failing_stager = Arc::new(TestRemoteMediaStager {
            path: PathBuf::from("/explicit/staging/never-uploaded.mp4"),
            stage_calls: AtomicU64::new(0),
            cleanup_attempted: Arc::new(AtomicBool::new(false)),
            stage_fails: true,
            cleanup_fails: false,
        });
        let executor = RecordingPublicationExecutor::new(false);
        let error = publish_with_staged_media(
            &executor,
            Some(&test_remote_media_support(failing_stager.clone())),
            Platform::Douyin,
            &remote_request(),
        )
        .unwrap_err();
        assert_eq!(error, "remote media staging failed");
        assert_eq!(failing_stager.stage_calls.load(Ordering::Relaxed), 1);
        assert_eq!(executor.calls.load(Ordering::Relaxed), 0);

        let cleanup_failure = Arc::new(TestRemoteMediaStager {
            path: PathBuf::from("/explicit/staging/cleanup-failure.mp4"),
            stage_calls: AtomicU64::new(0),
            cleanup_attempted: Arc::new(AtomicBool::new(false)),
            stage_fails: false,
            cleanup_fails: true,
        });
        let error = publish_with_staged_media(
            &executor,
            Some(&test_remote_media_support(cleanup_failure.clone())),
            Platform::Douyin,
            &remote_request(),
        )
        .unwrap_err();
        assert_eq!(error, "staged remote media cleanup failed");
        assert!(cleanup_failure.cleanup_attempted.load(Ordering::Relaxed));
    }

    #[test]
    fn article_executor_writes_codemirror_verifies_optional_summary_and_closes_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"article-session"})),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("editor"),
            value(json!(true)),
            element("summary"),
            value(json!(null)),
            Err("not present".into()),
            Err("not present".into()),
            element("publish-panel"),
            value(json!(null)),
            element("confirm"),
            value(json!(null)),
            element("success"),
            value(json!(true)),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        let mut article = article_request();
        article.summary = Some("A concise summary".into());
        assert_eq!(
            publisher.publish_article(&article).unwrap(),
            "webdriver-juejin-1"
        );
        let bodies = publisher.transport.bodies.lock().unwrap();
        assert!(bodies.iter().any(|body| {
            body.get("script") == Some(&Value::String(CODEMIRROR_WRITE_SCRIPT.into()))
                && body
                    .get("args")
                    .and_then(Value::as_array)
                    .is_some_and(|args| {
                        args.get(1) == Some(&Value::String("# Article body".into()))
                    })
        }));
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .is_some_and(|path| path.ends_with("/session/article-session"))
        );
    }

    #[test]
    fn article_executor_rejects_unverified_codemirror_write_and_closes_session() {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"article-session"})),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("editor"),
            value(json!(false)),
            value(json!(null)),
        ]);
        let publisher = test_publisher(mock);
        let error = publisher.publish_article(&article_request()).unwrap_err();
        assert!(error.automation_attempted);
        assert!(
            publisher
                .transport
                .paths
                .lock()
                .unwrap()
                .last()
                .is_some_and(|path| path.ends_with("/session/article-session"))
        );
    }

    #[test]
    fn article_input_validation_bounds_inline_and_local_files() {
        let mut inline = article_request();
        inline.title = "x".repeat(MAX_ARTICLE_TITLE_BYTES + 1);
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&inline).is_err());
        let mut inline = article_request();
        inline.content = Some("x".repeat(MAX_ARTICLE_BODY_BYTES + 1));
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&inline).is_err());
        let unsupported = temporary_article_path("html");
        fs::write(&unsupported, "body").unwrap();
        let mut file_request = article_request();
        file_request.content = None;
        file_request.file = Some(unsupported.clone());
        assert!(
            WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err()
        );
        fs::remove_file(unsupported).unwrap();
        let empty = temporary_article_path("md");
        fs::write(&empty, "").unwrap();
        file_request.file = Some(empty.clone());
        assert!(
            WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err()
        );
        fs::remove_file(empty).unwrap();
        let oversized = temporary_article_path("txt");
        fs::write(&oversized, vec![b'x'; MAX_ARTICLE_BODY_BYTES + 1]).unwrap();
        file_request.file = Some(oversized.clone());
        assert!(
            WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err()
        );
        fs::remove_file(oversized).unwrap();
        let invalid_utf8 = temporary_article_path("md");
        fs::write(&invalid_utf8, [0xff]).unwrap();
        file_request.file = Some(invalid_utf8.clone());
        assert!(
            WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err()
        );
        fs::remove_file(invalid_utf8).unwrap();
        let valid = temporary_article_path("md");
        fs::write(&valid, "# valid body").unwrap();
        file_request.file = Some(valid.clone());
        assert_eq!(
            WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).unwrap(),
            "# valid body"
        );
        fs::remove_file(valid).unwrap();
    }

    #[test]
    fn article_executor_marks_local_validation_failure_as_not_attempted() {
        let publisher = test_publisher(MockWebDriver::new(Vec::new()));
        let mut request = article_request();
        request.title = "x".repeat(MAX_ARTICLE_TITLE_BYTES + 1);
        let error = publisher.publish_article(&request).unwrap_err();
        assert!(!error.automation_attempted);
        assert!(publisher.transport.paths.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn article_input_validation_rejects_symlink_and_non_regular_files() {
        use std::os::unix::fs::symlink;

        let target = temporary_article_path("md");
        let link = temporary_article_path("md");
        fs::write(&target, "body").unwrap();
        symlink(&target, &link).unwrap();
        let mut request = article_request();
        request.content = None;
        request.file = Some(link.clone());
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_file(target).unwrap();
        let directory = temporary_article_path("md");
        fs::create_dir(&directory).unwrap();
        request.file = Some(directory.clone());
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn article_input_validation_rejects_nonlocal_or_unbounded_cover() {
        let mut request = article_request();
        request.cover = Some("https://example.invalid/cover.png".into());
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
        let unsupported = temporary_article_path("gif");
        fs::write(&unsupported, "cover").unwrap();
        request.cover = Some(unsupported.to_string_lossy().into_owned());
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
        fs::remove_file(unsupported).unwrap();
        let oversized = temporary_article_path("png");
        fs::write(&oversized, vec![b'x'; MAX_ARTICLE_COVER_BYTES as usize + 1]).unwrap();
        request.cover = Some(oversized.to_string_lossy().into_owned());
        assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
        fs::remove_file(oversized).unwrap();
    }

    #[tokio::test]
    async fn article_protocol_is_unavailable_without_explicit_opt_in_even_with_video_attach() {
        let router = app(Arc::new(runner_service(Some(Arc::new(Accepted)), None)));
        let request = ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: article_request(),
        };
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(matches!(
            serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
            ArticleRunnerResponse::Unavailable { .. }
        ));
        let mut invalid = serde_json::to_value(request).unwrap();
        invalid["version"] = json!(99);
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let mut routed = article_request();
        routed.account.partition = Some("persist:forbidden".into());
        let response = router
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ArticleRunnerRequest {
                            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                            request: routed,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn article_protocol_queues_executor_response_and_rejects_unknown_payload_fields() {
        let router = app(Arc::new(runner_service(None, Some(Arc::new(Accepted)))));
        let request = ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: article_request(),
        };
        let response = router
            .clone()
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id: "article-job-1".into(),
                automation_attempted: true,
            }
        );
        let mut malformed = serde_json::to_value(request).unwrap();
        malformed["profile"] = json!("forbidden");
        let response = router
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(malformed.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn article_protocol_rejects_scheduled_requests_before_starting_an_executor() {
        let executor = Arc::new(CountingArticleExecutor(AtomicU64::new(0)));
        let router = app(Arc::new(runner_service(None, Some(executor.clone()))));
        let mut request = article_request();
        request.scheduled_at =
            Some(matrixpost_core::LocalSchedule::parse("2026-01-02 03:04:05").unwrap());
        let response = router
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ArticleRunnerRequest {
                            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                            request,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(executor.0.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn article_protocol_marks_executor_failure_as_an_attempted_automation() {
        let router = app(Arc::new(runner_service(
            None,
            Some(Arc::new(FailingArticleExecutor)),
        )));
        let response = router
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ArticleRunnerRequest {
                            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                            request: article_request(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
            ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                reason: "mock automation failure".into(),
                automation_attempted: true,
            }
        );
    }

    #[tokio::test]
    async fn article_protocol_marks_pre_session_validation_failure_as_not_attempted() {
        let router = app(Arc::new(runner_service(
            None,
            Some(Arc::new(LocalValidationArticleExecutor)),
        )));
        let response = router
            .oneshot(
                Request::post("/v1/publish-article")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&ArticleRunnerRequest {
                            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                            request: article_request(),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(matches!(
            serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
            ArticleRunnerResponse::Rejected {
                automation_attempted: false,
                ..
            }
        ));
    }
}
