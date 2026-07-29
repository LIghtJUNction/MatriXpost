use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    KUAISHOU_ACTION_POLL_ATTEMPTS, KUAISHOU_ACTION_POLL_INTERVAL, KUAISHOU_ACTION_SCRIPT,
    KUAISHOU_ACTION_STATE_SCRIPT, KUAISHOU_STATEMENT_APPLIED_SCRIPT,
    KUAISHOU_STATEMENT_LIST_VISIBLE_SCRIPT, KUAISHOU_STATEMENT_OPEN_SCRIPT,
    KUAISHOU_STATEMENT_SELECT_SCRIPT, profile,
};
use matrixpost_core::{Platform, PublishRequest};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn input_kuaishou_metadata(
        &self,
        session: &str,
        request: &PublishRequest,
    ) -> Result<(), String> {
        let tags = request
            .overrides
            .iter()
            .find(|item| item.platform == Platform::Kuaishou)
            .and_then(|item| item.tags.as_ref())
            .unwrap_or(&request.tags)
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" ");
        let text = format!("{} {tags}", Self::title(Platform::Kuaishou, request));
        self.input(
            session,
            profile(Platform::Kuaishou)
                .expect("Kuaishou profile is installed")
                .title,
            &text,
        )
    }

    pub(super) fn apply_kuaishou_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
        if !self.execute_bool(session, KUAISHOU_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Kuaishou creative-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            KUAISHOU_STATEMENT_LIST_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, KUAISHOU_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Kuaishou creative-statement option could not be selected".into());
        }
        self.wait_for_statement_action(session, KUAISHOU_STATEMENT_APPLIED_SCRIPT, json!([label]))
    }

    fn kuaishou_action_state(&self, session: &str, script: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("ready") => Ok("ready"),
            Some("clicked") => Ok("clicked"),
            Some("pending") => Ok("pending"),
            Some("missing") => Ok("missing"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("disabled") => Ok("disabled"),
            _ => Err("Kuaishou publish action returned an invalid state".into()),
        }
    }

    fn wait_for_kuaishou_action(&self, session: &str) -> Result<(), String> {
        for attempt in 0..KUAISHOU_ACTION_POLL_ATTEMPTS {
            match self.kuaishou_action_state(session, KUAISHOU_ACTION_STATE_SCRIPT)? {
                "ready" => return Ok(()),
                "pending" => {}
                "missing" => return Err("Kuaishou publish action was not found".into()),
                "ambiguous" => return Err("Kuaishou publish action was ambiguous".into()),
                "disabled" => return Err("Kuaishou publish action was disabled".into()),
                _ => return Err("Kuaishou publish readiness returned an invalid state".into()),
            }
            if attempt + 1 < KUAISHOU_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(KUAISHOU_ACTION_POLL_INTERVAL);
            }
        }
        Err("Kuaishou video preview did not become ready before its deadline".into())
    }

    pub(super) fn publish_kuaishou_action(&self, session: &str) -> Result<(), String> {
        self.wait_for_kuaishou_action(session)?;
        match self.kuaishou_action_state(session, KUAISHOU_ACTION_SCRIPT)? {
            "clicked" => Ok(()),
            "pending" => Err("Kuaishou video preview was no longer ready".into()),
            "missing" => Err("Kuaishou publish action was not found".into()),
            "ambiguous" => Err("Kuaishou publish action was ambiguous".into()),
            "disabled" => Err("Kuaishou publish action became disabled".into()),
            _ => Err("Kuaishou publish action returned an invalid state".into()),
        }
    }
}
