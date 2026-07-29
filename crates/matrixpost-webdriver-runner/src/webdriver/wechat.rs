use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    WECHAT_UPLOAD_READY_POLL_ATTEMPTS, WECHAT_UPLOAD_READY_POLL_INTERVAL,
    WECHAT_UPLOAD_READY_STATE_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn wechat_upload_poll_interval() -> std::time::Duration {
        if cfg!(test) {
            std::time::Duration::ZERO
        } else {
            WECHAT_UPLOAD_READY_POLL_INTERVAL
        }
    }

    fn wechat_upload_state(&self, session: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":WECHAT_UPLOAD_READY_STATE_SCRIPT,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("ready") => Ok("ready"),
            Some("pending") => Ok("pending"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("invalid") => Ok("invalid"),
            _ => Err("WeChat upload processing returned an invalid state".into()),
        }
    }

    pub(super) fn wait_for_wechat_upload_ready(&self, session: &str) -> Result<(), String> {
        for attempt in 0..WECHAT_UPLOAD_READY_POLL_ATTEMPTS {
            match self.wechat_upload_state(session)? {
                "ready" => return Ok(()),
                "pending" => {}
                "ambiguous" => return Err("WeChat upload processing was ambiguous".into()),
                "invalid" => return Err("WeChat upload processing was invalid".into()),
                _ => return Err("WeChat upload processing returned an invalid state".into()),
            }
            if attempt + 1 < WECHAT_UPLOAD_READY_POLL_ATTEMPTS {
                std::thread::sleep(Self::wechat_upload_poll_interval());
            }
        }
        Err("WeChat upload processing did not become ready before its deadline".into())
    }
}
