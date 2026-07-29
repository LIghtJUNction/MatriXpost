use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
#[cfg(not(test))]
use crate::profiles::DOUYIN_READY_POLL_INTERVAL;
use crate::profiles::{
    DOUYIN_PREVIEW_STATE_SCRIPT, DOUYIN_READY_POLL_ATTEMPTS, DOUYIN_SAVE_PERMISSION_ACTION_SCRIPT,
    DOUYIN_SAVE_PERMISSION_STATE_SCRIPT, DOUYIN_STATEMENT_CONFIRM_SCRIPT,
    DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT, DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT,
    DOUYIN_STATEMENT_OPEN_SCRIPT, DOUYIN_STATEMENT_SELECT_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn douyin_ready_poll_interval() -> std::time::Duration {
        #[cfg(test)]
        {
            // Mock transports prove the full finite probe count; sleeping adds
            // no coverage and would turn a 30-second production boundary into
            // an unnecessarily slow unit test.
            std::time::Duration::ZERO
        }
        #[cfg(not(test))]
        {
            DOUYIN_READY_POLL_INTERVAL
        }
    }

    fn douyin_state(&self, session: &str, script: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("ready") => Ok("ready"),
            Some("selected") => Ok("selected"),
            Some("clicked") => Ok("clicked"),
            Some("pending") => Ok("pending"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("disabled") => Ok("disabled"),
            Some("invalid") => Ok("invalid"),
            _ => Err("Douyin post-upload action returned an invalid state".into()),
        }
    }

    fn wait_for_douyin_preview(&self, session: &str) -> Result<(), String> {
        for attempt in 0..DOUYIN_READY_POLL_ATTEMPTS {
            match self.douyin_state(session, DOUYIN_PREVIEW_STATE_SCRIPT)? {
                "ready" => return Ok(()),
                "pending" => {}
                "ambiguous" => return Err("Douyin video preview was ambiguous".into()),
                _ => return Err("Douyin video preview returned an invalid state".into()),
            }
            if attempt + 1 < DOUYIN_READY_POLL_ATTEMPTS {
                std::thread::sleep(Self::douyin_ready_poll_interval());
            }
        }
        Err("Douyin video preview did not become ready before its deadline".into())
    }

    fn set_douyin_save_permission(&self, session: &str) -> Result<(), String> {
        for attempt in 0..DOUYIN_READY_POLL_ATTEMPTS {
            match self.douyin_state(session, DOUYIN_SAVE_PERMISSION_STATE_SCRIPT)? {
                "selected" => return Ok(()),
                "ready" => {
                    match self.douyin_state(session, DOUYIN_SAVE_PERMISSION_ACTION_SCRIPT)? {
                        "selected" | "clicked" => return Ok(()),
                        "pending" => {}
                        "ambiguous" => return Err("Douyin save permission was ambiguous".into()),
                        "disabled" => return Err("Douyin save permission was disabled".into()),
                        "invalid" => {
                            return Err("Douyin save permission could not be selected".into());
                        }
                        _ => return Err("Douyin save permission returned an invalid state".into()),
                    }
                }
                "pending" => {}
                "ambiguous" => return Err("Douyin save permission was ambiguous".into()),
                "disabled" => return Err("Douyin save permission was disabled".into()),
                "invalid" => return Err("Douyin save permission was invalid".into()),
                _ => return Err("Douyin save permission returned an invalid state".into()),
            }
            if attempt + 1 < DOUYIN_READY_POLL_ATTEMPTS {
                std::thread::sleep(Self::douyin_ready_poll_interval());
            }
        }
        Err("Douyin save permission did not become ready before its deadline".into())
    }

    pub(super) fn prepare_douyin_video(&self, session: &str) -> Result<(), String> {
        self.wait_for_douyin_preview(session)?;
        self.set_douyin_save_permission(session)
    }

    pub(super) fn apply_douyin_autonomous_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
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
}
