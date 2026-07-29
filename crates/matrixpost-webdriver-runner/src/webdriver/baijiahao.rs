use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    BAIJIAHAO_ACTION_POLL_ATTEMPTS, BAIJIAHAO_ACTION_POLL_INTERVAL, BAIJIAHAO_ACTION_SCRIPT,
    BAIJIAHAO_ACTION_STATE_SCRIPT, BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT,
    BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT, BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT,
    BAIJIAHAO_STATEMENT_OPEN_SCRIPT, BAIJIAHAO_STATEMENT_SELECT_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn apply_baijiahao_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
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

    fn baijiahao_action_state(
        &self,
        session: &str,
        script: &str,
        action: &str,
    ) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":[action]}),
        )?)?;
        match value.as_str() {
            Some("ready") => Ok("ready"),
            Some("clicked") => Ok("clicked"),
            Some("pending") => Ok("pending"),
            Some("missing") => Ok("missing"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("disabled") => Ok("disabled"),
            _ => Err("Baijiahao publish action returned an invalid state".into()),
        }
    }

    fn wait_for_baijiahao_action(&self, session: &str, action: &str) -> Result<(), String> {
        for attempt in 0..BAIJIAHAO_ACTION_POLL_ATTEMPTS {
            match self.baijiahao_action_state(session, BAIJIAHAO_ACTION_STATE_SCRIPT, action)? {
                "ready" => return Ok(()),
                "pending" => {}
                "missing" => return Err("Baijiahao publish action was not found".into()),
                "ambiguous" => return Err("Baijiahao publish action was ambiguous".into()),
                "disabled" => return Err("Baijiahao publish action was disabled".into()),
                _ => return Err("Baijiahao publish readiness returned an invalid state".into()),
            }
            if attempt + 1 < BAIJIAHAO_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(BAIJIAHAO_ACTION_POLL_INTERVAL);
            }
        }
        Err("Baijiahao publish action did not become ready before its deadline".into())
    }

    pub(super) fn publish_baijiahao_action(
        &self,
        session: &str,
        requested_draft: bool,
    ) -> Result<(), String> {
        let action = if requested_draft { "draft" } else { "publish" };
        self.wait_for_baijiahao_action(session, action)?;
        match self.baijiahao_action_state(session, BAIJIAHAO_ACTION_SCRIPT, action)? {
            "clicked" => Ok(()),
            "pending" => Err("Baijiahao publish action was no longer ready".into()),
            "missing" => Err("Baijiahao publish action was not found".into()),
            "ambiguous" => Err("Baijiahao publish action was ambiguous".into()),
            "disabled" => Err("Baijiahao publish action became disabled".into()),
            _ => Err("Baijiahao publish action returned an invalid state".into()),
        }
    }
}
