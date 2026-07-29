use super::*;
use matrixpost_core::{Platform, REVIEW_STATUS_TITLE_QUERY_MAX_BYTES, ReviewStatus};
use serde_json::json;

impl<T: WebDriverTransport> LoginNavigationExecutor for WebDriverPublisher<T> {
    fn open_manual_login(&self, platform: Platform) -> Result<(), String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let session = self.session()?;
        let outcome = self.navigate(&session, profile.upload_url);
        let cleanup = self.delete_session(&session);
        outcome?;
        cleanup
    }
}

impl<T: WebDriverTransport> AccountStatusExecutor for WebDriverPublisher<T> {
    fn account_readiness(&self, platform: Platform) -> Result<bool, String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let session = self.session()?;
        let outcome = (|| {
            self.navigate(&session, profile.upload_url)?;
            Ok(profile
                .file
                .iter()
                .any(|selector| self.find_once(&session, selector).is_some()))
        })();
        let cleanup = self.delete_session(&session);
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

impl<T: WebDriverTransport> ReviewStatusExecutor for WebDriverPublisher<T> {
    fn review_status(&self, title_query: &str) -> Result<ReviewStatus, String> {
        let title_query = normalize_review_title_query(title_query);
        if title_query.is_empty() || title_query.len() > REVIEW_STATUS_TITLE_QUERY_MAX_BYTES {
            return Err("review status title query is invalid".into());
        }
        let session = self.session()?;
        let outcome = (|| {
            self.navigate(&session, FANQIE_VIDEO_LIST_URL)?;
            for attempt in 0..FANQIE_REVIEW_SCROLL_ATTEMPTS {
                let value = Self::webdriver_value(self.transport.request(
                    "POST",
                    &format!("/session/{session}/execute/sync"),
                    json!({"script":FANQIE_REVIEW_STATUS_SCRIPT,"args":[title_query]}),
                )?)?;
                if let Some(status) = value.as_str() {
                    return match status {
                        "published" => Ok(ReviewStatus::Published),
                        "under_review" => Ok(ReviewStatus::UnderReview),
                        "rejected" => Ok(ReviewStatus::Rejected),
                        _ => Err("review status script returned an invalid value".into()),
                    };
                }
                if attempt + 1 < FANQIE_REVIEW_SCROLL_ATTEMPTS {
                    std::thread::sleep(FANQIE_REVIEW_SCROLL_INTERVAL);
                }
            }
            Ok(ReviewStatus::NotFound)
        })();
        let cleanup = self.delete_session(&session);
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}
