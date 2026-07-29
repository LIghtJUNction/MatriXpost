//! Versioned DTOs for credential-free local runner protocols.

use crate::types::Platform;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProviderAvailability {
    Available,
    Unavailable { reason: String },
}
/// Provider dispatch result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DispatchOutcome {
    Queued { job_id: String },
    Unavailable { reason: String },
    Rejected { reason: String },
}

/// Version of the credential-free, local runner HTTP protocol.
pub const PROVIDER_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Version of the explicit, local-only manual-login navigation protocol.
///
/// This protocol can only ask an already-attached local runner to open a
/// platform page. It never carries browser profiles, cookies, credentials, or
/// a claim that a user has completed login.
pub const LOGIN_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Version of the credential-free terminal QR login protocol.
///
/// This protocol is intentionally separate from [`LoginRunnerRequest`]. It
/// can expose QR image pixels for a user to render in a terminal, but never
/// carries or reports browser state, credentials, routes, or login success.
pub const TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Largest opaque attempt token accepted from a terminal QR runner.
pub const TERMINAL_QR_LOGIN_ATTEMPT_TOKEN_MAX_BYTES: usize = 128;

/// Largest base64-encoded PNG accepted from a terminal QR runner.
///
/// QR codes are small; this cap keeps a compromised loopback runner from
/// making a CLI allocate an unbounded response while still leaving generous
/// room for lossless PNG encoding.
pub const TERMINAL_QR_LOGIN_PNG_BASE64_MAX_BYTES: usize = 1_048_576;

/// Largest complete JSON response accepted from a terminal QR runner.
pub const TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES: usize =
    TERMINAL_QR_LOGIN_PNG_BASE64_MAX_BYTES + 4_096;

/// Version of the credential-free local account-readiness protocol.
pub const ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Version of the credential-free Fanqie review-status protocol.
pub const REVIEW_STATUS_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Maximum UTF-8 byte length for the bounded title lookup used by a review
/// probe. The runner never returns this lookup text or a matching page title.
pub const REVIEW_STATUS_TITLE_QUERY_MAX_BYTES: usize = 200;

/// A deliberately minimal request for a Fanqie video review-status lookup.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStatusRunnerRequest {
    pub version: u16,
    pub platform: Platform,
    pub title_query: String,
}

/// Safe, credential-free result from the Fanqie video-list probe.
///
/// `Published` means a matching list card displayed a published-like state. It
/// does not prove that a provider accepted, processed, or publicly exposed a
/// particular publication request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewStatusRunnerResponse {
    Published { version: u16, platform: Platform },
    UnderReview { version: u16, platform: Platform },
    Rejected { version: u16, platform: Platform },
    NotFound { version: u16, platform: Platform },
    Unavailable { version: u16, platform: Platform },
}

/// Public, reason-free projection of a Fanqie review-status lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Published,
    UnderReview,
    Rejected,
    NotFound,
    Unavailable,
}

impl ReviewStatusRunnerRequest {
    /// Checks the only permitted platform and bounded title-query shape.
    pub fn validate(&self) -> bool {
        self.platform == Platform::FanqieVideo
            && !self.title_query.trim().is_empty()
            && self.title_query.len() <= REVIEW_STATUS_TITLE_QUERY_MAX_BYTES
    }
}

pub(crate) fn normalize_review_title_query(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

impl ReviewStatusRunnerResponse {
    pub(crate) fn into_review_status(self) -> Option<ReviewStatus> {
        match self {
            Self::Published { version, platform }
                if version == REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == Platform::FanqieVideo =>
            {
                Some(ReviewStatus::Published)
            }
            Self::UnderReview { version, platform }
                if version == REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == Platform::FanqieVideo =>
            {
                Some(ReviewStatus::UnderReview)
            }
            Self::Rejected { version, platform }
                if version == REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == Platform::FanqieVideo =>
            {
                Some(ReviewStatus::Rejected)
            }
            Self::NotFound { version, platform }
                if version == REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == Platform::FanqieVideo =>
            {
                Some(ReviewStatus::NotFound)
            }
            Self::Unavailable { version, platform }
                if version == REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == Platform::FanqieVideo =>
            {
                Some(ReviewStatus::Unavailable)
            }
            _ => None,
        }
    }
}

/// A deliberately minimal request for an inferred upload-page readiness probe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountStatusRunnerRequest {
    pub version: u16,
    pub platform: Platform,
}

/// Safe readiness result from a local runner. It is an upload-form inference,
/// never an assertion about credentials, cookies, or a browser session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountStatusRunnerResponse {
    Ready { version: u16, platform: Platform },
    NotReady { version: u16, platform: Platform },
    Unavailable { version: u16, platform: Platform },
    Rejected { version: u16, platform: Platform },
}

/// Public, credential-free projection of an account readiness probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountReadiness {
    Ready,
    NotReady,
    Unavailable,
    Rejected,
}

impl AccountStatusRunnerResponse {
    pub(crate) fn into_readiness(self, expected_platform: Platform) -> Option<AccountReadiness> {
        match self {
            Self::Ready { version, platform }
                if version == ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == expected_platform =>
            {
                Some(AccountReadiness::Ready)
            }
            Self::NotReady { version, platform }
                if version == ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == expected_platform =>
            {
                Some(AccountReadiness::NotReady)
            }
            Self::Unavailable { version, platform }
                if version == ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == expected_platform =>
            {
                Some(AccountReadiness::Unavailable)
            }
            Self::Rejected { version, platform }
                if version == ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION
                    && platform == expected_platform =>
            {
                Some(AccountReadiness::Rejected)
            }
            _ => None,
        }
    }
}

/// Request sent to an explicitly configured local runner to open a platform
/// page for a user to complete login manually.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRunnerRequest {
    pub version: u16,
    pub platform: Platform,
}

/// Minimal request for a local terminal QR login attempt.
///
/// Only Douyin and WeChat Channels are supported. The request does not carry
/// a token because any attempt association stays inside the local runner and
/// its loopback client boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalQrLoginRunnerRequest {
    pub version: u16,
    pub platform: Platform,
}

impl TerminalQrLoginRunnerRequest {
    /// Checks the version and the two platforms that expose terminal QR flow.
    pub const fn validate(&self) -> bool {
        self.version == TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION
            && matches!(self.platform, Platform::Douyin | Platform::WechatChannels)
    }
}

/// Attempt-scoped request for a local terminal QR refresh.
///
/// The opaque token is valid only within the runner's local loopback-client
/// boundary. It never identifies a browser profile or authenticated session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalQrLoginRefreshRequest {
    pub version: u16,
    pub platform: Platform,
    pub attempt_token: String,
}

impl TerminalQrLoginRefreshRequest {
    /// Checks the protocol version, supported platform, and opaque token.
    pub fn validate(&self) -> bool {
        terminal_qr_request_matches(self.version, self.platform)
            && terminal_qr_attempt_token_is_valid(&self.attempt_token)
    }
}

/// Attempt-scoped request to cancel a local terminal QR attempt.
///
/// Cancellation only discards the runner's local attempt state; it never
/// changes a browser session or a platform login state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalQrLoginCancelRequest {
    pub version: u16,
    pub platform: Platform,
    pub attempt_token: String,
}

impl TerminalQrLoginCancelRequest {
    /// Checks the protocol version, supported platform, and opaque token.
    pub fn validate(&self) -> bool {
        terminal_qr_request_matches(self.version, self.platform)
            && terminal_qr_attempt_token_is_valid(&self.attempt_token)
    }
}

/// Credential-free state emitted by a terminal QR login runner.
///
/// `QrAvailable` contains only the QR PNG pixels and a bounded opaque attempt
/// token. It never means that login completed. `Pending`, `TimedOut`, and
/// `Cancelled` likewise report only the local attempt state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalQrLoginRunnerResponse {
    QrAvailable {
        version: u16,
        platform: Platform,
        attempt_token: String,
        png_base64: String,
    },
    Pending {
        version: u16,
        platform: Platform,
        attempt_token: String,
    },
    TimedOut {
        version: u16,
        platform: Platform,
    },
    Cancelled {
        version: u16,
        platform: Platform,
    },
    Unavailable {
        version: u16,
        platform: Platform,
    },
    Rejected {
        version: u16,
        platform: Platform,
    },
}

/// Validated local state for a terminal QR login attempt.
///
/// No variant claims that a browser session exists or that a user completed
/// login. Callers must use a separate, credential-free readiness check after
/// the user has acted on the QR code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalQrLoginOutcome {
    QrAvailable {
        attempt_token: String,
        png_base64: String,
    },
    Pending {
        attempt_token: String,
    },
    TimedOut,
    Cancelled,
    Unavailable,
    Rejected,
}

impl TerminalQrLoginRunnerResponse {
    pub(crate) fn into_terminal_qr_login(
        self,
        expected_platform: Platform,
    ) -> Option<TerminalQrLoginOutcome> {
        match self {
            Self::QrAvailable {
                version,
                platform,
                attempt_token,
                png_base64,
            } if terminal_qr_response_matches(version, platform, expected_platform)
                && terminal_qr_attempt_token_is_valid(&attempt_token)
                && terminal_qr_png_base64_is_valid(&png_base64) =>
            {
                Some(TerminalQrLoginOutcome::QrAvailable {
                    attempt_token,
                    png_base64,
                })
            }
            Self::Pending {
                version,
                platform,
                attempt_token,
            } if terminal_qr_response_matches(version, platform, expected_platform)
                && terminal_qr_attempt_token_is_valid(&attempt_token) =>
            {
                Some(TerminalQrLoginOutcome::Pending { attempt_token })
            }
            Self::TimedOut { version, platform }
                if terminal_qr_response_matches(version, platform, expected_platform) =>
            {
                Some(TerminalQrLoginOutcome::TimedOut)
            }
            Self::Cancelled { version, platform }
                if terminal_qr_response_matches(version, platform, expected_platform) =>
            {
                Some(TerminalQrLoginOutcome::Cancelled)
            }
            Self::Unavailable { version, platform }
                if terminal_qr_response_matches(version, platform, expected_platform) =>
            {
                Some(TerminalQrLoginOutcome::Unavailable)
            }
            Self::Rejected { version, platform }
                if terminal_qr_response_matches(version, platform, expected_platform) =>
            {
                Some(TerminalQrLoginOutcome::Rejected)
            }
            _ => None,
        }
    }
}

fn terminal_qr_response_matches(
    version: u16,
    platform: Platform,
    expected_platform: Platform,
) -> bool {
    version == TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION
        && platform == expected_platform
        && matches!(platform, Platform::Douyin | Platform::WechatChannels)
}

const fn terminal_qr_request_matches(version: u16, platform: Platform) -> bool {
    version == TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION
        && matches!(platform, Platform::Douyin | Platform::WechatChannels)
}

fn terminal_qr_attempt_token_is_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= TERMINAL_QR_LOGIN_ATTEMPT_TOKEN_MAX_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn terminal_qr_png_base64_is_valid(value: &str) -> bool {
    const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

    if value.len() < 12
        || value.len() > TERMINAL_QR_LOGIN_PNG_BASE64_MAX_BYTES
        || !value.len().is_multiple_of(4)
    {
        return false;
    }

    let mut signature = [0_u8; 8];
    let mut written = 0;
    let bytes = value.as_bytes();
    if let Some(first_padding) = bytes.iter().position(|&byte| byte == b'=')
        && bytes[first_padding..].iter().any(|&byte| byte != b'=')
    {
        return false;
    }
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let Some(a) = base64_value(chunk[0]) else {
            return false;
        };
        let Some(b) = base64_value(chunk[1]) else {
            return false;
        };
        let padding = chunk.iter().skip(2).filter(|&&byte| byte == b'=').count();
        if padding > 2
            || (padding > 0 && index + 1 != bytes.len() / 4)
            || (padding == 2 && chunk[2] != b'=')
        {
            return false;
        }
        let c = if chunk[2] == b'=' {
            0
        } else if let Some(value) = base64_value(chunk[2]) {
            value
        } else {
            return false;
        };
        let d = if chunk[3] == b'=' {
            0
        } else if let Some(value) = base64_value(chunk[3]) {
            value
        } else {
            return false;
        };
        let decoded = [(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d];
        for byte in decoded.into_iter().take(3 - padding) {
            if written < signature.len() {
                signature[written] = byte;
                written += 1;
            }
        }
    }
    written >= PNG_SIGNATURE.len() && signature == PNG_SIGNATURE
}

const fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Result returned by a local manual-login navigation request.
///
/// `Opened` confirms only that the runner navigated its already-attached
/// browser to the platform page. The user must still complete login manually.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoginRunnerResponse {
    Opened {
        version: u16,
        platform: Platform,
        /// Always true: navigation does not prove that the user completed login.
        manual_login_required: bool,
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

/// Validated result of asking a configured local runner to open its manual
/// login page. None of these variants assert that the user completed login.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualLoginOutcome {
    /// The runner opened its already-attached browser for manual login.
    Opened,
    /// No usable local loopback-TCP runner is configured or it is unavailable.
    Unavailable,
    /// The local runner response was rejected, malformed, or could not be read.
    Rejected,
}

impl LoginRunnerResponse {
    pub(crate) fn into_manual_login(
        self,
        expected_platform: Platform,
    ) -> Option<ManualLoginOutcome> {
        match self {
            Self::Opened {
                version,
                platform,
                manual_login_required: true,
            } if version == LOGIN_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ManualLoginOutcome::Opened)
            }
            Self::Unavailable {
                version, platform, ..
            } if version == LOGIN_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ManualLoginOutcome::Unavailable)
            }
            Self::Rejected {
                version, platform, ..
            } if version == LOGIN_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ManualLoginOutcome::Rejected)
            }
            _ => None,
        }
    }
}
