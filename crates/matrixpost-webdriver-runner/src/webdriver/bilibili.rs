use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    BILIBILI_UPLOAD_READY_POLL_ATTEMPTS, BILIBILI_UPLOAD_READY_POLL_INTERVAL,
    BILIBILI_UPLOAD_READY_STATE_SCRIPT,
};

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
}
