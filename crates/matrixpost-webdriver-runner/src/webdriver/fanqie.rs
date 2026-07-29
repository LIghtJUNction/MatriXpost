use serde_json::json;

use super::{WebDriverPublisher, WebDriverTransport};
use crate::profiles::{
    FANQIE_CHANNEL_PANEL_OPEN_SCRIPT, FANQIE_CHANNEL_PANEL_VISIBLE_SCRIPT,
    FANQIE_CHANNELS_ENABLE_SCRIPT, FANQIE_CHANNELS_SELECTED_SCRIPT,
    FANQIE_ONE_CLICK_PUBLISH_READY_SCRIPT, FANQIE_ONE_CLICK_PUBLISH_SCRIPT,
    FANQIE_PUBLISH_POLL_ATTEMPTS, FANQIE_PUBLISH_POLL_INTERVAL, FANQIE_PUBLISH_RESULT_SCRIPT,
    FANQIE_UPLOAD_READY_SCRIPT,
};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    fn fanqie_action_state(&self, session: &str, script: &str) -> Result<&'static str, String> {
        let value = Self::webdriver_value(self.transport.request(
            "POST",
            &format!("/session/{session}/execute/sync"),
            json!({"script":script,"args":[]}),
        )?)?;
        match value.as_str() {
            Some("open") => Ok("open"),
            Some("opened") => Ok("opened"),
            Some("ready") => Ok("ready"),
            Some("selected") => Ok("selected"),
            Some("clicked") => Ok("clicked"),
            Some("success") => Ok("success"),
            Some("failure") => Ok("failure"),
            Some("pending") => Ok("pending"),
            Some("missing") => Ok("missing"),
            Some("ambiguous") => Ok("ambiguous"),
            Some("disabled") => Ok("disabled"),
            _ => Err("Fanqie publish action returned an invalid state".into()),
        }
    }

    fn wait_for_fanqie_bool(&self, session: &str, script: &str, stage: &str) -> Result<(), String> {
        for attempt in 0..FANQIE_PUBLISH_POLL_ATTEMPTS {
            if self.execute_bool(session, script, json!([]))? {
                return Ok(());
            }
            if attempt + 1 < FANQIE_PUBLISH_POLL_ATTEMPTS {
                std::thread::sleep(FANQIE_PUBLISH_POLL_INTERVAL);
            }
        }
        Err(format!(
            "Fanqie {stage} did not complete before its deadline"
        ))
    }

    fn wait_for_fanqie_result(&self, session: &str) -> Result<(), String> {
        for attempt in 0..FANQIE_PUBLISH_POLL_ATTEMPTS {
            match self.fanqie_action_state(session, FANQIE_PUBLISH_RESULT_SCRIPT)? {
                "success" => return Ok(()),
                "failure" => return Err("Fanqie publish result reported failure".into()),
                "pending" => {}
                _ => return Err("Fanqie publish result returned an invalid state".into()),
            }
            if attempt + 1 < FANQIE_PUBLISH_POLL_ATTEMPTS {
                std::thread::sleep(FANQIE_PUBLISH_POLL_INTERVAL);
            }
        }
        Err("Fanqie publish result did not arrive before its deadline".into())
    }

    fn wait_for_fanqie_one_click_publish(&self, session: &str) -> Result<(), String> {
        let mut last_state = "pending";
        for attempt in 0..FANQIE_PUBLISH_POLL_ATTEMPTS {
            match self.fanqie_action_state(session, FANQIE_ONE_CLICK_PUBLISH_READY_SCRIPT)? {
                "ready" => {
                    return match self
                        .fanqie_action_state(session, FANQIE_ONE_CLICK_PUBLISH_SCRIPT)?
                    {
                        "clicked" => Ok(()),
                        "missing" => Err("Fanqie one-click publish action was not found".into()),
                        "ambiguous" => Err("Fanqie one-click publish action was ambiguous".into()),
                        "disabled" => Err("Fanqie one-click publish action became disabled".into()),
                        _ => {
                            Err("Fanqie one-click publish action returned an invalid state".into())
                        }
                    };
                }
                "missing" => return Err("Fanqie one-click publish action was not found".into()),
                "ambiguous" => return Err("Fanqie one-click publish action was ambiguous".into()),
                state @ ("disabled" | "pending") => last_state = state,
                _ => {
                    return Err(
                        "Fanqie one-click publish readiness returned an invalid state".into(),
                    );
                }
            }
            if attempt + 1 < FANQIE_PUBLISH_POLL_ATTEMPTS {
                std::thread::sleep(FANQIE_PUBLISH_POLL_INTERVAL);
            }
        }
        Err(format!(
            "Fanqie one-click publish action remained {last_state} before its deadline"
        ))
    }

    pub(super) fn publish_fanqie_video(&self, session: &str) -> Result<(), String> {
        self.wait_for_fanqie_bool(session, FANQIE_UPLOAD_READY_SCRIPT, "upload readiness")?;
        match self.fanqie_action_state(session, FANQIE_CHANNEL_PANEL_OPEN_SCRIPT)? {
            "open" | "opened" => {}
            "missing" => return Err("Fanqie publish-channel panel was not found".into()),
            "ambiguous" => return Err("Fanqie publish-channel panel was ambiguous".into()),
            _ => return Err("Fanqie publish-channel panel returned an invalid state".into()),
        }
        self.wait_for_fanqie_bool(
            session,
            FANQIE_CHANNEL_PANEL_VISIBLE_SCRIPT,
            "publish-channel panel readiness",
        )?;
        match self.fanqie_action_state(session, FANQIE_CHANNELS_ENABLE_SCRIPT)? {
            "selected" | "clicked" => {}
            "missing" => return Err("Fanqie publish-channel switch was not found".into()),
            "ambiguous" => return Err("Fanqie publish-channel switches were ambiguous".into()),
            "disabled" => return Err("Fanqie publish-channel switch was disabled".into()),
            _ => return Err("Fanqie publish-channel switches returned an invalid state".into()),
        }
        self.wait_for_fanqie_bool(
            session,
            FANQIE_CHANNELS_SELECTED_SCRIPT,
            "publish-channel selection",
        )?;
        self.wait_for_fanqie_one_click_publish(session)?;
        self.wait_for_fanqie_result(session)
    }
}
