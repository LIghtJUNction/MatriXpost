use super::*;
use serde_json::{Value, json};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(super) fn webdriver_value(reply: Value) -> Result<Value, String> {
        reply
            .get("value")
            .cloned()
            .ok_or_else(|| "WebDriver response omitted value".into())
    }

    pub(super) fn session(&self) -> Result<String, String> {
        let debugger_address = self.browser_debugger_address.to_string();
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            "/session",
            json!({"capabilities":{"alwaysMatch":{"goog:chromeOptions":{"debuggerAddress":debugger_address}}}}),
        )?)?;
        value
            .get("sessionId")
            .and_then(Value::as_str)
            .or_else(|| value.get("session_id").and_then(Value::as_str))
            .map(str::to_owned)
            .ok_or_else(|| "WebDriver did not create a session".into())
    }

    pub(super) fn find_once(&self, session: &str, selector: &str) -> Option<String> {
        let reply = self
            .transport
            .request(
                "POST",
                &format!("/session/{session}/element"),
                json!({"using":"css selector","value":selector}),
            )
            .ok()?;
        let value = Self::webdriver_value(reply).ok()?;
        value
            .get(ELEMENT_KEY)
            .and_then(Value::as_str)
            .or_else(|| value.get("ELEMENT").and_then(Value::as_str))
            .map(str::to_owned)
    }

    pub(super) fn find(&self, session: &str, selectors: &[&str]) -> Result<String, String> {
        for attempt in 0..ELEMENT_POLL_ATTEMPTS {
            for selector in selectors {
                if let Some(element) = self.find_once(session, selector) {
                    return Ok(element);
                }
            }
            if attempt + 1 < ELEMENT_POLL_ATTEMPTS {
                std::thread::sleep(ELEMENT_POLL_INTERVAL);
            }
        }
        Err("no supported selector matched the current platform page".into())
    }

    pub(super) fn navigate(&self, session: &str, url: &str) -> Result<(), String> {
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/url"),
            json!({"url":url}),
        )?)
        .map(|_| ())
    }

    pub(super) fn input(
        &self,
        session: &str,
        selectors: &[&str],
        text: &str,
    ) -> Result<(), String> {
        let element = self.find(session, selectors)?;
        Self::webdriver_value(self.transport.request("POST", &format!("/session/{session}/element/{element}/value"), json!({"text":text,"value":text.chars().map(|item| item.to_string()).collect::<Vec<_>>() }))?).map(|_| ())
    }

    pub(super) fn click(&self, session: &str, selectors: &[&str]) -> Result<(), String> {
        let element = self.find(session, selectors)?;
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/element/{element}/click"),
            json!({}),
        )?)
        .map(|_| ())
    }

    pub(super) fn delete_session(&self, session: &str) -> Result<(), String> {
        Self::webdriver_value(self.transport.request(
            "DELETE",
            &format!("/session/{session}"),
            json!({}),
        )?)
        .map(|_| ())
    }

    pub(super) fn execute_bool(
        &self,
        session: &str,
        script: &str,
        args: Value,
    ) -> Result<bool, String> {
        Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":args}),
        )?)?
        .as_bool()
        .ok_or_else(|| "WebDriver shadow-root action did not return a boolean".into())
    }

    pub(super) fn is_visible(&self, session: &str, element: &str) -> Result<bool, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":VISIBLE_SCRIPT,"args":[{"element-6066-11e4-a52e-4f735466cecf":element}]}),
        )?)?;
        value
            .as_bool()
            .ok_or_else(|| "WebDriver visibility script did not return a boolean".into())
    }

    pub(super) fn success_marker_visible(
        &self,
        session: &str,
        profile: &PlatformProfile,
    ) -> Result<bool, String> {
        for selector in profile.success {
            if let Some(element) = self.find_once(session, selector)
                && self.is_visible(session, &element)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn wait_for_success_transition(
        &self,
        session: &str,
        profile: &PlatformProfile,
    ) -> Result<(), String> {
        for attempt in 0..self.acknowledgement.attempts {
            if self.success_marker_visible(session, profile)? {
                return Ok(());
            }
            if attempt + 1 < self.acknowledgement.attempts {
                std::thread::sleep(self.acknowledgement.interval);
            }
        }
        Err(
            "post-click success acknowledgement did not become visibly present before deadline"
                .into(),
        )
    }
}
