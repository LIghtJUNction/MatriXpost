//! Video provider transport, local runner declarations, and dispatch registry.

use super::protocol::*;
use crate::{error::DomainError, types::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, io::Read, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration,
};
use thiserror::Error;

/// Request sent from an embedding to its configured local runner.
///
/// The endpoint is deliberately separate from WebDriver. It carries only a
/// validated publication request and never browser/session configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRunnerRequest {
    pub version: u16,
    pub platform: Platform,
    pub request: PublishRequest,
}

/// Response accepted from a local runner.
///
/// `Queued` means the runner completed its configured WebDriver phases; it is
/// not a claim that the remote platform has finished processing the media.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderRunnerResponse {
    Queued {
        version: u16,
        platform: Platform,
        job_id: String,
    },
    Unavailable {
        version: u16,
        platform: Platform,
        reason: String,
    },
    Rejected {
        version: u16,
        platform: Platform,
        reason: String,
    },
}

impl ProviderRunnerResponse {
    pub(crate) fn into_dispatch(self, expected_platform: Platform) -> Option<DispatchOutcome> {
        match self {
            Self::Queued {
                version,
                platform,
                job_id,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION
                && platform == expected_platform
                && !job_id.trim().is_empty() =>
            {
                Some(DispatchOutcome::Queued { job_id })
            }
            Self::Unavailable {
                version,
                platform,
                reason,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(DispatchOutcome::Unavailable { reason })
            }
            Self::Rejected {
                version,
                platform,
                reason,
            } if version == PROVIDER_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(DispatchOutcome::Rejected { reason })
            }
            _ => None,
        }
    }
}
/// Boundary implemented by opt-in platform adapters.
pub trait PublishProvider: Send + Sync {
    fn platform(&self) -> Platform;
    fn availability(&self) -> ProviderAvailability;
    fn enqueue(&self, request: &PublishRequest) -> Result<DispatchOutcome, DomainError>;
}

/// Provider which invokes the versioned protocol on one loopback TCP runner.
///
/// It never contacts a platform or WebDriver directly. Transport and response
/// failures become explicit rejected outcomes so callers cannot mistake a
/// malformed local response for publication acceptance.
pub(crate) struct TcpRunnerProvider {
    pub(crate) platform: Platform,
    pub(crate) address: SocketAddr,
}

pub(crate) trait RunnerHttpTransport {
    fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()>;
}

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

/// Transport-level failure that never exposes local endpoints or runner data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualLoginTransportError {
    RequestFailed,
    ResponseReadFailed,
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

struct UreqManualLoginHttpTransport;

struct UreqTerminalQrLoginHttpTransport;

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

struct UreqRunnerHttpTransport;

impl RunnerHttpTransport for UreqRunnerHttpTransport {
    fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| ())?;
        let status = response.status();
        let body = response.into_string().map_err(|_| ())?;
        Ok((status, body))
    }
}

impl TcpRunnerProvider {
    fn rejected() -> DispatchOutcome {
        DispatchOutcome::Rejected {
            reason: "local provider runner did not return a valid accepted response".into(),
        }
    }

    pub(crate) fn enqueue_with<T: RunnerHttpTransport>(
        &self,
        request: &PublishRequest,
        transport: &T,
    ) -> Result<DispatchOutcome, DomainError> {
        let endpoint = format!("http://{}/v1/publish", self.address);
        let payload = ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: self.platform,
            request: request.runner_safe(),
        };
        let payload = serde_json::to_string(&payload).map_err(DomainError::serialization)?;
        let (status, body) = match transport.post_json(&endpoint, &payload) {
            Ok(response) => response,
            Err(()) => return Ok(Self::rejected()),
        };
        if status != 200 {
            return Ok(Self::rejected());
        }
        let response: ProviderRunnerResponse = match serde_json::from_str(&body) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected()),
        };
        Ok(response
            .into_dispatch(self.platform)
            .unwrap_or_else(Self::rejected))
    }
}

impl PublishProvider for TcpRunnerProvider {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn enqueue(&self, request: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        self.enqueue_with(request, &UreqRunnerHttpTransport)
    }
}

/// A credential-free description of a local runner owned by an embedding.
///
/// MatriXpost never launches a runner. A loopback-TCP declaration installs the
/// stable v1 HTTP adapter and opens the endpoint only when dispatching a valid
/// request; other declared transports remain visibility-only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRunner {
    pub platform: Platform,
    #[serde(flatten)]
    pub transport: ProviderRunnerTransport,
}

/// Local-only transports understood by a [`ProviderRunner`] declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum ProviderRunnerTransport {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
    Tcp { address: SocketAddr },
}

/// Invalid provider-runner configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderRunnerConfigError {
    #[error("provider runner platform is configured more than once: {platform:?}")]
    DuplicatePlatform { platform: Platform },
    #[error("provider runner unix socket path must be absolute")]
    UnixSocketPathMustBeAbsolute,
    #[error("provider runner named pipe must use the \\\\.\\pipe\\ namespace")]
    NamedPipeMustBeLocal,
    #[error("provider runner TCP address must bind to loopback")]
    TcpMustBeLoopback,
    #[error("provider runner endpoint must not contain credential-like data")]
    CredentialLikeEndpoint,
    #[error("provider runner argument must use PLATFORM=TRANSPORT:ENDPOINT")]
    InvalidArgument,
}

impl ProviderRunner {
    /// Validates that this is a local, credential-free runner declaration.
    pub fn validate(&self) -> Result<(), ProviderRunnerConfigError> {
        match &self.transport {
            ProviderRunnerTransport::UnixSocket { path } => {
                if !path.is_absolute() {
                    return Err(ProviderRunnerConfigError::UnixSocketPathMustBeAbsolute);
                }
                reject_credential_like_endpoint(&path.to_string_lossy())?;
            }
            ProviderRunnerTransport::NamedPipe { name } => {
                if !name.starts_with(r"\\.\pipe\") {
                    return Err(ProviderRunnerConfigError::NamedPipeMustBeLocal);
                }
                reject_credential_like_endpoint(name)?;
            }
            ProviderRunnerTransport::Tcp { address } => {
                if !address.ip().is_loopback() {
                    return Err(ProviderRunnerConfigError::TcpMustBeLoopback);
                }
            }
        }
        Ok(())
    }

    /// Parses the CLI form `PLATFORM=unix:/absolute/path`,
    /// `PLATFORM=pipe:\\\\.\\pipe\\name`, or `PLATFORM=tcp:127.0.0.1:PORT`.
    pub fn parse_cli(value: &str) -> Result<Self, ProviderRunnerConfigError> {
        let (platform, transport) = value
            .split_once('=')
            .ok_or(ProviderRunnerConfigError::InvalidArgument)?;
        let platform =
            Platform::from_str(platform).map_err(|_| ProviderRunnerConfigError::InvalidArgument)?;
        let transport = if let Some(path) = transport.strip_prefix("unix:") {
            ProviderRunnerTransport::UnixSocket {
                path: PathBuf::from(path),
            }
        } else if let Some(name) = transport.strip_prefix("pipe:") {
            ProviderRunnerTransport::NamedPipe {
                name: name.to_owned(),
            }
        } else if let Some(address) = transport.strip_prefix("tcp:") {
            ProviderRunnerTransport::Tcp {
                address: address
                    .parse()
                    .map_err(|_| ProviderRunnerConfigError::InvalidArgument)?,
            }
        } else {
            return Err(ProviderRunnerConfigError::InvalidArgument);
        };
        let runner = Self {
            platform,
            transport,
        };
        runner.validate()?;
        Ok(runner)
    }

    fn unavailable_reason(&self) -> String {
        let transport = match &self.transport {
            ProviderRunnerTransport::UnixSocket { .. } => "Unix socket",
            ProviderRunnerTransport::NamedPipe { .. } => "named pipe",
            ProviderRunnerTransport::Tcp { .. } => "loopback TCP",
        };
        format!(
            "{transport} runner configured for {}; no execution adapter is installed",
            self.platform.as_str()
        )
    }

    /// Returns the configured loopback TCP endpoint, if this declaration can
    /// use the local HTTP runner protocol.
    ///
    /// Callers must not substitute arbitrary endpoints: this accessor enforces
    /// loopback-only TCP even if a caller constructed the public fields
    /// directly instead of using [`Self::validate`].
    pub fn loopback_tcp_address(&self) -> Option<SocketAddr> {
        match &self.transport {
            ProviderRunnerTransport::Tcp { address } if address.ip().is_loopback() => {
                Some(*address)
            }
            ProviderRunnerTransport::UnixSocket { .. }
            | ProviderRunnerTransport::NamedPipe { .. }
            | ProviderRunnerTransport::Tcp { .. } => None,
        }
    }

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

const CREDENTIAL_LIKE_TERMS: &[&str] = &[
    "cookie",
    "token",
    "password",
    "secret",
    "session",
    "authorization",
    "credential",
];

pub(crate) fn contains_credential_like_term(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    CREDENTIAL_LIKE_TERMS
        .iter()
        .any(|needle| lower.contains(needle))
}

pub(crate) fn reject_credential_like_endpoint(
    value: &str,
) -> Result<(), ProviderRunnerConfigError> {
    if contains_credential_like_term(value)
        || value
            .chars()
            .any(|character| matches!(character, '@' | '?' | '#'))
    {
        return Err(ProviderRunnerConfigError::CredentialLikeEndpoint);
    }
    Ok(())
}

/// Deterministic failure returned when a platform is registered more than once.
///
/// A registry never replaces an existing provider implicitly: replacing an
/// adapter can change the side-effecting backend used for a publication.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProviderRegistrationError {
    #[error("provider already registered for platform: {platform:?}")]
    Duplicate { platform: Platform },
}

/// Per-platform results from one multi-target provider dispatch.
///
/// The map is ordered by [`Platform`], rather than registration or request
/// order, so callers get stable output across runs.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderDispatchReport {
    pub outcomes: BTreeMap<Platform, DispatchOutcome>,
}

/// Explicit registry for installed publication providers.
///
/// The registry contains no browser sessions or credentials. It only owns
/// provider implementations supplied by the embedding application. An absent
/// platform is a normal, explicit unavailable result, never an implied
/// automation capability.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<Platform, Box<dyn PublishProvider>>,
    runners: BTreeMap<Platform, ProviderRunner>,
}

impl ProviderRegistry {
    /// Creates an empty provider registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a registry with validated local runner declarations.
    ///
    /// Loopback-TCP declarations install the stable local runner adapter.
    /// Unix sockets and Windows named pipes remain declared-but-unavailable
    /// until those transports receive an audited implementation.
    pub fn from_runners(
        runners: impl IntoIterator<Item = ProviderRunner>,
    ) -> Result<Self, ProviderRunnerConfigError> {
        let mut registry = Self::new();
        for runner in runners {
            runner.validate()?;
            let platform = runner.platform;
            if registry.runners.insert(platform, runner).is_some() {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
        }
        for (platform, address) in
            registry
                .runners
                .values()
                .filter_map(|runner| match runner.transport {
                    ProviderRunnerTransport::Tcp { address } => Some((runner.platform, address)),
                    _ => None,
                })
        {
            if registry.providers.contains_key(&platform) {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
            registry
                .providers
                .insert(platform, Box::new(TcpRunnerProvider { platform, address }));
        }
        Ok(registry)
    }

    /// Registers a provider without allowing an implicit replacement.
    pub fn register(
        &mut self,
        provider: Box<dyn PublishProvider>,
    ) -> Result<(), ProviderRegistrationError> {
        let platform = provider.platform();
        if self.providers.contains_key(&platform) {
            return Err(ProviderRegistrationError::Duplicate { platform });
        }
        self.providers.insert(platform, provider);
        Ok(())
    }

    /// Returns an installed provider's declared availability.
    pub fn availability(&self, platform: Platform) -> ProviderAvailability {
        self.providers
            .get(&platform)
            .map(|provider| provider.availability())
            .unwrap_or_else(|| ProviderAvailability::Unavailable {
                reason: self.unregistered_reason(platform),
            })
    }

    /// Returns every known platform's availability in canonical platform order.
    pub fn availability_report(&self) -> BTreeMap<Platform, ProviderAvailability> {
        Platform::ALL
            .iter()
            .copied()
            .map(|platform| (platform, self.availability(platform)))
            .collect()
    }

    /// Dispatches one target after proving that target belongs to the request.
    ///
    /// Provider errors are retained as errors. Missing providers and providers
    /// that declare themselves unavailable are ordinary dispatch outcomes, so
    /// callers can safely aggregate partial multi-target results.
    pub fn dispatch(
        &self,
        platform: Platform,
        request: &PublishRequest,
    ) -> Result<DispatchOutcome, DomainError> {
        request.validate()?;
        if !request.targets.contains(&platform) {
            return Err(DomainError::ProviderPlatformNotTarget { platform });
        }

        let Some(provider) = self.providers.get(&platform) else {
            return Ok(DispatchOutcome::Unavailable {
                reason: self.unregistered_reason(platform),
            });
        };

        match provider.availability() {
            ProviderAvailability::Available => provider.enqueue(request),
            ProviderAvailability::Unavailable { reason } => {
                Ok(DispatchOutcome::Unavailable { reason })
            }
        }
    }

    /// Dispatches every requested target and preserves one outcome per platform.
    ///
    /// A malformed request is rejected before any provider is touched. Once the
    /// request is valid, an individual provider failure becomes that target's
    /// rejected outcome and cannot prevent the remaining targets from running.
    pub fn dispatch_all(
        &self,
        request: &PublishRequest,
    ) -> Result<ProviderDispatchReport, DomainError> {
        request.validate()?;
        let mut outcomes = BTreeMap::new();
        for platform in request.targets.iter().copied() {
            let outcome = match self.dispatch(platform, request) {
                Ok(outcome) => outcome,
                Err(error) => DispatchOutcome::Rejected {
                    reason: error.to_string(),
                },
            };
            outcomes.insert(platform, outcome);
        }
        Ok(ProviderDispatchReport { outcomes })
    }

    fn unregistered_reason(&self, platform: Platform) -> String {
        self.runners
            .get(&platform)
            .map(ProviderRunner::unavailable_reason)
            .unwrap_or_else(|| format!("no provider registered for {}", platform.as_str()))
    }
}
