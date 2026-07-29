use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    BILIBILI_TAG_COMMITTED_SCRIPT, BILIBILI_TAG_SUBMIT_SCRIPT, BILIBILI_UPLOAD_READY_POLL_ATTEMPTS,
    BILIBILI_UPLOAD_READY_POLL_INTERVAL, BILIBILI_UPLOAD_READY_STATE_SCRIPT,
};
use matrixpost_core::{Platform, PublishRequest};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn bilibili_upload_poll_interval() -> std::time::Duration {
        if cfg!(test) {
            std::time::Duration::ZERO
        } else {
            BILIBILI_UPLOAD_READY_POLL_INTERVAL
        }
    }

    fn bilibili_upload_state(&self, session: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":BILIBILI_UPLOAD_READY_STATE_SCRIPT,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("ready") => Ok("ready"),
            Some("pending") => Ok("pending"),
            Some("ambiguous") => Ok("ambiguous"),
            _ => Err("Bilibili upload processing returned an invalid state".into()),
        }
    }

    pub(super) fn wait_for_bilibili_upload_ready(&self, session: &str) -> Result<(), String> {
        for attempt in 0..BILIBILI_UPLOAD_READY_POLL_ATTEMPTS {
            match self.bilibili_upload_state(session)? {
                "ready" => return Ok(()),
                "pending" => {}
                "ambiguous" => return Err("Bilibili upload processing was ambiguous".into()),
                _ => return Err("Bilibili upload processing returned an invalid state".into()),
            }
            if attempt + 1 < BILIBILI_UPLOAD_READY_POLL_ATTEMPTS {
                std::thread::sleep(Self::bilibili_upload_poll_interval());
            }
        }
        Err("Bilibili upload processing did not become ready before its deadline".into())
    }

    pub(super) fn input_bilibili_tags(
        &self,
        session: &str,
        request: &PublishRequest,
    ) -> Result<(), String> {
        let tags = request
            .overrides
            .iter()
            .find(|item| item.platform == Platform::Bilibili)
            .and_then(|item| item.tags.as_deref())
            .unwrap_or(&request.tags);
        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() {
                return Err("Bilibili tags must not be empty".into());
            }
            if !self.execute_bool(session, BILIBILI_TAG_SUBMIT_SCRIPT, json!([tag]))? {
                return Err("Bilibili tag input was unavailable or disabled".into());
            }
            self.wait_for_statement_action(session, BILIBILI_TAG_COMMITTED_SCRIPT, json!([]))?;
        }
        Ok(())
    }
}
