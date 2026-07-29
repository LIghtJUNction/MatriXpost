//! Explicit local runner adapter for supported article publication.

use super::provider::reject_credential_like_endpoint;
use crate::{
    error::DomainError,
    types::{ArticlePlatform, PublishArticleRequest},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use thiserror::Error;

/// Version of the credential-free local article runner HTTP protocol.
pub const ARTICLE_RUNNER_PROTOCOL_VERSION: u16 = 1;

/// Request sent to a local article runner.
///
/// The request contains only the validated article command. Browser profile,
/// session, and credential configuration remain outside this protocol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleRunnerRequest {
    pub version: u16,
    pub request: PublishArticleRequest,
}

/// Explicit result returned by a local article runner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArticleRunnerResponse {
    Queued {
        version: u16,
        platform: ArticlePlatform,
        job_id: String,
        automation_attempted: bool,
    },
    Unavailable {
        version: u16,
        platform: ArticlePlatform,
        reason: String,
        automation_attempted: bool,
    },
    Rejected {
        version: u16,
        platform: ArticlePlatform,
        reason: String,
        automation_attempted: bool,
    },
}

/// A validated article runner response, suitable for a future embedding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ArticleDispatchOutcome {
    Queued {
        job_id: String,
    },
    Unavailable {
        reason: String,
    },
    Rejected {
        reason: String,
        automation_attempted: bool,
    },
}

impl ArticleRunnerResponse {
    /// Converts only a response matching the version, platform, and outcome
    /// invariants of this protocol.
    pub fn into_dispatch(
        self,
        expected_platform: ArticlePlatform,
    ) -> Option<ArticleDispatchOutcome> {
        match self {
            Self::Queued {
                version,
                platform,
                job_id,
                automation_attempted: true,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION
                && platform == expected_platform
                && !job_id.trim().is_empty() =>
            {
                Some(ArticleDispatchOutcome::Queued { job_id })
            }
            Self::Unavailable {
                version,
                platform,
                reason,
                automation_attempted: false,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ArticleDispatchOutcome::Unavailable { reason })
            }
            Self::Rejected {
                version,
                platform,
                reason,
                automation_attempted,
            } if version == ARTICLE_RUNNER_PROTOCOL_VERSION && platform == expected_platform => {
                Some(ArticleDispatchOutcome::Rejected {
                    reason,
                    automation_attempted,
                })
            }
            _ => None,
        }
    }
}

/// Credential-free, loopback-only endpoint for the Juejin article runner.
///
/// This is deliberately separate from video provider runners: article
/// publication has a distinct protocol and is not scheduled by MatriXpost.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleRunner {
    pub address: SocketAddr,
}

/// Invalid article-runner configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ArticleRunnerConfigError {
    #[error("article runner TCP address must bind to loopback")]
    TcpMustBeLoopback,
    #[error("article runner endpoint must not contain credential-like data")]
    CredentialLikeEndpoint,
    #[error("article runner argument must use tcp:127.0.0.1:PORT")]
    InvalidArgument,
}

/// Injectable HTTP boundary for deterministic article-runner adapter tests.
pub trait ArticleRunnerHttpTransport {
    /// POSTs a JSON document and returns the HTTP status and response body.
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ArticleRunnerTransportError>;
}

/// Non-sensitive failure classification for the local article-runner transport.
///
/// This intentionally carries no endpoint, body, response, or credential data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArticleRunnerTransportError {
    /// The local HTTP request could not complete.
    RequestFailed,
    /// The local runner response could not be read.
    ResponseReadFailed,
}

struct UreqArticleRunnerHttpTransport;

impl ArticleRunnerHttpTransport for UreqArticleRunnerHttpTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ArticleRunnerTransportError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let response = agent
            .post(endpoint)
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|_| ArticleRunnerTransportError::RequestFailed)?;
        let status = response.status();
        let body = response
            .into_string()
            .map_err(|_| ArticleRunnerTransportError::ResponseReadFailed)?;
        Ok((status, body))
    }
}

impl ArticleRunner {
    /// Parses the explicit local form `tcp:127.0.0.1:PORT`.
    pub fn parse_cli(value: &str) -> Result<Self, ArticleRunnerConfigError> {
        reject_credential_like_endpoint(value)
            .map_err(|_| ArticleRunnerConfigError::CredentialLikeEndpoint)?;
        let address = value
            .strip_prefix("tcp:")
            .ok_or(ArticleRunnerConfigError::InvalidArgument)?
            .parse()
            .map_err(|_| ArticleRunnerConfigError::InvalidArgument)?;
        let runner = Self { address };
        runner.validate()?;
        Ok(runner)
    }

    /// Ensures the endpoint cannot target a remote host.
    pub fn validate(&self) -> Result<(), ArticleRunnerConfigError> {
        if self.address.ip().is_loopback() {
            Ok(())
        } else {
            Err(ArticleRunnerConfigError::TcpMustBeLoopback)
        }
    }

    fn rejected(automation_attempted: bool) -> ArticleDispatchOutcome {
        ArticleDispatchOutcome::Rejected {
            reason: "local article runner did not return a valid accepted response".into(),
            automation_attempted,
        }
    }

    /// Dispatches an unscheduled article through the versioned local protocol.
    ///
    /// A queued response only proves local runner completion; it never confirms
    /// that Juejin processed or published the article.
    pub fn dispatch(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<ArticleDispatchOutcome, DomainError> {
        self.dispatch_with(request, &UreqArticleRunnerHttpTransport)
    }

    /// Same as [`Self::dispatch`] with an injected HTTP transport.
    pub fn dispatch_with<T: ArticleRunnerHttpTransport>(
        &self,
        request: &PublishArticleRequest,
        transport: &T,
    ) -> Result<ArticleDispatchOutcome, DomainError> {
        request.validate()?;
        if request.scheduled_at.is_some() {
            return Ok(ArticleDispatchOutcome::Rejected {
                reason: "scheduled article dispatch is not supported".into(),
                automation_attempted: false,
            });
        }
        let expected_platform = request.article_platform()?;
        let endpoint = format!("http://{}/v1/publish-article", self.address);
        let payload = ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: request.runner_safe(),
        };
        let payload = serde_json::to_string(&payload).map_err(DomainError::serialization)?;
        let (status, body) = match transport.post_json(&endpoint, &payload) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected(true)),
        };
        if status != 200 {
            return Ok(Self::rejected(true));
        }
        let response: ArticleRunnerResponse = match serde_json::from_str(&body) {
            Ok(response) => response,
            Err(_) => return Ok(Self::rejected(true)),
        };
        Ok(response
            .into_dispatch(expected_platform)
            .unwrap_or_else(|| Self::rejected(true)))
    }
}
