use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    XIAOHONGSHU_PK_COVER_CLOSE_SCRIPT, XIAOHONGSHU_PK_COVER_STATE_SCRIPT,
    XIAOHONGSHU_STATEMENT_APPLIED_SCRIPT, XIAOHONGSHU_STATEMENT_LIST_VISIBLE_SCRIPT,
    XIAOHONGSHU_STATEMENT_OPEN_SCRIPT, XIAOHONGSHU_STATEMENT_SELECT_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn xiaohongshu_statement_is_applied(&self, session: &str, label: &str) -> Result<bool, String> {
        let proof = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":XIAOHONGSHU_STATEMENT_APPLIED_SCRIPT,"args":[label]}),
        )?)?;
        match proof.as_str() {
            Some("description" | "placeholder") => Ok(true),
            Some("prompt" | "open" | "pending") => Ok(false),
            _ => Err("Xiaohongshu creative-statement proof was invalid".into()),
        }
    }

    fn wait_for_xiaohongshu_statement(&self, session: &str, label: &str) -> Result<(), String> {
        for attempt in 0..crate::profiles::DOUYIN_STATEMENT_POLL_ATTEMPTS {
            if self.xiaohongshu_statement_is_applied(session, label)? {
                return Ok(());
            }
            if attempt + 1 < crate::profiles::DOUYIN_STATEMENT_POLL_ATTEMPTS {
                std::thread::sleep(crate::profiles::DOUYIN_STATEMENT_POLL_INTERVAL);
            }
        }
        Err("Xiaohongshu creative-statement action did not complete before its deadline".into())
    }

    pub(super) fn apply_xiaohongshu_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
        if !self.execute_bool(session, XIAOHONGSHU_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Xiaohongshu creative-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            XIAOHONGSHU_STATEMENT_LIST_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, XIAOHONGSHU_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Xiaohongshu creative-statement option could not be selected".into());
        }
        self.wait_for_xiaohongshu_statement(session, label)
    }

    fn xiaohongshu_pk_cover_state(&self, session: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":XIAOHONGSHU_PK_COVER_STATE_SCRIPT,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("absent") => Ok("absent"),
            Some("unchecked") => Ok("unchecked"),
            Some("checked") => Ok("checked"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("invalid") => Ok("invalid"),
            _ => Err("Xiaohongshu PK-cover state was invalid".into()),
        }
    }

    pub(super) fn normalize_xiaohongshu_pk_cover(&self, session: &str) -> Result<(), String> {
        match self.xiaohongshu_pk_cover_state(session)? {
            "absent" | "unchecked" => return Ok(()),
            "ambiguous" => return Err("Xiaohongshu PK-cover checkbox was ambiguous".into()),
            "invalid" => return Err("Xiaohongshu PK-cover checkbox was invalid".into()),
            "checked" => {}
            _ => unreachable!("PK-cover state validator accepts only finite values"),
        }
        if !self.execute_bool(session, XIAOHONGSHU_PK_COVER_CLOSE_SCRIPT, json!([]))? {
            return Err("Xiaohongshu PK-cover checkbox could not be cleared".into());
        }
        for attempt in 0..crate::profiles::DOUYIN_STATEMENT_POLL_ATTEMPTS {
            match self.xiaohongshu_pk_cover_state(session)? {
                "unchecked" => return Ok(()),
                "checked" => {}
                "absent" => {
                    return Err("Xiaohongshu PK-cover checkbox disappeared after action".into());
                }
                "ambiguous" => return Err("Xiaohongshu PK-cover checkbox was ambiguous".into()),
                "invalid" => return Err("Xiaohongshu PK-cover checkbox was invalid".into()),
                _ => unreachable!("PK-cover state validator accepts only finite values"),
            }
            if attempt + 1 < crate::profiles::DOUYIN_STATEMENT_POLL_ATTEMPTS {
                std::thread::sleep(crate::profiles::DOUYIN_STATEMENT_POLL_INTERVAL);
            }
        }
        Err("Xiaohongshu PK-cover checkbox remained selected before its deadline".into())
    }
}
