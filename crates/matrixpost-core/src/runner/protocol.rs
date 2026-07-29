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
