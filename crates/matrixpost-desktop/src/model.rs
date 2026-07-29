//! Local-only Tauri adapter for the credential-free MatriXpost core.
//!
//! The desktop process owns its SQLite state in the operating system's
//! application-data directory. It never starts the daemon, a shell, a browser,
//! or a provider adapter.

use std::collections::BTreeMap;

use matrixpost_core::{ApprovalStatus, BusinessObjectStatus, LedgerDirection, PlatformMetadata};
use serde::{Deserialize, Serialize};

/// Values shown by the desktop overview. All account data is credential-free.
#[derive(Debug, Serialize)]
pub struct DesktopSnapshot {
    pub platforms: Vec<PlatformMetadata>,
    pub accounts: Vec<AccountEntry>,
    pub article_accounts: Vec<ArticleAccountEntry>,
    pub history_count: usize,
    pub provider_automation_available: bool,
}

/// Video-account metadata safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct AccountEntry {
    pub id: String,
    pub platform: &'static str,
    pub display_name: String,
    pub status: &'static str,
}

/// Small input surface deliberately limited to creating a local video draft.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDraftInput {
    pub title: String,
    pub media_path: String,
    pub targets: Vec<String>,
    pub scheduled_at: Option<String>,
}

/// The durable result of a draft save, with no implication of remote dispatch.
#[derive(Debug, Serialize)]
pub struct DraftSaved {
    pub id: String,
    pub state: &'static str,
    pub remote_publish_attempted: bool,
}

/// Strict one-shot request for already-running, loopback-only provider runners.
///
/// Runner declarations are intentionally supplied for this invocation only;
/// the desktop application neither persists them nor manages browser sessions.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchToLocalRunnerInput {
    pub title: String,
    pub media_path: String,
    pub targets: Vec<String>,
    pub scheduled_at: Option<String>,
    pub provider_runners: Vec<String>,
    /// Explicit confirmation for this immediate, one-shot local dispatch.
    pub confirmed: bool,
}

/// Credential-free summary of one local runner result.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRunnerDispatchOutcome {
    pub platform: &'static str,
    pub state: &'static str,
    pub reason: String,
}

/// One-shot local runner result, never a claim of remote platform publication.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRunnerDispatchReport {
    pub outcomes: Vec<LocalRunnerDispatchOutcome>,
    pub remote_publish_confirmed: bool,
}

/// Explicit, one-shot readiness probe for a separately started local runner.
///
/// The runner declaration is never persisted. Without one, the result is
/// deliberately `unavailable` rather than an attempt to discover a runner.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountReadinessInput {
    pub platform: String,
    pub provider_runner: Option<String>,
    pub confirmed: bool,
}

/// Safe, reason-free account readiness result for the desktop UI.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct AccountReadinessReport {
    pub state: &'static str,
}

/// Explicit, one-shot Fanqie review probe for a separately started local
/// runner. The bounded title is never returned or persisted.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FanqieReviewStatusInput {
    pub title_query: String,
    pub provider_runner: Option<String>,
    pub confirmed: bool,
}

/// Safe, reason-free Fanqie review-status result for the desktop UI.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct FanqieReviewStatusReport {
    pub state: &'static str,
}

/// Credential-free account metadata accepted from the local desktop form.
///
/// `phone` and `partition` are the existing upstream routing fields. They are
/// required by the durable account model, but never treated as authentication
/// material by this application.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAccountInput {
    pub platform: String,
    pub display_name: String,
    pub status: String,
    pub phone: String,
    pub partition: String,
}

/// The local result of saving safe account metadata.
#[derive(Debug, Serialize)]
pub struct AccountSaved {
    pub id: String,
}

/// Strict, credential-free Juejin article-account metadata from the desktop UI.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveArticleAccountInput {
    pub display_name: String,
    pub status: String,
    pub phone: String,
    pub partition: String,
}

/// Article-account metadata safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct ArticleAccountEntry {
    pub id: String,
    pub display_name: String,
    pub status: &'static str,
}

/// Local result of saving Juejin account metadata.
#[derive(Debug, Serialize)]
pub struct ArticleAccountSaved {
    pub id: String,
    pub status: &'static str,
}

/// Strict, local-only history filtering accepted through Tauri IPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryQueryInput {
    pub days: Option<u16>,
    #[serde(default)]
    pub all: bool,
    pub platform: Option<String>,
    pub status: Option<String>,
}

/// A history record safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub state: &'static str,
    pub recorded_at: String,
    pub title: String,
    pub targets: Vec<String>,
    pub draft: bool,
    pub scheduled: bool,
}

/// Strict, generic lifecycle-object creation input accepted through local IPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateLifecycleObjectInput {
    pub id: String,
    pub kind: String,
    pub external_id: Option<String>,
    pub display_name: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// Strict object identifier input used by object-scoped lifecycle reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleObjectIdInput {
    pub business_object_id: String,
}

/// Lifecycle and approval states accepted from the desktop UI.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatusInput {
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

/// Approval states accepted from the desktop UI.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleApprovalStatusInput {
    Pending,
    Approved,
    Rejected,
}

impl From<LifecycleApprovalStatusInput> for ApprovalStatus {
    fn from(value: LifecycleApprovalStatusInput) -> Self {
        match value {
            LifecycleApprovalStatusInput::Pending => Self::Pending,
            LifecycleApprovalStatusInput::Approved => Self::Approved,
            LifecycleApprovalStatusInput::Rejected => Self::Rejected,
        }
    }
}

/// Ledger direction accepted from the desktop UI.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleLedgerDirectionInput {
    Expense,
    Revenue,
}

impl From<LifecycleLedgerDirectionInput> for LedgerDirection {
    fn from(value: LifecycleLedgerDirectionInput) -> Self {
        match value {
            LifecycleLedgerDirectionInput::Expense => Self::Expense,
            LifecycleLedgerDirectionInput::Revenue => Self::Revenue,
        }
    }
}

/// Strict input for appending one immutable local ledger entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendLifecycleLedgerEntryInput {
    pub id: String,
    pub business_object_id: String,
    pub direction: LifecycleLedgerDirectionInput,
    pub category: String,
    pub amount_minor: i64,
    pub currency: String,
    pub approval_status: Option<LifecycleApprovalStatusInput>,
    pub counterparty: Option<String>,
    pub reference: Option<String>,
    pub description: Option<String>,
}

/// Strict input for linking existing local publication history to an object.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddLifecycleContentAttributionInput {
    pub business_object_id: String,
    pub history_id: String,
}

/// Strict input for creating an immutable directed relation between local objects.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddLifecycleBusinessRelationInput {
    pub id: String,
    pub source_business_object_id: String,
    pub target_business_object_id: String,
    pub relation_type: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// Strict input for an optimistic lifecycle/approval state transition.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransitionLifecycleObjectInput {
    pub id: String,
    pub expected_revision: u64,
    pub lifecycle_status: LifecycleStatusInput,
    pub approval_status: LifecycleApprovalStatusInput,
}

/// Credential-free projection of a generic lifecycle object.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleObjectEntry {
    pub id: String,
    pub kind: String,
    pub external_id: Option<String>,
    pub display_name: String,
    pub lifecycle_status: &'static str,
    pub approval_status: &'static str,
    pub revision: u64,
    pub attributes: BTreeMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Credential-free projection of one immutable lifecycle ledger entry.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleLedgerEntry {
    pub id: String,
    pub business_object_id: String,
    pub direction: &'static str,
    pub category: String,
    pub amount_minor: i64,
    pub currency: String,
    pub occurred_at: String,
    pub approval_status: &'static str,
    pub counterparty: Option<String>,
    pub reference: Option<String>,
    pub description: Option<String>,
    pub created_at: String,
}

/// Credential-free projection of a publication-history attribution.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleContentAttributionEntry {
    pub business_object_id: String,
    pub history_id: String,
    pub created_at: String,
}

/// Credential-free projection of one immutable, directed business relation.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleBusinessRelationEntry {
    pub id: String,
    pub source_business_object_id: String,
    pub target_business_object_id: String,
    pub relation_type: String,
    pub attributes: BTreeMap<String, String>,
    pub created_at: String,
}
