use serde_json::{Value, json};

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    DOUYIN_STATEMENT_POLL_ATTEMPTS, DOUYIN_STATEMENT_POLL_INTERVAL, TOUTIAO_FOOTER_ACTION_SCRIPT,
    TOUTIAO_FOOTER_STATE_SCRIPT, TOUTIAO_STATEMENT_SELECT_SCRIPT,
    TOUTIAO_STATEMENT_SELECTED_SCRIPT,
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

    fn toutiao_footer_value(
        &self,
        session: &str,
        script: &str,
        args: Value,
    ) -> Result<String, String> {
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":args}),
        )?)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Toutiao publish footer returned an invalid action state".into())
    }

    fn wait_for_toutiao_footer_ready(&self, session: &str) -> Result<String, String> {
        for attempt in 0..DOUYIN_STATEMENT_POLL_ATTEMPTS {
            let state =
                self.toutiao_footer_value(session, TOUTIAO_FOOTER_STATE_SCRIPT, json!([]))?;
            match state.as_str() {
                "horizontal_ready" | "vertical_ready" => return Ok(state),
                "pending" => {
                    if attempt + 1 < DOUYIN_STATEMENT_POLL_ATTEMPTS {
                        std::thread::sleep(DOUYIN_STATEMENT_POLL_INTERVAL);
                    }
                }
                "ambiguous" => return Err("Toutiao publish footer action was ambiguous".into()),
                "disabled" => return Err("Toutiao publish footer action was disabled".into()),
                "invalid" => return Err("Toutiao publish footer action was invalid".into()),
                _ => return Err("Toutiao publish footer returned an invalid action state".into()),
            }
        }
        Err("Toutiao publish footer did not become ready before its deadline".into())
    }

    pub(super) fn publish_toutiao_footer(
        &self,
        session: &str,
        requested_draft: bool,
    ) -> Result<(), String> {
        let layout = self.wait_for_toutiao_footer_ready(session)?;
        let action = if requested_draft && layout == "horizontal_ready" {
            "draft"
        } else {
            "submit"
        };
        match self
            .toutiao_footer_value(session, TOUTIAO_FOOTER_ACTION_SCRIPT, json!([action]))?
            .as_str()
        {
            "clicked" => Ok(()),
            "pending" => Err("Toutiao publish footer was no longer ready for its action".into()),
            "ambiguous" => Err("Toutiao publish footer action was ambiguous".into()),
            "disabled" => Err("Toutiao publish footer action was disabled".into()),
            "invalid" => Err("Toutiao publish footer rejected the requested action".into()),
            _ => Err("Toutiao publish footer returned an invalid action state".into()),
        }
    }
}
