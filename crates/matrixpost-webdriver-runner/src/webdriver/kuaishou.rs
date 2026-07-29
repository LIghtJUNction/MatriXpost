use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    KUAISHOU_STATEMENT_APPLIED_SCRIPT, KUAISHOU_STATEMENT_LIST_VISIBLE_SCRIPT,
    KUAISHOU_STATEMENT_OPEN_SCRIPT, KUAISHOU_STATEMENT_SELECT_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
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
}
