//! Generic lifecycle and durable queue model types.

use crate::{
    error::DomainError,
    types::{LocalSchedule, Platform, PublishRequest, PublishState},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};
use thiserror::Error;

/// An immutable publication-history entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub request: PublishRequest,
    pub state: PublishState,
    pub recorded_at: DateTime<Utc>,
    pub detail: Option<String>,
}

/// A serialization-safe publication-history projection for public interfaces.
///
/// Durable history retains the original request for local lifecycle and retry
/// handling. This view deliberately exposes only the fields safe for CLI, MCP,
/// and desktop presentation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicationHistoryEntry {
    pub id: String,
    pub state: &'static str,
    pub recorded_at: String,
    pub title: String,
    pub targets: Vec<String>,
    pub draft: bool,
    pub scheduled: bool,
}

impl From<&HistoryRecord> for PublicationHistoryEntry {
    fn from(record: &HistoryRecord) -> Self {
        Self {
            id: record.id.clone(),
            state: publication_state_label(record.state),
            recorded_at: record.recorded_at.to_rfc3339(),
            title: record.request.title.clone(),
            targets: record
                .request
                .targets
                .iter()
                .map(|platform| platform.as_str().to_owned())
                .collect(),
            draft: record.request.draft,
            scheduled: record.state == PublishState::Queued
                && record.request.scheduled_at.is_some(),
        }
    }
}

impl From<HistoryRecord> for PublicationHistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        let scheduled =
            record.state == PublishState::Queued && record.request.scheduled_at.is_some();
        Self {
            id: record.id,
            state: publication_state_label(record.state),
            recorded_at: record.recorded_at.to_rfc3339(),
            title: record.request.title,
            targets: record
                .request
                .targets
                .into_iter()
                .map(|platform| platform.as_str().to_owned())
                .collect(),
            draft: record.request.draft,
            scheduled,
        }
    }
}

const fn publication_state_label(state: PublishState) -> &'static str {
    match state {
        PublishState::Draft => "draft",
        PublishState::Queued => "queued",
        PublishState::Dispatching => "dispatching",
        PublishState::Published => "published",
        PublishState::Failed => "failed",
        PublishState::Unavailable => "unavailable",
    }
}

/// The lifecycle phase of a generic business object.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessObjectStatus {
    Draft,
    Active,
    Completed,
    Archived,
}

impl BusinessObjectStatus {
    /// Returns whether a lifecycle update from `self` to `next` is legal.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Active | Self::Archived)
                | (Self::Active, Self::Completed | Self::Archived)
                | (Self::Completed, Self::Archived)
        )
    }

    pub(crate) fn db(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Archived => "archived",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, DomainError> {
        match value {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "completed" => Ok(Self::Completed),
            "archived" => Ok(Self::Archived),
            _ => Err(DomainError::CorruptState(format!(
                "business object status: {value}"
            ))),
        }
    }
}

/// Approval state shared by business objects and financial entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

impl ApprovalStatus {
    /// Returns whether an approval update from `self` to `next` is legal.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Approved | Self::Rejected) | (Self::Rejected, Self::Pending)
        )
    }

    pub(crate) fn db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, DomainError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            _ => Err(DomainError::CorruptState(format!(
                "approval status: {value}"
            ))),
        }
    }
}

/// A configurable business object tracked throughout its lifecycle.
///
/// `kind` selects a caller-defined template such as `asset`, `campaign`, or
/// `project`; it is deliberately not a fixed domain enum. `external_id` is an
/// optional identifier unique within that kind. Attribute keys resembling
/// credential names are rejected, while values remain generic business text
/// and are not content-scanned; callers must never supply credentials as
/// values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BusinessObject {
    pub id: String,
    pub kind: String,
    pub external_id: Option<String>,
    pub display_name: String,
    pub lifecycle_status: BusinessObjectStatus,
    pub approval_status: ApprovalStatus,
    /// Monotonically increasing version used to reject conflicting updates.
    #[serde(default)]
    pub revision: u64,
    pub attributes: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Whether a ledger entry represents a cost or income.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerDirection {
    Expense,
    Revenue,
}

impl LedgerDirection {
    pub(crate) fn db(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Revenue => "revenue",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, DomainError> {
        match value {
            "expense" => Ok(Self::Expense),
            "revenue" => Ok(Self::Revenue),
            _ => Err(DomainError::CorruptState(format!(
                "ledger direction: {value}"
            ))),
        }
    }
}

/// An immutable, money-safe entry in a business object's ledger.
///
/// Amounts are stored in the smallest unit of `currency` (for example, cents)
/// and therefore never use floating-point values. Corrections are represented
/// by a separate entry rather than an update or delete operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,
    pub business_object_id: String,
    pub direction: LedgerDirection,
    pub category: String,
    pub amount_minor: i64,
    pub currency: String,
    pub occurred_at: DateTime<Utc>,
    pub approval_status: ApprovalStatus,
    pub counterparty: Option<String>,
    pub reference: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A durable link from published content history to a generic business object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentAttribution {
    pub business_object_id: String,
    pub history_id: String,
    pub created_at: DateTime<Utc>,
}

/// An immutable, directed link between two generic business objects.
///
/// `relation_type` is caller-defined, allowing applications to model links
/// such as an asset's customer interest or a supplier's service without
/// hard-coding any vertical-specific relationship. Attribute keys follow the
/// same credential-safe policy as [`BusinessObject`] attributes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BusinessRelation {
    pub id: String,
    pub source_business_object_id: String,
    pub target_business_object_id: String,
    pub relation_type: String,
    pub attributes: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

/// The publication states accepted by the upstream history query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryStatus {
    Success,
    Failed,
    Publishing,
    Scheduled,
}

impl FromStr for HistoryStatus {
    type Err = HistoryFilterError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            "publishing" => Ok(Self::Publishing),
            "scheduled" => Ok(Self::Scheduled),
            _ => Err(HistoryFilterError::InvalidStatus(value.to_owned())),
        }
    }
}

/// A validated, deterministic local publication-history query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryFilter {
    cutoff: Option<DateTime<Utc>>,
    platform: Option<Platform>,
    status: Option<HistoryStatus>,
}

impl HistoryFilter {
    /// Builds a query from the upstream trailing-days form using a caller-supplied clock.
    pub fn from_query(
        days: Option<u16>,
        all: bool,
        platform: Option<Platform>,
        status: Option<HistoryStatus>,
        now: DateTime<Utc>,
    ) -> Result<Self, HistoryFilterError> {
        let cutoff = if all {
            None
        } else {
            let days = days.unwrap_or(7);
            if days == 0 {
                return Err(HistoryFilterError::NonPositiveDays);
            }
            Some(now - ChronoDuration::days(i64::from(days)))
        };
        Ok(Self {
            cutoff,
            platform,
            status,
        })
    }

    /// Retains matching records in their original order.
    pub fn filter(&self, history: Vec<HistoryRecord>) -> Vec<HistoryRecord> {
        history
            .into_iter()
            .filter(|record| self.matches(record))
            .collect()
    }

    /// Tests one record without changing the query or record.
    pub fn matches(&self, record: &HistoryRecord) -> bool {
        self.cutoff
            .is_none_or(|cutoff| record.recorded_at >= cutoff)
            && self
                .platform
                .is_none_or(|platform| record.request.targets.contains(&platform))
            && self
                .status
                .is_none_or(|status| status.matches(record.state))
    }
}

impl HistoryStatus {
    fn matches(self, state: PublishState) -> bool {
        match self {
            Self::Success => state == PublishState::Published,
            Self::Failed => state == PublishState::Failed,
            Self::Publishing => state == PublishState::Dispatching,
            Self::Scheduled => state == PublishState::Queued,
        }
    }
}

/// Errors raised while constructing a history query from user input.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HistoryFilterError {
    #[error("days must be greater than zero unless all is true")]
    NonPositiveDays,
    #[error("status must be success, failed, publishing, or scheduled")]
    InvalidStatus(String),
}

/// A scheduled durable job; `revision` makes transitions deterministic under retries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub request: PublishRequest,
    pub state: PublishState,
    pub due_at: Option<LocalSchedule>,
    pub revision: u64,
    pub updated_at: DateTime<Utc>,
}
