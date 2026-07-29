use super::ProviderRunner;
use crate::{
    error::DomainError,
    runner::{
        TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES, TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        TerminalQrLoginCancelRequest, TerminalQrLoginOutcome, TerminalQrLoginRefreshRequest,
        TerminalQrLoginRunnerRequest, TerminalQrLoginRunnerResponse,
    },
};
use std::{io::Read, time::Duration};

/// Injectable HTTP boundary for the local terminal QR login protocol.
///
/// The runner protocol only accepts a version and a supported platform. A
/// transport must not add browser-state headers, cookies, or any other
/// session-bearing data.
pub trait TerminalQrLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), TerminalQrLoginTransportError>;
}

/// Transport-level failure for a terminal QR runner request.
///
/// These variants deliberately contain no local endpoint or runner data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalQrLoginTransportError {
    RequestFailed,
    ResponseReadFailed,
    ResponseTooLarge,
}

struct UreqTerminalQrLoginHttpTransport;

impl TerminalQrLoginHttpTransport for UreqTerminalQrLoginHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), TerminalQrLoginTransportError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| TerminalQrLoginTransportError::RequestFailed)?;
        let status = response.status();
        let mut reader = response
            .into_reader()
            .take((TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES as u64) + 1);
        let mut response_body = String::new();
        reader
            .read_to_string(&mut response_body)
            .map_err(|_| TerminalQrLoginTransportError::ResponseReadFailed)?;
        if response_body.len() > TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES {
            return Err(TerminalQrLoginTransportError::ResponseTooLarge);
        }
        Ok((status, response_body))
    }
}

impl ProviderRunner {
    /// Starts or observes one explicit terminal QR login attempt at a local
    /// runner.
    ///
    /// This method never starts or reads a browser, and none of its outcomes
    /// claim that login completed. After the user scans a QR code, callers
    /// must use a separate readiness probe if they need an upload-form
    /// inference.
    pub fn request_terminal_qr_login(&self) -> Result<TerminalQrLoginOutcome, DomainError> {
        self.request_terminal_qr_login_with(&UreqTerminalQrLoginHttpTransport)
    }

    /// Same as [`Self::request_terminal_qr_login`] with an injectable
    /// transport.
    pub fn request_terminal_qr_login_with<T: TerminalQrLoginHttpTransport>(
        &self,
        transport: &T,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        let request = TerminalQrLoginRunnerRequest {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
        };
        if !request.validate() {
            return Ok(TerminalQrLoginOutcome::Rejected);
        }
        let payload = serde_json::to_string(&request).map_err(DomainError::serialization)?;
        self.terminal_qr_login_request_with("/v1/login/terminal-qr", payload, transport)
    }

    /// Refreshes a terminal QR attempt scoped to a bounded opaque token.
    ///
    /// This only returns a new QR state or an attempt terminal state. It never
    /// confirms login or exports browser/session data.
    pub fn refresh_terminal_qr_login(
        &self,
        attempt_token: &str,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        self.refresh_terminal_qr_login_with(attempt_token, &UreqTerminalQrLoginHttpTransport)
    }

    /// Same as [`Self::refresh_terminal_qr_login`] with injectable transport.
    pub fn refresh_terminal_qr_login_with<T: TerminalQrLoginHttpTransport>(
        &self,
        attempt_token: &str,
        transport: &T,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        let request = TerminalQrLoginRefreshRequest {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
            attempt_token: attempt_token.into(),
        };
        if !request.validate() {
            return Ok(TerminalQrLoginOutcome::Rejected);
        }
        let payload = serde_json::to_string(&request).map_err(DomainError::serialization)?;
        self.terminal_qr_login_request_with("/v1/login/terminal-qr/refresh", payload, transport)
    }

    /// Cancels a terminal QR attempt scoped to a bounded opaque token.
    ///
    /// This does not alter browser or platform session state and does not
    /// claim that the user logged in or out.
    pub fn cancel_terminal_qr_login(
        &self,
        attempt_token: &str,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        self.cancel_terminal_qr_login_with(attempt_token, &UreqTerminalQrLoginHttpTransport)
    }

    /// Same as [`Self::cancel_terminal_qr_login`] with injectable transport.
    pub fn cancel_terminal_qr_login_with<T: TerminalQrLoginHttpTransport>(
        &self,
        attempt_token: &str,
        transport: &T,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        let request = TerminalQrLoginCancelRequest {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
            attempt_token: attempt_token.into(),
        };
        if !request.validate() {
            return Ok(TerminalQrLoginOutcome::Rejected);
        }
        let payload = serde_json::to_string(&request).map_err(DomainError::serialization)?;
        self.terminal_qr_login_request_with("/v1/login/terminal-qr/cancel", payload, transport)
    }

    fn terminal_qr_login_request_with<T: TerminalQrLoginHttpTransport>(
        &self,
        path: &str,
        payload: String,
        transport: &T,
    ) -> Result<TerminalQrLoginOutcome, DomainError> {
        let Some(address) = self.loopback_tcp_address() else {
            return Ok(TerminalQrLoginOutcome::Unavailable);
        };
        let (status, body) = match transport.post_json(&format!("http://{address}{path}"), &payload)
        {
            Ok(response) => response,
            Err(_) => return Ok(TerminalQrLoginOutcome::Rejected),
        };
        if status != 200 || body.len() > TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES {
            return Ok(TerminalQrLoginOutcome::Rejected);
        }
        Ok(serde_json::from_str::<TerminalQrLoginRunnerResponse>(&body)
            .ok()
            .and_then(|response| response.into_terminal_qr_login(self.platform))
            .unwrap_or(TerminalQrLoginOutcome::Rejected))
    }
}
