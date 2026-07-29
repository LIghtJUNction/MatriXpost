use super::ProviderRunner;
use crate::{
    error::DomainError,
    runner::{
        ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION, AccountReadiness, AccountStatusRunnerRequest,
        AccountStatusRunnerResponse, LOGIN_RUNNER_PROTOCOL_VERSION, LoginRunnerRequest,
        LoginRunnerResponse, ManualLoginOutcome, REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
        ReviewStatus, ReviewStatusRunnerRequest, ReviewStatusRunnerResponse,
        normalize_review_title_query,
    },
    types::Platform,
};
use std::time::Duration;

/// HTTP boundary for the explicit local manual-login protocol.
///
/// Implementations must send only to the endpoint provided by
/// [`ProviderRunner::request_manual_login_with`]. The core implementation
/// supplies a bounded-timeout loopback HTTP client; this trait permits
/// deterministic embedding tests without network access.
pub trait ManualLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError>;
}

/// Injectable HTTP boundary for the local account-readiness probe.
pub trait AccountStatusHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError>;
}

/// Injectable HTTP boundary for the local Fanqie review-status probe.
pub trait ReviewStatusHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError>;
}

/// Transport-level failure that never exposes local endpoints or runner data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualLoginTransportError {
    RequestFailed,
    ResponseReadFailed,
}

struct UreqManualLoginHttpTransport;

impl ManualLoginHttpTransport for UreqManualLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| ManualLoginTransportError::RequestFailed)?;
        let status = response.status();
        let body = response
            .into_string()
            .map_err(|_| ManualLoginTransportError::ResponseReadFailed)?;
        Ok((status, body))
    }
}

impl AccountStatusHttpTransport for UreqManualLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        ManualLoginHttpTransport::post_json(self, endpoint, body)
    }
}

impl ReviewStatusHttpTransport for UreqManualLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        ManualLoginHttpTransport::post_json(self, endpoint, body)
    }
}

impl ProviderRunner {
    /// Asks this explicitly configured local runner to open a platform page
    /// for manual login in its already-attached browser.
    ///
    /// This never starts a browser, reads browser state, or confirms that a
    /// user completed login. Only a validated loopback-TCP declaration may
    /// receive the request.
    pub fn request_manual_login(&self) -> Result<ManualLoginOutcome, DomainError> {
        self.request_manual_login_with(&UreqManualLoginHttpTransport)
    }

    /// Same as [`Self::request_manual_login`] with an injectable transport.
    pub fn request_manual_login_with<T: ManualLoginHttpTransport>(
        &self,
        transport: &T,
    ) -> Result<ManualLoginOutcome, DomainError> {
        let Some(address) = self.loopback_tcp_address() else {
            return Ok(ManualLoginOutcome::Unavailable);
        };
        let endpoint = format!("http://{address}/v1/login");
        let payload = LoginRunnerRequest {
            version: LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
        };
        let payload = serde_json::to_string(&payload).map_err(DomainError::serialization)?;
        let (status, body) = match transport.post_json(&endpoint, &payload) {
            Ok(response) => response,
            Err(_) => return Ok(ManualLoginOutcome::Rejected),
        };
        if status != 200 {
            return Ok(ManualLoginOutcome::Rejected);
        }
        let response: LoginRunnerResponse = match serde_json::from_str(&body) {
            Ok(response) => response,
            Err(_) => return Ok(ManualLoginOutcome::Rejected),
        };
        Ok(response
            .into_manual_login(self.platform)
            .unwrap_or(ManualLoginOutcome::Rejected))
    }

    /// Infers whether the platform upload form is reachable in a separately
    /// managed attached browser. No browser state is read or exported.
    pub fn account_readiness(&self) -> Result<AccountReadiness, DomainError> {
        self.account_readiness_with(&UreqManualLoginHttpTransport)
    }

    /// Same as [`Self::account_readiness`] with an injectable transport.
    pub fn account_readiness_with<T: AccountStatusHttpTransport>(
        &self,
        transport: &T,
    ) -> Result<AccountReadiness, DomainError> {
        let Some(address) = self.loopback_tcp_address() else {
            return Ok(AccountReadiness::Unavailable);
        };
        let payload = serde_json::to_string(&AccountStatusRunnerRequest {
            version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
        })
        .map_err(DomainError::serialization)?;
        let (status, body) =
            match transport.post_json(&format!("http://{address}/v1/account-status"), &payload) {
                Ok(response) => response,
                Err(_) => return Ok(AccountReadiness::Rejected),
            };
        if status != 200 {
            return Ok(AccountReadiness::Rejected);
        }
        Ok(serde_json::from_str::<AccountStatusRunnerResponse>(&body)
            .ok()
            .and_then(|response| response.into_readiness(self.platform))
            .unwrap_or(AccountReadiness::Rejected))
    }

    /// Queries only the bounded Fanqie title status in the attached local
    /// browser. No browser state, page contents, title, URL, or identifier is
    /// returned to the caller.
    pub fn fanqie_review_status(&self, title_query: &str) -> Result<ReviewStatus, DomainError> {
        self.fanqie_review_status_with(title_query, &UreqManualLoginHttpTransport)
    }

    /// Same as [`Self::fanqie_review_status`] with an injectable transport.
    pub fn fanqie_review_status_with<T: ReviewStatusHttpTransport>(
        &self,
        title_query: &str,
        transport: &T,
    ) -> Result<ReviewStatus, DomainError> {
        let request = ReviewStatusRunnerRequest {
            version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::FanqieVideo,
            title_query: normalize_review_title_query(title_query),
        };
        if self.platform != Platform::FanqieVideo || !request.validate() {
            return Ok(ReviewStatus::Rejected);
        }
        let Some(address) = self.loopback_tcp_address() else {
            return Ok(ReviewStatus::Unavailable);
        };
        let payload = serde_json::to_string(&request).map_err(DomainError::serialization)?;
        let (status, body) =
            match transport.post_json(&format!("http://{address}/v1/review-status"), &payload) {
                Ok(response) => response,
                Err(_) => return Ok(ReviewStatus::Rejected),
            };
        if status != 200 {
            return Ok(ReviewStatus::Rejected);
        }
        Ok(serde_json::from_str::<ReviewStatusRunnerResponse>(&body)
            .ok()
            .and_then(ReviewStatusRunnerResponse::into_review_status)
            .unwrap_or(ReviewStatus::Rejected))
    }
}
