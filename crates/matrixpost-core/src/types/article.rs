use super::{AccountSelection, ArticlePlatform, LocalSchedule};
use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed article command retained independently from video publication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishArticleRequest {
    pub platform: String,
    #[serde(default, skip_serializing_if = "AccountSelection::is_empty")]
    pub account: AccountSelection,
    pub title: String,
    pub content: Option<String>,
    pub file: Option<PathBuf>,
    pub cover: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub scheduled_at: Option<LocalSchedule>,
}

impl PublishArticleRequest {
    /// Returns a copy safe to cross a local runner boundary.
    pub fn runner_safe(&self) -> Self {
        let mut safe = self.clone();
        safe.account = AccountSelection::default();
        safe
    }

    /// Returns true if this request still carries account-routing data.
    pub const fn has_account_routing(&self) -> bool {
        !self.account.is_empty()
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.title.trim().is_empty() {
            return Err(DomainError::EmptyTitle);
        }
        if self
            .content
            .as_deref()
            .is_none_or(|item| item.trim().is_empty())
            && self.file.is_none()
        {
            return Err(DomainError::EmptyArticleContent);
        }
        if let Some(schedule) = &self.scheduled_at {
            schedule.as_naive()?;
        }
        self.article_platform()?;
        Ok(())
    }

    /// Returns the only article platform supported by this protocol.
    pub fn article_platform(&self) -> Result<ArticlePlatform, DomainError> {
        match self.platform.trim().to_ascii_lowercase().as_str() {
            "juejin" | "掘金" => Ok(ArticlePlatform::Juejin),
            _ => Err(DomainError::UnknownPlatform(self.platform.clone())),
        }
    }
}

/// Durable state for one scheduled article dispatch. Article jobs are kept
/// separate from video jobs because their runner protocol and terminal history
/// are intentionally independent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleScheduledJob {
    pub id: String,
    pub request: PublishArticleRequest,
    pub state: PublishState,
    pub due_at: LocalSchedule,
    pub revision: u64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Immutable terminal evidence for a scheduled article's local runner
/// workflow. It is not evidence of remote Juejin publication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArticleHistoryRecord {
    pub id: String,
    /// The supported article platform. Account routing is intentionally absent.
    pub platform: ArticlePlatform,
    /// The requested article title. Body, files, runner endpoints, and account
    /// routes are deliberately excluded from durable history.
    pub title: String,
    pub state: PublishState,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// A fixed generic workflow outcome, never runner-provided diagnostics.
    pub detail: Option<String>,
}

/// Durable state for one publication job.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishState {
    Draft,
    Queued,
    Dispatching,
    Published,
    Failed,
    Unavailable,
}

impl PublishState {
    /// The finite transition graph enforced by the scheduler.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Queued | Self::Unavailable)
                | (
                    Self::Queued,
                    Self::Dispatching | Self::Failed | Self::Unavailable
                )
                | (
                    Self::Dispatching,
                    Self::Published | Self::Failed | Self::Unavailable
                )
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                from: self,
                to: next,
            })
        }
    }

    pub(crate) fn db(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, DomainError> {
        match value {
            "draft" => Ok(Self::Draft),
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "published" => Ok(Self::Published),
            "failed" => Ok(Self::Failed),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(DomainError::CorruptState(value.to_owned())),
        }
    }
}
