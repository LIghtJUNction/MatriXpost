use super::WebDriverTransport;
use serde_json::Value;
use std::time::Duration;
use url::Url;

pub(crate) struct HttpWebDriver {
    pub(crate) endpoint: Url,
}

impl WebDriverTransport for HttpWebDriver {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String> {
        let endpoint = format!("{}{}", self.endpoint.as_str().trim_end_matches('/'), path);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(45))
            .build();
        let request = match method {
            "GET" => agent.get(&endpoint),
            "POST" => agent.post(&endpoint),
            "DELETE" => agent.delete(&endpoint),
            _ => return Err("unsupported WebDriver method".into()),
        };
        if method == "GET" {
            return request
                .call()
                .map_err(|_| "WebDriver request failed".to_owned())?
                .into_string()
                .map_err(|_| "WebDriver response was not valid JSON".to_owned())
                .and_then(|body| {
                    serde_json::from_str(&body)
                        .map_err(|_| "WebDriver response was not valid JSON".to_owned())
                });
        }
        let body = serde_json::to_string(&body)
            .map_err(|_| "WebDriver request could not be serialized".to_owned())?;
        request
            .set("content-type", "application/json")
            .send_string(&body)
            .map_err(|_| "WebDriver request failed".to_owned())?
            .into_string()
            .map_err(|_| "WebDriver response was not valid JSON".to_owned())
            .and_then(|body| {
                serde_json::from_str(&body)
                    .map_err(|_| "WebDriver response was not valid JSON".to_owned())
            })
    }
}
