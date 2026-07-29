use super::*;
use matrixpost_core::{ArticlePlatform, PublishArticleRequest};
use serde_json::json;
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    sync::atomic::Ordering,
};
use url::Url;

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn article_success_marker_visible(&self, session: &str) -> Result<bool, String> {
        for selector in JUEJIN_PROFILE.success {
            if let Some(element) = self.find_once(session, selector)
                && self.is_visible(session, &element)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn wait_for_article_success_transition(&self, session: &str) -> Result<(), String> {
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
