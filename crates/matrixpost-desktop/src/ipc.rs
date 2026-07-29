use chrono::Utc;
use tauri::Manager;

use crate::{
    AccountReadinessInput, AccountReadinessReport, AccountSaved, AddLifecycleBusinessRelationInput,
    AddLifecycleContentAttributionInput, AppendLifecycleLedgerEntryInput, ArticleAccountSaved,
    CreateLifecycleObjectInput, DesktopError, DesktopService, DesktopSnapshot,
    DispatchToLocalRunnerInput, DraftSaved, FanqieReviewStatusInput, FanqieReviewStatusReport,
    HistoryEntry, HistoryQueryInput, LifecycleBusinessRelationEntry,
    LifecycleContentAttributionEntry, LifecycleLedgerEntry, LifecycleObjectEntry,
    LifecycleObjectIdInput, LocalRunnerDispatchReport, SaveAccountInput, SaveArticleAccountInput,
    SaveDraftInput, TransitionLifecycleObjectInput,
};

/// Managed Tauri state; the service itself has no dependency on Tauri.
pub struct DesktopState {
    service: DesktopService,
}

#[tauri::command]
fn desktop_snapshot(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopSnapshot, DesktopError> {
    state.service.snapshot()
}

#[tauri::command]
fn save_local_draft(
    state: tauri::State<'_, DesktopState>,
    input: SaveDraftInput,
) -> Result<DraftSaved, DesktopError> {
    state.service.save_local_draft(input)
}

#[tauri::command]
fn dispatch_to_local_runner(
    state: tauri::State<'_, DesktopState>,
    input: DispatchToLocalRunnerInput,
) -> Result<LocalRunnerDispatchReport, DesktopError> {
    state.service.dispatch_to_local_runner(input)
}

#[tauri::command]
fn account_readiness(
    state: tauri::State<'_, DesktopState>,
    input: AccountReadinessInput,
) -> Result<AccountReadinessReport, DesktopError> {
    state.service.account_readiness(input)
}

#[tauri::command]
fn fanqie_review_status(
    state: tauri::State<'_, DesktopState>,
    input: FanqieReviewStatusInput,
) -> Result<FanqieReviewStatusReport, DesktopError> {
    state.service.fanqie_review_status(input)
}

#[tauri::command]
fn save_account(
    state: tauri::State<'_, DesktopState>,
    input: SaveAccountInput,
) -> Result<AccountSaved, DesktopError> {
    state.service.save_account(input)
}

#[tauri::command]
fn save_article_account(
    state: tauri::State<'_, DesktopState>,
    input: SaveArticleAccountInput,
) -> Result<ArticleAccountSaved, DesktopError> {
    state.service.save_article_account(input)
}

#[tauri::command]
fn local_history(
    state: tauri::State<'_, DesktopState>,
    input: HistoryQueryInput,
) -> Result<Vec<HistoryEntry>, DesktopError> {
    state.service.history_entries(input, Utc::now())
}

#[tauri::command]
fn lifecycle_objects(
    state: tauri::State<'_, DesktopState>,
) -> Result<Vec<LifecycleObjectEntry>, DesktopError> {
    state.service.lifecycle_objects()
}

#[tauri::command]
fn create_lifecycle_object(
    state: tauri::State<'_, DesktopState>,
    input: CreateLifecycleObjectInput,
) -> Result<LifecycleObjectEntry, DesktopError> {
    state.service.create_lifecycle_object(input)
}

#[tauri::command]
fn lifecycle_ledger_entries(
    state: tauri::State<'_, DesktopState>,
    input: LifecycleObjectIdInput,
) -> Result<Vec<LifecycleLedgerEntry>, DesktopError> {
    state
        .service
        .lifecycle_ledger_entries(input.business_object_id)
}

#[tauri::command]
fn append_lifecycle_ledger_entry(
    state: tauri::State<'_, DesktopState>,
    input: AppendLifecycleLedgerEntryInput,
) -> Result<LifecycleLedgerEntry, DesktopError> {
    state.service.append_lifecycle_ledger_entry(input)
}

#[tauri::command]
fn lifecycle_content_attributions(
    state: tauri::State<'_, DesktopState>,
    input: LifecycleObjectIdInput,
) -> Result<Vec<LifecycleContentAttributionEntry>, DesktopError> {
    state
        .service
        .lifecycle_content_attributions(input.business_object_id)
}

#[tauri::command]
fn add_lifecycle_content_attribution(
    state: tauri::State<'_, DesktopState>,
    input: AddLifecycleContentAttributionInput,
) -> Result<LifecycleContentAttributionEntry, DesktopError> {
    state.service.add_lifecycle_content_attribution(input)
}

#[tauri::command]
fn lifecycle_business_relations(
    state: tauri::State<'_, DesktopState>,
    input: LifecycleObjectIdInput,
) -> Result<Vec<LifecycleBusinessRelationEntry>, DesktopError> {
    state
        .service
        .lifecycle_business_relations(input.business_object_id)
}

#[tauri::command]
fn add_lifecycle_business_relation(
    state: tauri::State<'_, DesktopState>,
    input: AddLifecycleBusinessRelationInput,
) -> Result<LifecycleBusinessRelationEntry, DesktopError> {
    state.service.add_lifecycle_business_relation(input)
}

#[tauri::command]
fn transition_lifecycle_object(
    state: tauri::State<'_, DesktopState>,
    input: TransitionLifecycleObjectInput,
) -> Result<LifecycleObjectEntry, DesktopError> {
    state.service.transition_lifecycle_object(input)
}

/// Starts the platform-native shell. All UI access is through Tauri IPC.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let state_path = directory.join("matrixpost.db");
            app.manage(DesktopState {
                service: DesktopService::open(state_path)?,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_snapshot,
            save_local_draft,
            dispatch_to_local_runner,
            account_readiness,
            fanqie_review_status,
            save_account,
            save_article_account,
            local_history,
            lifecycle_objects,
            create_lifecycle_object,
            lifecycle_ledger_entries,
            append_lifecycle_ledger_entry,
            lifecycle_content_attributions,
            add_lifecycle_content_attribution,
            lifecycle_business_relations,
            add_lifecycle_business_relation,
            transition_lifecycle_object
        ])
        .run(tauri::generate_context!())
        .expect("error while running MatriXpost desktop");
}
