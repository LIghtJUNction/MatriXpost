use serde_json::{Value, json};

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    DOUYIN_STATEMENT_POLL_ATTEMPTS, DOUYIN_STATEMENT_POLL_INTERVAL,
    TOUTIAO_STATEMENT_SELECT_SCRIPT, TOUTIAO_STATEMENT_SELECTED_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn wait_for_statement_action(
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

    fn select_toutiao_statement(&self, session: &str, label: &str) -> Result<bool, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":TOUTIAO_STATEMENT_SELECT_SCRIPT,"args":[label]}),
        )?)?;
        match value.as_str() {
            Some("selected") => Ok(true),
            Some("clicked") => Ok(false),
            Some("missing") => Err("Toutiao video-source checkbox was not found".into()),
            Some("ambiguous") => Err("Toutiao video-source checkbox was ambiguous".into()),
            Some("disabled") => Err("Toutiao video-source checkbox was disabled".into()),
            Some("unverified") => {
                Err("Toutiao video-source checkbox did not become selected".into())
            }
            _ => Err("Toutiao video-source checkbox returned an invalid action state".into()),
        }
    }

    pub(super) fn apply_toutiao_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
        if self.execute_bool(session, TOUTIAO_STATEMENT_SELECTED_SCRIPT, json!([label]))? {
            return Ok(());
        }
        if self.select_toutiao_statement(session, label)? {
            return Ok(());
        }
        self.wait_for_statement_action(session, TOUTIAO_STATEMENT_SELECTED_SCRIPT, json!([label]))
    }
}
