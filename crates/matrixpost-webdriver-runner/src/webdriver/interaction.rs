use super::*;
use serde_json::{Value, json};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn wait_for_wechat_shadow_action(
        &self,
        session: &str,
        script: &str,
        args: Value,
    ) -> Result<(), String> {
        for attempt in 0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
            if self.execute_bool(session, script, args.clone())? {
                return Ok(());
            }
            if attempt + 1 < WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(WECHAT_SHADOW_ACTION_POLL_INTERVAL);
            }
        }
        Err("WeChat shadow-root action did not complete before its deadline".into())
    }

    pub(super) fn wait_for_optional_wechat_shadow_action(
        &self,
        session: &str,
        script: &str,
    ) -> Result<bool, String> {
        for attempt in 0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
            if self.execute_bool(session, script, json!([]))? {
                return Ok(true);
            }
            if attempt + 1 < WECHAT_SHADOW_ACTION_POLL_ATTEMPTS {
                std::thread::sleep(WECHAT_SHADOW_ACTION_POLL_INTERVAL);
            }
        }
        Ok(false)
    }

    pub(super) fn attach_wechat_product(
        &self,
        session: &str,
        product_id: &str,
    ) -> Result<(), String> {
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_TYPE_READY_SCRIPT, json!([]))?;
        if !self.execute_bool(session, WECHAT_PRODUCT_OPEN_CHOOSER_SCRIPT, json!([]))? {
            return Err("WeChat product chooser could not be opened".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_PRODUCT_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )?;
        if !self.execute_bool(session, WECHAT_PRODUCT_SEARCH_SCRIPT, json!([product_id]))? {
            return Err("WeChat product search could not be started".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_PRODUCT_EXACT_ROW_SCRIPT,
            json!([product_id]),
        )?;
        if !self.execute_bool(
            session,
            WECHAT_PRODUCT_SELECT_EXACT_SCRIPT,
            json!([product_id]),
        )? {
            return Err("WeChat product could not be selected".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_ADD_READY_SCRIPT, json!([]))?;
        if !self.execute_bool(session, WECHAT_PRODUCT_ADD_SCRIPT, json!([]))? {
            return Err("WeChat product could not be added".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_PRODUCT_ATTACHED_SCRIPT, json!([]))
    }

    pub(super) fn apply_wechat_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
        if !self.execute_bool(session, WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("WeChat creative-statement selector could not be opened".into());
        }
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT,
            json!([label]),
        )
    }

    pub(super) fn apply_bilibili_creative_statement(
        &self,
        session: &str,
        label: &str,
    ) -> Result<(), String> {
        if !self.execute_bool(session, BILIBILI_STATEMENT_OPEN_SCRIPT, json!([]))? {
            return Err("Bilibili creative-statement selector could not be opened".into());
        }
        self.wait_for_statement_action(
            session,
            BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT,
            json!([label]),
        )?;
        if !self.execute_bool(session, BILIBILI_STATEMENT_SELECT_SCRIPT, json!([label]))? {
            return Err("Bilibili creative-statement option could not be selected".into());
        }
        self.wait_for_statement_action(session, BILIBILI_STATEMENT_LIST_GONE_SCRIPT, json!([label]))
    }

    pub(super) fn try_declare_wechat_original(&self, session: &str) -> Result<(), String> {
        if !self.execute_bool(session, WECHAT_ORIGINAL_ENTRY_SCRIPT, json!([]))? {
            return Ok(());
        }
        if !self.wait_for_optional_wechat_shadow_action(
            session,
            WECHAT_ORIGINAL_ANY_DIALOG_VISIBLE_SCRIPT,
        )? {
            return Ok(());
        }
        if self.execute_bool(
            session,
            WECHAT_ORIGINAL_PROTOCOL_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )? {
            self.wait_for_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT,
                json!([]),
            )?;
            self.wait_for_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_PROTOCOL_DIALOG_GONE_SCRIPT,
                json!([]),
            )?;
            if !self.wait_for_optional_wechat_shadow_action(
                session,
                WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT,
            )? {
                return Ok(());
            }
        } else if !self.execute_bool(
            session,
            WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT,
            json!([]),
        )? {
            return Err("WeChat original-declaration dialog state changed unexpectedly".into());
        }
        self.wait_for_wechat_shadow_action(session, WECHAT_ORIGINAL_CONFIRM_SCRIPT, json!([]))?;
        self.wait_for_wechat_shadow_action(
            session,
            WECHAT_ORIGINAL_DECLARATION_DIALOG_GONE_SCRIPT,
            json!([]),
        )
    }
}
