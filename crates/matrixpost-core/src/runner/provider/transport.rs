use crate::{
    error::DomainError,
    runner::{DispatchOutcome, PROVIDER_RUNNER_PROTOCOL_VERSION, ProviderAvailability},
    types::{Platform, PublishRequest},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};

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
