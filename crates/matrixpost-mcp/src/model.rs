use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use matrixpost_core::{
    ApprovalStatus, BusinessObjectStatus, HistoryStatus, LedgerDirection, LocalSchedule, Platform,
    PublishState, ReviewStatus,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Exact upstream account-query platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AccountsPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
    Xhs,
    Juejin,
    Fqsp,
}

/// Exact upstream video-publication platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VideoPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
}

/// Exact upstream article-publication platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ArticlePlatformInput {
    Juejin,
}

/// Exact documented history filter set; Fanqie is intentionally absent.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
    Xhs,
}

/// Exact upstream publication-state filter set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryStatusInput {
    Success,
    Failed,
    Publishing,
    Scheduled,
}

impl From<HistoryStatusInput> for HistoryStatus {
    fn from(value: HistoryStatusInput) -> Self {
        match value {
            HistoryStatusInput::Success => Self::Success,
            HistoryStatusInput::Failed => Self::Failed,
            HistoryStatusInput::Publishing => Self::Publishing,
            HistoryStatusInput::Scheduled => Self::Scheduled,
        }
    }
}

/// The only accepted account-list filter from the upstream MCP contract.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListAccountsInput {
    /// Optional exact upstream platform code.
    pub(crate) platform: Option<AccountsPlatform>,
}

/// The upstream-compatible history query. Filters operate only on local state.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListHistoryInput {
    /// Number of trailing days to return; defaults to seven unless `all` is true.
    pub(crate) days: Option<u16>,
    /// Optional exact upstream platform code.
    pub(crate) platform: Option<HistoryPlatform>,
    /// One of `success`, `failed`, `publishing`, or `scheduled`.
    pub(crate) status: Option<HistoryStatusInput>,
    /// When true, do not apply the default trailing-seven-day filter.
    pub(crate) all: Option<bool>,
}

/// Read-only terminal scheduled-article workflow history.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListArticleHistoryInput {}

/// Bounded Fanqie title lookup. The tool returns only a finite status label;
/// it never returns the submitted title or any page content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewFanqieStatusInput {
    /// A bounded title fragment used only inside the local runner.
    pub(crate) title: String,
}

/// Video-link metadata accepted by MatrixMedia's WeChat Channels request form.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum SphLinkInput {
    /// Explicitly disables a link.
    None {},
    /// Links to a product and therefore requires its provider value.
    Product { value: String },
}

/// Upstream-compatible video publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublishVideoInput {
    /// One exact upstream video platform code.
    pub(crate) platform: VideoPlatform,
    /// Local absolute video path or an `http`/`https` URL.
    pub(crate) file: String,
    /// Publication title.
    pub(crate) title: String,
    /// Account phone/partition selector used only as local routing metadata.
    pub(crate) phone: String,
    /// Upstream secondary-title field.
    pub(crate) bt2: Option<String>,
    /// Comma- or whitespace-separated tags.
    pub(crate) tags: Option<String>,
    /// Optional publication address.
    pub(crate) address: Option<String>,
    /// Upstream schedule in `YYYY-MM-DD HH:MM` or `YYYY-MM-DD HH:MM:SS` form.
    pub(crate) publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    pub(crate) show: Option<bool>,
    /// Record the job as a draft instead of a queued local intent.
    pub(crate) draft: Option<bool>,
    /// Optional platform-specific creative declaration.
    pub(crate) creative_statement: Option<String>,
    /// WeChat Channels product identifier.
    pub(crate) sph_product_id: Option<String>,
    /// WeChat Channels link data.
    pub(crate) sph_link: Option<SphLinkInput>,
}

/// Upstream-compatible Juejin article publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublishArticleInput {
    /// The only upstream MCP article target: `juejin`.
    pub(crate) platform: ArticlePlatformInput,
    /// Account phone/partition selector used only as local routing metadata.
    pub(crate) phone: String,
    /// Article title.
    pub(crate) title: String,
    /// Inline article body; required when `file` is omitted.
    pub(crate) content: Option<String>,
    /// Markdown file path; required when `content` is omitted.
    pub(crate) file: Option<String>,
    /// Optional cover image path.
    pub(crate) cover: Option<String>,
    /// Optional Juejin category.
    pub(crate) category: Option<String>,
    /// Upstream single-string tags, normalized into the typed core vector.
    pub(crate) tags: Option<String>,
    /// Optional article summary.
    pub(crate) summary: Option<String>,
    /// CLI-compatible `HH:MM`, `YYYY-MM-DD HH:MM`, or `YYYY-MM-DD HH:MM:SS` schedule.
    pub(crate) publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    pub(crate) show: Option<bool>,
}

/// Generic lifecycle state accepted at the MCP boundary.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleStatusInput {
    Draft,
    Active,
    Completed,
    Archived,
}

impl From<LifecycleStatusInput> for BusinessObjectStatus {
    fn from(value: LifecycleStatusInput) -> Self {
        match value {
            LifecycleStatusInput::Draft => Self::Draft,
            LifecycleStatusInput::Active => Self::Active,
            LifecycleStatusInput::Completed => Self::Completed,
            LifecycleStatusInput::Archived => Self::Archived,
        }
    }
}

/// Shared approval state accepted at the MCP boundary.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovalStatusInput {
    Pending,
    Approved,
    Rejected,
}

impl From<ApprovalStatusInput> for ApprovalStatus {
    fn from(value: ApprovalStatusInput) -> Self {
        match value {
            ApprovalStatusInput::Pending => Self::Pending,
            ApprovalStatusInput::Approved => Self::Approved,
            ApprovalStatusInput::Rejected => Self::Rejected,
        }
    }
}

/// Ledger direction accepted at the MCP boundary.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LedgerDirectionInput {
    Expense,
    Revenue,
}

impl From<LedgerDirectionInput> for LedgerDirection {
    fn from(value: LedgerDirectionInput) -> Self {
        match value {
            LedgerDirectionInput::Expense => Self::Expense,
            LedgerDirectionInput::Revenue => Self::Revenue,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GetBusinessObjectInput {
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListBusinessObjectsInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateBusinessObjectInput {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) display_name: String,
    pub(crate) external_id: Option<String>,
    pub(crate) lifecycle_status: Option<LifecycleStatusInput>,
    pub(crate) approval_status: Option<ApprovalStatusInput>,
    #[schemars(with = "Option<BTreeMap<String, String>>")]
    pub(crate) attributes: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListLedgerEntriesInput {
    pub(crate) business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppendLedgerEntryInput {
    pub(crate) id: String,
    pub(crate) business_object_id: String,
    pub(crate) direction: LedgerDirectionInput,
    pub(crate) category: String,
    pub(crate) amount_minor: i64,
    pub(crate) currency: String,
    pub(crate) approval_status: Option<ApprovalStatusInput>,
    pub(crate) occurred_at: Option<DateTime<Utc>>,
    pub(crate) counterparty: Option<String>,
    pub(crate) reference: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListContentAttributionsInput {
    pub(crate) business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AddContentAttributionInput {
    pub(crate) business_object_id: String,
    pub(crate) history_id: String,
    pub(crate) created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListBusinessRelationsInput {
    pub(crate) business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AddBusinessRelationInput {
    pub(crate) id: String,
    pub(crate) source_business_object_id: String,
    pub(crate) target_business_object_id: String,
    pub(crate) relation_type: String,
    #[schemars(with = "Option<BTreeMap<String, String>>")]
    pub(crate) attributes: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransitionBusinessObjectInput {
    pub(crate) id: String,
    pub(crate) expected_revision: u64,
    pub(crate) lifecycle_status: LifecycleStatusInput,
    pub(crate) approval_status: ApprovalStatusInput,
    pub(crate) updated_at: Option<DateTime<Utc>>,
}

/// Exact upstream account-list item contract.
#[derive(Debug, Serialize)]
pub(crate) struct ListedAccount {
    pub(crate) phone: String,
    pub(crate) platform: &'static str,
    pub(crate) partition: String,
}

/// The non-sensitive, durable part of a locally queued video job.
#[derive(Debug, Serialize)]
pub(crate) struct JobResult {
    pub(crate) id: String,
    pub(crate) state: PublishState,
    pub(crate) due_at: Option<LocalSchedule>,
    pub(crate) revision: u64,
}

/// Explicit provider boundary returned by both publication tools.
#[derive(Debug, Serialize)]
pub(crate) struct PublicationResult {
    pub(crate) outcome: &'static str,
    pub(crate) provider_available: bool,
    pub(crate) remote_publish_attempted: bool,
    pub(crate) persisted: bool,
    pub(crate) job: Option<JobResult>,
    pub(crate) providers: Option<BTreeMap<Platform, SafeProviderOutcome>>,
    pub(crate) message: &'static str,
}

/// Reason-free result of a Fanqie local review-status lookup.
#[derive(Debug, Serialize)]
pub(crate) struct ReviewStatusResult {
    pub(crate) outcome: &'static str,
    pub(crate) platform: &'static str,
    pub(crate) message: &'static str,
}

impl ReviewStatusResult {
    pub(crate) fn unavailable() -> Self {
        Self {
            outcome: "unavailable",
            platform: "fqsp",
            message: "no local Fanqie runner is configured; no browser review-status probe was attempted",
        }
    }

    pub(crate) fn rejected() -> Self {
        Self {
            outcome: "rejected",
            platform: "fqsp",
            message: "the local Fanqie review-status probe was rejected; no remote publication success is claimed",
        }
    }
}

impl From<ReviewStatus> for ReviewStatusResult {
    fn from(status: ReviewStatus) -> Self {
        let (outcome, message) = match status {
            ReviewStatus::Published => (
                "published",
                "a matching local video-list card is published-like; this does not prove remote publication acceptance",
            ),
            ReviewStatus::UnderReview => (
                "under_review",
                "a matching local video-list card is under review; no remote publication success is claimed",
            ),
            ReviewStatus::Rejected => (
                "rejected",
                "the local Fanqie review-status probe was rejected; no remote publication success is claimed",
            ),
            ReviewStatus::NotFound => (
                "not_found",
                "no matching local video-list card was found; no remote publication success is claimed",
            ),
            ReviewStatus::Unavailable => (
                "unavailable",
                "the local Fanqie runner is unavailable; no browser review-status probe was completed",
            ),
        };
        Self {
            outcome,
            platform: "fqsp",
            message,
        }
    }
}

/// A reason-free local runner status for one requested platform.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SafeProviderOutcome {
    Queued,
    Unavailable,
    Rejected,
}

/// Typed, inspectable validation result for an MCP tool call.
#[derive(Debug, Serialize)]
pub(crate) struct ToolFailure {
    pub(crate) outcome: &'static str,
    pub(crate) code: &'static str,
    pub(crate) message: String,
}
