//! Local-only Tauri adapter for the credential-free MatriXpost core.
//!
//! The desktop process owns its SQLite state in the operating system's
//! application-data directory. It never starts the daemon, a shell, a browser,
//! or a provider adapter.

mod error;
mod ipc;
mod model;
mod projection;
mod runner;
mod service;

#[cfg(test)]
#[path = "tests/service.rs"]
mod tests;

pub use error::DesktopError;
pub use ipc::{DesktopState, run};
pub use model::{
    AccountEntry, AccountReadinessInput, AccountReadinessReport, AccountSaved,
    AddLifecycleBusinessRelationInput, AddLifecycleContentAttributionInput,
    AppendLifecycleLedgerEntryInput, ArticleAccountEntry, ArticleAccountSaved,
    CreateLifecycleObjectInput, DesktopSnapshot, DispatchToLocalRunnerInput, DraftSaved,
    FanqieReviewStatusInput, FanqieReviewStatusReport, HistoryEntry, HistoryQueryInput,
    LifecycleApprovalStatusInput, LifecycleBusinessRelationEntry, LifecycleContentAttributionEntry,
    LifecycleLedgerDirectionInput, LifecycleLedgerEntry, LifecycleObjectEntry,
    LifecycleObjectIdInput, LifecycleStatusInput, LocalRunnerDispatchOutcome,
    LocalRunnerDispatchReport, SaveAccountInput, SaveArticleAccountInput, SaveDraftInput,
    TransitionLifecycleObjectInput,
};
pub use service::DesktopService;
