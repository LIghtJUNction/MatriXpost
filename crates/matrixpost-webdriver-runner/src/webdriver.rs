use std::{
    fs::{self, File},
    io::Read,
    net::SocketAddr,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use matrixpost_core::{
    ArticlePlatform, MediaSource, Platform, PublishArticleRequest, PublishRequest,
    REVIEW_STATUS_TITLE_QUERY_MAX_BYTES, ReviewStatus,
};
use serde_json::{Value, json};
use url::Url;

use crate::profiles::*;
mod kuaishou;

pub(crate) trait WebDriverTransport: Send + Sync {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String>;
}

pub(crate) struct HttpWebDriver {
    pub(crate) endpoint: Url,
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

pub(crate) trait PublicationExecutor: Send + Sync {
    fn publish(&self, platform: Platform, request: &PublishRequest) -> Result<String, String>;
}

/// Opens an existing attached browser at a platform page for a user-driven
/// login. It does not inspect session state or assert that login completed.
pub(crate) trait LoginNavigationExecutor: Send + Sync {
    fn open_manual_login(&self, platform: Platform) -> Result<(), String>;
}

/// Performs only an upload-form presence inference in an attached browser.
pub(crate) trait AccountStatusExecutor: Send + Sync {
    fn account_readiness(&self, platform: Platform) -> Result<bool, String>;
}

/// Looks up only a bounded Fanqie title in an attached browser and returns a
/// finite review outcome. It never returns DOM text or identifiers.
pub(crate) trait ReviewStatusExecutor: Send + Sync {
    fn review_status(&self, title_query: &str) -> Result<ReviewStatus, String>;
}

#[derive(Debug)]
pub(crate) struct ArticleExecutionError {
    pub(crate) reason: String,
    pub(crate) automation_attempted: bool,
}

impl ArticleExecutionError {
    pub(crate) fn local(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: false,
        }
    }

    pub(crate) fn attempted(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: true,
        }
    }
}

pub(crate) trait ArticlePublicationExecutor: Send + Sync {
    fn publish_article(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<String, ArticleExecutionError>;
}

pub(crate) struct WebDriverPublisher<T> {
    pub(crate) transport: T,
    pub(crate) browser_debugger_address: SocketAddr,
    pub(crate) acknowledgement: AcknowledgementPolicy,
    pub(crate) next_job: AtomicU64,
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

    fn execute_bool(&self, session: &str, script: &str, args: Value) -> Result<bool, String> {
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":args}),
        )?)?
        .as_bool()
        .ok_or_else(|| "WebDriver shadow-root action did not return a boolean".into())
    }

    fn wait_for_wechat_shadow_action(
        &self,
        session: &str,
        script: &str,
        args: Value,
    ) -> Result<(), String> {
        for attempt in 0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
            if self.execute_bool(session, script, args.clone())? {
                return Ok(());
            }
            if attempt + 1 < WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(WECHAT_SHADOW_ACTION_POLL_INTERVAL);
            }
        }
        Err("WeChat shadow-root action did not complete before its deadline".into())
    }

    fn wait_for_optional_wechat_shadow_action(
        &self,
        session: &str,
        script: &str,
    ) -> Result<bool, String> {
        for attempt in 0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
            if self.execute_bool(session, script, json!([]))? {
                return Ok(true);
            }
            if attempt + 1 < WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(WECHAT_SHADOW_ACTION_POLL_INTERVAL);
            }
        }
        Ok(false)
    }

    fn wait_for_statement_action(
        &self,
        session: &str,
        script: &str,
        args: Value,
    ) -> Result<(), String> {
        for attempt in 0..DOUYIN_STATEMENT_POLL_ATTEMPTS {
            if self.execute_bool(session, script, args.clone())? {
                return Ok(());
            }
            if attempt + 1 < DOUYIN_STATEMENT_POLL_ATTEMPTS {
                std::thread::sleep(DOUYIN_STATEMENT_POLL_INTERVAL);
            }
        }
        Err("creative-statement action did not complete before its deadline".into())
    }

    pub(crate) fn wechat_product_id(request: &PublishRequest) -> Result<Option<String>, String> {
        let link = &request.wechat_link;
        let product_id = link.product_id.as_deref().map(str::trim);
        let link_type = link.link_type.as_deref().map(str::trim);
        let link_value = link.link_value.as_deref().map(str::trim);
        // MatrixMedia accepts the explicit sphProductId independently of the
        // optional `sphLink` object. Keep that precedence: a product ID is a
        // complete product attachment request even if old input carries an
        // unrelated link-type field beside it.
        let product_id = if let Some(product_id) = product_id.filter(|value| !value.is_empty()) {
            product_id
        } else {
            match link_type {
                None if link_value.is_none() => return Ok(None),
                Some(value) if value.eq_ignore_ascii_case("none") => return Ok(None),
                Some(value) if value.eq_ignore_ascii_case("product") => link_value
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        "WeChat product link requires a non-empty product identifier".to_owned()
                    })?,
                Some(_) => {
                    return Err("WeChat link type is not supported by the local runner".into());
                }
                None => return Err("WeChat link type is required for its link value".into()),
            }
        };
        if product_id.len() > 128 || product_id.chars().any(char::is_control) {
            return Err("WeChat product identifier is malformed".into());
        }
        Ok(Some(product_id.to_owned()))
    }

    fn attach_wechat_product(&self, session: &str, product_id: &str) -> Result<(), String> {
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_TYPE_READY_SCRIPT, json!([]))?;
        if !self.execute_bool(session, WECHAT_PRODUCT_OPEN_CHOOSER_SCRIPT, json!([]))? {
            return Err("WeChat product chooser could not be opened".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_PRODUCT_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )?;
        if !self.execute_bool(session, WECHAT_PRODUCT_SEARCH_SCRIPT, json!([product_id]))? {
            return Err("WeChat product search could not be started".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_PRODUCT_EXACT_ROW_SCRIPT,
            json!([product_id]),
        )?;
        if !self.execute_bool(
            session,
            WECHAT_PRODUCT_SELECT_EXACT_SCRIPT,
            json!([product_id]),
        )? {
            return Err("WeChat product could not be selected".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_ADD_READY_SCRIPT, json!([]))?;
        if !self.execute_bool(session, WECHAT_PRODUCT_ADD_SCRIPT, json!([]))? {
            return Err("WeChat product could not be added".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_ATTACHED_SCRIPT, json!([]))
    }

    fn apply_wechat_creative_statement(&self, session: &str, label: &str) -> Result<(), String> {
        if !self.execute_bool(session, WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("WeChat creative-statement selector could not be opened".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT,
            json!([label]),
        )
    }

    fn apply_douyin_autonomous_statement(&self, session: &str, label: &str) -> Result<(), String> {
        if !self.execute_bool(session, DOUYIN_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Douyin autonomous-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, DOUYIN_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Douyin autonomous-statement option could not be selected".into());
        }
        if !self.execute_bool(session, DOUYIN_STATEMENT_CONFIRM_SCRIPT, json!([label]))? {
            return Err("Douyin autonomous-statement confirmation was unavailable".into());
        }
        self.wait_for_statement_action(session, DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT, json!([label]))
    }

    fn apply_baijiahao_creative_statement(&self, session: &str, label: &str) -> Result<(), String> {
        if !self.execute_bool(session, BAIJIAHAO_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Baijiahao creative-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, BAIJIAHAO_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Baijiahao creative-statement option could not be selected".into());
        }
        if !self.execute_bool(session, BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT, json!([label]))? {
            return Err("Baijiahao creative-statement confirmation was unavailable".into());
        }
        self.wait_for_statement_action(
            session,
            BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT,
            json!([label]),
        )
    }

    fn apply_bilibili_creative_statement(&self, session: &str, label: &str) -> Result<(), String> {
        if !self.execute_bool(session, BILIBILI_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Bilibili creative-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, BILIBILI_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Bilibili creative-statement option could not be selected".into());
        }
        self.wait_for_statement_action(session, BILIBILI_STATEMENT_LIST_GONE_SCRIPT, json!([label]))
    }

    fn try_declare_wechat_original(&self, session: &str) -> Result<(), String> {
        if !self.execute_bool(session, WECHAT_ORIGINAL_ENTRY_SCRIPT, json!([]))? {
            return Ok(());
        }
        if !self.wait_for_optional_wechat_shadow_action(
            session,
            WECHAT_ORIGINAL_ANY_DIALOG_VISIBLE_SCRIPT,
        )? {
            return Ok(());
        }
        if self.execute_bool(
            session,
            WECHAT_ORIGINAL_PROTOCOL_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )? {
            self.wait_for_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT,
                json!([]),
            )?;
            self.wait_for_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_PROTOCOL_DIALOG_GONE_SCRIPT,
                json!([]),
            )?;
            if !self.wait_for_optional_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT,
            )? {
                return Ok(());
            }
        } else if !self.execute_bool(
            session,
            WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )? {
            return Err("WeChat original-declaration dialog state changed unexpectedly".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_ORIGINAL_CONFIRM_SCRIPT, json!([]))?;
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_ORIGINAL_DECLARATION_DIALOG_GONE_SCRIPT,
            json!([]),
        )
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

    pub(crate) fn validate_article_request(
        request: &PublishArticleRequest,
    ) -> Result<String, String> {
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
        if !matches!(
            platform,
            Platform::WechatChannels
                | Platform::Douyin
                | Platform::Bilibili
                | Platform::Baijiahao
                | Platform::Kuaishou
        ) && let Some(statement) =
            override_value.and_then(|item| item.creative_statement.as_ref())
        {
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

    fn short_title(platform: Platform, request: &PublishRequest) -> Option<&str> {
        request
            .overrides
            .iter()
            .find(|item| item.platform == platform)
            .and_then(|item| item.short_title.as_deref())
            .or(request.short_title.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
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

impl<T: WebDriverTransport> AccountStatusExecutor for WebDriverPublisher<T> {
    fn account_readiness(&self, platform: Platform) -> Result<bool, String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let session = self.session()?;
        let outcome = (|| {
            self.navigate(&session, profile.upload_url)?;
            Ok(profile
                .file
                .iter()
                .any(|selector| self.find_once(&session, selector).is_some()))
        })();
        let cleanup = self.delete_session(&session);
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

impl<T: WebDriverTransport> ReviewStatusExecutor for WebDriverPublisher<T> {
    fn review_status(&self, title_query: &str) -> Result<ReviewStatus, String> {
        let title_query = normalize_review_title_query(title_query);
        if title_query.is_empty() || title_query.len() > REVIEW_STATUS_TITLE_QUERY_MAX_BYTES {
            return Err("review status title query is invalid".into());
        }
        let session = self.session()?;
        let outcome = (|| {
            self.navigate(&session, FANQIE_VIDEO_LIST_URL)?;
            for attempt in 0..FANQIE_REVIEW_SCROLL_ATTEMPTS {
                let value = Self::webdriver_value(self.transport.request(
                    "POST",
                    &format!("/session/{session}/execute/sync"),
                    json!({"script":FANQIE_REVIEW_STATUS_SCRIPT,"args":[title_query]}),
                )?)?;
                if let Some(status) = value.as_str() {
                    return match status {
                        "published" => Ok(ReviewStatus::Published),
                        "under_review" => Ok(ReviewStatus::UnderReview),
                        "rejected" => Ok(ReviewStatus::Rejected),
                        _ => Err("review status script returned an invalid value".into()),
                    };
                }
                if attempt + 1 < FANQIE_REVIEW_SCROLL_ATTEMPTS {
                    std::thread::sleep(FANQIE_REVIEW_SCROLL_INTERVAL);
                }
            }
            Ok(ReviewStatus::NotFound)
        })();
        let cleanup = self.delete_session(&session);
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
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
        // Validate WeChat product input before creating an attached-browser session.
        let wechat_product = if platform == Platform::WechatChannels {
            Self::wechat_product_id(request)?
        } else {
            None
        };
        let wechat_creative_statement = (platform == Platform::WechatChannels)
            .then(|| wechat_creative_statement_label(request))
            .flatten();
        let douyin_autonomous_statement = (platform == Platform::Douyin)
            .then(|| douyin_autonomous_statement_label(request))
            .flatten();
        let baijiahao_creative_statement = (platform == Platform::Baijiahao)
            .then(|| baijiahao_creative_statement_label(request))
            .flatten();
        let bilibili_creative_statement = (platform == Platform::Bilibili)
            .then(|| bilibili_creative_statement_label(request))
            .flatten();
        let kuaishou_creative_statement = (platform == Platform::Kuaishou)
            .then(|| kuaishou_creative_statement_label(request))
            .flatten();
        let session = self.session()?;
        let outcome: Result<(), String> = (|| {
            self.navigate(&session, profile.upload_url)?;
            self.input(&session, profile.file, file)?;
            self.input(&session, profile.title, Self::title(platform, request))?;
            if platform == Platform::WechatChannels
                && let (Some(selectors), Some(short_title)) =
                    (profile.short_title, Self::short_title(platform, request))
            {
                self.input(&session, selectors, short_title)?;
            }
            let description = Self::description(platform, request);
            if !description.is_empty() {
                self.input(&session, profile.description, &description)?;
            }
            if let Some(product_id) = wechat_product.as_deref() {
                self.attach_wechat_product(&session, product_id)?;
            }
            if let Some(label) = wechat_creative_statement {
                self.apply_wechat_creative_statement(&session, label)?;
            }
            if let Some(label) = douyin_autonomous_statement {
                self.apply_douyin_autonomous_statement(&session, label)?;
            }
            if let Some(label) = baijiahao_creative_statement {
                self.apply_baijiahao_creative_statement(&session, label)?;
            }
            if let Some(label) = bilibili_creative_statement {
                self.apply_bilibili_creative_statement(&session, label)?;
            }
            if let Some(label) = kuaishou_creative_statement {
                self.apply_kuaishou_creative_statement(&session, label)?;
            }
            if platform == Platform::WechatChannels {
                self.try_declare_wechat_original(&session)?;
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
