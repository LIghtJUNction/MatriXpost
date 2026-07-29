//! Local-only Tauri adapter for the credential-free MatriXpost core.
//!
//! The desktop process owns its SQLite state in the operating system's
//! application-data directory. It never starts the daemon, a shell, a browser,
//! or a provider adapter.

use std::{collections::BTreeMap, path::PathBuf, str::FromStr, sync::Arc};

use chrono::Utc;
use matrixpost_core::{
    Account, AccountStatus, ApprovalStatus, ArticleAccount, ArticleAccountStatus, ArticlePlatform,
    BusinessObject, BusinessObjectStatus, BusinessRelation, ContentAttribution, DomainError,
    HistoryFilter, HistoryRecord, HistoryStatus, LedgerDirection, LedgerEntry, LifecycleRepository,
    LocalSchedule, MediaSource, Platform, PlatformMetadata, PublicationQueue, PublishRequest,
    PublishState, Repository, SqliteRepository,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use thiserror::Error;

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

/// IPC-safe error returned to the static frontend.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum DesktopError {
    #[error("invalid local draft: {0}")]
    InvalidRequest(String),
    #[error("local lifecycle record was not found: {0}")]
    NotFound(String),
    #[error("local state is unavailable: {0}")]
    Storage(String),
}

impl From<DomainError> for DesktopError {
    fn from(error: DomainError) -> Self {
        Self::InvalidRequest(error.to_string())
    }
}

/// Testable local application service, independent of the Tauri runtime.
#[derive(Clone)]
pub struct DesktopService {
    repository: Arc<SqliteRepository>,
}

impl DesktopService {
    pub fn new(repository: Arc<SqliteRepository>) -> Self {
        Self { repository }
    }

    pub fn open(state_path: PathBuf) -> Result<Self, DesktopError> {
        SqliteRepository::open(state_path)
            .map(|repository| Self::new(Arc::new(repository)))
            .map_err(|error| DesktopError::Storage(error.to_string()))
    }

    pub fn snapshot(&self) -> Result<DesktopSnapshot, DesktopError> {
        Ok(DesktopSnapshot {
            platforms: Platform::ALL
                .iter()
                .copied()
                .map(Platform::metadata)
                .collect(),
            accounts: self
                .repository
                .accounts()?
                .into_iter()
                .map(AccountEntry::from)
                .collect(),
            article_accounts: self
                .repository
                .article_accounts()?
                .into_iter()
                .map(ArticleAccountEntry::from)
                .collect(),
            history_count: self.repository.history()?.len(),
            provider_automation_available: false,
        })
    }

    pub fn save_local_draft(&self, input: SaveDraftInput) -> Result<DraftSaved, DesktopError> {
        let request = PublishRequest {
            source: MediaSource::LocalFile(PathBuf::from(input.media_path.trim())),
            title: input.title,
            short_title: None,
            tags: Vec::new(),
            address: None,
            // This adapter is intentionally unable to create a queued job.
            draft: true,
            bt2: None,
            scheduled_at: input
                .scheduled_at
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(LocalSchedule::parse)
                .transpose()?,
            task_name: None,
            account: Default::default(),
            wechat_link: Default::default(),
            overrides: Vec::new(),
            targets: input
                .targets
                .iter()
                .map(|target| Platform::from_str(target))
                .collect::<Result<Vec<_>, _>>()?,
        };
        request.validate()?;
        let job = PublicationQueue::enqueue(self.repository.as_ref(), &request, Utc::now())?;
        debug_assert_eq!(job.state, matrixpost_core::PublishState::Draft);
        Ok(DraftSaved {
            id: job.id,
            state: "draft",
            remote_publish_attempted: false,
        })
    }

    pub fn save_account(&self, input: SaveAccountInput) -> Result<AccountSaved, DesktopError> {
        let platform = Platform::from_str(&input.platform)?;
        let display_name = input.display_name.trim();
        if display_name.is_empty() {
            return Err(DesktopError::InvalidRequest(
                "account display name cannot be empty".into(),
            ));
        }
        let status = match input.status.trim() {
            "logged_in" => AccountStatus::LoggedIn,
            "expired" => AccountStatus::Expired,
            "logged_out" => AccountStatus::LoggedOut,
            "unavailable" => AccountStatus::Unavailable,
            value => {
                return Err(DesktopError::InvalidRequest(format!(
                    "unknown account status: {value}"
                )));
            }
        };
        let id = account_id(platform, display_name);
        let account = Account {
            id: id.clone(),
            platform,
            display_name: display_name.to_owned(),
            status,
            phone: input.phone.trim().to_owned(),
            partition: input.partition.trim().to_owned(),
        };
        self.repository.save_account(&account)?;
        Ok(AccountSaved { id })
    }

    pub fn save_article_account(
        &self,
        input: SaveArticleAccountInput,
    ) -> Result<ArticleAccountSaved, DesktopError> {
        let display_name = input.display_name.trim();
        if display_name.is_empty() {
            return Err(DesktopError::InvalidRequest(
                "article account display name cannot be empty".into(),
            ));
        }
        let status = article_account_status(&input.status)?;
        let id = article_account_id(display_name);
        self.repository.save_article_account(&ArticleAccount {
            id: id.clone(),
            platform: ArticlePlatform::Juejin,
            display_name: display_name.to_owned(),
            status,
            phone: input.phone.trim().to_owned(),
            partition: input.partition.trim().to_owned(),
        })?;
        Ok(ArticleAccountSaved {
            id,
            status: article_account_status_label(status),
        })
    }

    pub fn history_entries(
        &self,
        input: HistoryQueryInput,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<HistoryEntry>, DesktopError> {
        let platform = input
            .platform
            .as_deref()
            .map(Platform::from_str)
            .transpose()?;
        let status = input
            .status
            .as_deref()
            .map(HistoryStatus::from_str)
            .transpose()
            .map_err(|error| DesktopError::InvalidRequest(error.to_string()))?;
        let filter = HistoryFilter::from_query(input.days, input.all, platform, status, now)
            .map_err(|error| DesktopError::InvalidRequest(error.to_string()))?;

        Ok(filter
            .filter(self.repository.history()?)
            .into_iter()
            .map(HistoryEntry::from)
            .collect())
    }

    /// Lists generic lifecycle objects from the same local SQLite state as the
    /// publishing history. This deliberately has no provider or browser path.
    pub fn lifecycle_objects(&self) -> Result<Vec<LifecycleObjectEntry>, DesktopError> {
        self.repository
            .business_objects()
            .map_err(lifecycle_error)
            .map(|objects| {
                objects
                    .into_iter()
                    .map(LifecycleObjectEntry::from)
                    .collect()
            })
    }

    /// Creates a generic lifecycle object at revision zero with system time.
    pub fn create_lifecycle_object(
        &self,
        input: CreateLifecycleObjectInput,
    ) -> Result<LifecycleObjectEntry, DesktopError> {
        let now = Utc::now();
        let object = BusinessObject {
            id: input.id,
            kind: input.kind,
            external_id: input.external_id,
            display_name: input.display_name,
            lifecycle_status: BusinessObjectStatus::Draft,
            approval_status: ApprovalStatus::Pending,
            revision: 0,
            attributes: input.attributes,
            created_at: now,
            updated_at: now,
        };
        self.repository
            .insert_business_object(&object)
            .map_err(lifecycle_error)?;
        Ok(LifecycleObjectEntry::from(object))
    }

    /// Lists immutable ledger entries for a generic lifecycle object.
    pub fn lifecycle_ledger_entries(
        &self,
        business_object_id: String,
    ) -> Result<Vec<LifecycleLedgerEntry>, DesktopError> {
        self.repository
            .ledger_entries(&business_object_id)
            .map_err(lifecycle_error)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(LifecycleLedgerEntry::from)
                    .collect()
            })
    }

    /// Appends an immutable ledger entry using the system UTC clock.
    pub fn append_lifecycle_ledger_entry(
        &self,
        input: AppendLifecycleLedgerEntryInput,
    ) -> Result<LifecycleLedgerEntry, DesktopError> {
        let now = Utc::now();
        let entry = LedgerEntry {
            id: input.id,
            business_object_id: input.business_object_id,
            direction: input.direction.into(),
            category: input.category,
            amount_minor: input.amount_minor,
            currency: input.currency,
            occurred_at: now,
            approval_status: input
                .approval_status
                .unwrap_or(LifecycleApprovalStatusInput::Pending)
                .into(),
            counterparty: input.counterparty,
            reference: input.reference,
            description: input.description,
            created_at: now,
        };
        self.repository
            .insert_ledger_entry(&entry)
            .map_err(lifecycle_error)?;
        Ok(LifecycleLedgerEntry::from(entry))
    }

    /// Lists existing local publication-history links for a lifecycle object.
    pub fn lifecycle_content_attributions(
        &self,
        business_object_id: String,
    ) -> Result<Vec<LifecycleContentAttributionEntry>, DesktopError> {
        self.repository
            .content_attributions(&business_object_id)
            .map_err(lifecycle_error)
            .map(|attributions| {
                attributions
                    .into_iter()
                    .map(LifecycleContentAttributionEntry::from)
                    .collect()
            })
    }

    /// Links an existing local history record to an object using system UTC time.
    pub fn add_lifecycle_content_attribution(
        &self,
        input: AddLifecycleContentAttributionInput,
    ) -> Result<LifecycleContentAttributionEntry, DesktopError> {
        let attribution = ContentAttribution {
            business_object_id: input.business_object_id,
            history_id: input.history_id,
            created_at: Utc::now(),
        };
        self.repository
            .insert_content_attribution(&attribution)
            .map_err(lifecycle_error)?;
        Ok(LifecycleContentAttributionEntry::from(attribution))
    }

    /// Lists both inbound and outbound immutable relations for a local object.
    pub fn lifecycle_business_relations(
        &self,
        business_object_id: String,
    ) -> Result<Vec<LifecycleBusinessRelationEntry>, DesktopError> {
        self.repository
            .business_relations(&business_object_id)
            .map_err(lifecycle_error)
            .map(|relations| {
                relations
                    .into_iter()
                    .map(LifecycleBusinessRelationEntry::from)
                    .collect()
            })
    }

    /// Creates an immutable relation using the system UTC clock.
    pub fn add_lifecycle_business_relation(
        &self,
        input: AddLifecycleBusinessRelationInput,
    ) -> Result<LifecycleBusinessRelationEntry, DesktopError> {
        let relation = BusinessRelation {
            id: input.id,
            source_business_object_id: input.source_business_object_id,
            target_business_object_id: input.target_business_object_id,
            relation_type: input.relation_type,
            attributes: input.attributes,
            created_at: Utc::now(),
        };
        self.repository
            .insert_business_relation(&relation)
            .map_err(lifecycle_error)?;
        Ok(LifecycleBusinessRelationEntry::from(relation))
    }

    /// Performs an optimistic lifecycle and approval transition using system UTC time.
    pub fn transition_lifecycle_object(
        &self,
        input: TransitionLifecycleObjectInput,
    ) -> Result<LifecycleObjectEntry, DesktopError> {
        self.repository
            .transition_business_object(
                &input.id,
                input.expected_revision,
                input.lifecycle_status.into(),
                input.approval_status.into(),
                Utc::now(),
            )
            .map_err(lifecycle_error)
            .map(LifecycleObjectEntry::from)
    }
}

fn lifecycle_error(error: DomainError) -> DesktopError {
    match error {
        DomainError::UnknownBusinessObject(_) | DomainError::UnknownHistoryRecord(_) => {
            DesktopError::NotFound("the requested lifecycle record does not exist".into())
        }
        _ => DesktopError::InvalidRequest("lifecycle request could not be completed".into()),
    }
}

impl From<BusinessObject> for LifecycleObjectEntry {
    fn from(object: BusinessObject) -> Self {
        Self {
            id: object.id,
            kind: object.kind,
            external_id: object.external_id,
            display_name: object.display_name,
            lifecycle_status: lifecycle_status_label(object.lifecycle_status),
            approval_status: approval_status_label(object.approval_status),
            revision: object.revision,
            attributes: object.attributes,
            created_at: object.created_at.to_rfc3339(),
            updated_at: object.updated_at.to_rfc3339(),
        }
    }
}

impl From<LedgerEntry> for LifecycleLedgerEntry {
    fn from(entry: LedgerEntry) -> Self {
        Self {
            id: entry.id,
            business_object_id: entry.business_object_id,
            direction: ledger_direction_label(entry.direction),
            category: entry.category,
            amount_minor: entry.amount_minor,
            currency: entry.currency,
            occurred_at: entry.occurred_at.to_rfc3339(),
            approval_status: approval_status_label(entry.approval_status),
            counterparty: entry.counterparty,
            reference: entry.reference,
            description: entry.description,
            created_at: entry.created_at.to_rfc3339(),
        }
    }
}

impl From<ContentAttribution> for LifecycleContentAttributionEntry {
    fn from(attribution: ContentAttribution) -> Self {
        Self {
            business_object_id: attribution.business_object_id,
            history_id: attribution.history_id,
            created_at: attribution.created_at.to_rfc3339(),
        }
    }
}

impl From<BusinessRelation> for LifecycleBusinessRelationEntry {
    fn from(relation: BusinessRelation) -> Self {
        Self {
            id: relation.id,
            source_business_object_id: relation.source_business_object_id,
            target_business_object_id: relation.target_business_object_id,
            relation_type: relation.relation_type,
            attributes: relation.attributes,
            created_at: relation.created_at.to_rfc3339(),
        }
    }
}

impl From<HistoryRecord> for HistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        let scheduled =
            record.state == PublishState::Queued && record.request.scheduled_at.is_some();
        Self {
            id: record.id,
            state: publish_state_label(record.state),
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

impl From<ArticleAccount> for ArticleAccountEntry {
    fn from(account: ArticleAccount) -> Self {
        Self {
            id: account.id,
            display_name: account.display_name,
            status: article_account_status_label(account.status),
        }
    }
}

impl From<Account> for AccountEntry {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            platform: account.platform.as_str(),
            display_name: account.display_name,
            status: account_status_label(account.status),
        }
    }
}

const fn account_status_label(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::LoggedIn => "logged_in",
        AccountStatus::Expired => "expired",
        AccountStatus::LoggedOut => "logged_out",
        AccountStatus::Unavailable => "unavailable",
    }
}

fn article_account_status(value: &str) -> Result<ArticleAccountStatus, DesktopError> {
    match value.trim() {
        "logged_in" => Ok(ArticleAccountStatus::LoggedIn),
        "expired" => Ok(ArticleAccountStatus::Expired),
        "logged_out" => Ok(ArticleAccountStatus::LoggedOut),
        "unavailable" => Ok(ArticleAccountStatus::Unavailable),
        value => Err(DesktopError::InvalidRequest(format!(
            "unknown article account status: {value}"
        ))),
    }
}

const fn article_account_status_label(status: ArticleAccountStatus) -> &'static str {
    match status {
        ArticleAccountStatus::LoggedIn => "logged_in",
        ArticleAccountStatus::Expired => "expired",
        ArticleAccountStatus::LoggedOut => "logged_out",
        ArticleAccountStatus::Unavailable => "unavailable",
    }
}

const fn publish_state_label(state: PublishState) -> &'static str {
    match state {
        PublishState::Draft => "draft",
        PublishState::Queued => "queued",
        PublishState::Dispatching => "dispatching",
        PublishState::Published => "published",
        PublishState::Failed => "failed",
        PublishState::Unavailable => "unavailable",
    }
}

const fn lifecycle_status_label(status: BusinessObjectStatus) -> &'static str {
    match status {
        BusinessObjectStatus::Draft => "draft",
        BusinessObjectStatus::Active => "active",
        BusinessObjectStatus::Completed => "completed",
        BusinessObjectStatus::Archived => "archived",
    }
}

const fn approval_status_label(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    }
}

const fn ledger_direction_label(direction: LedgerDirection) -> &'static str {
    match direction {
        LedgerDirection::Expense => "expense",
        LedgerDirection::Revenue => "revenue",
    }
}

fn account_id(platform: Platform, display_name: &str) -> String {
    format!("{}-{}", platform.as_str(), account_slug(display_name))
}

fn article_account_id(display_name: &str) -> String {
    format!("juejin-{}", account_slug(display_name))
}

fn account_slug(display_name: &str) -> String {
    let slug = display_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "account".into()
    } else {
        slug.into()
    }
}

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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use chrono::{Duration, TimeZone, Utc};
    use matrixpost_core::{
        AccountSelection, HistoryRecord, LocalSchedule, MediaSource, PublishRequest, PublishState,
        Repository, SqliteRepository,
    };
    use serde::Deserialize;
    use serde::de::value::{
        BoolDeserializer, Error as ValueError, MapDeserializer, StringDeserializer,
    };

    use super::{
        AddLifecycleBusinessRelationInput, AddLifecycleContentAttributionInput,
        AppendLifecycleLedgerEntryInput, CreateLifecycleObjectInput, DesktopService,
        HistoryQueryInput, LifecycleApprovalStatusInput, LifecycleLedgerDirectionInput,
        LifecycleObjectIdInput, LifecycleStatusInput, SaveAccountInput, SaveArticleAccountInput,
        SaveDraftInput, TransitionLifecycleObjectInput,
    };

    fn service() -> DesktopService {
        DesktopService::new(Arc::new(
            SqliteRepository::in_memory().expect("in-memory state"),
        ))
    }

    fn history_input(
        days: Option<u16>,
        all: bool,
        platform: Option<&str>,
        status: Option<&str>,
    ) -> HistoryQueryInput {
        HistoryQueryInput {
            days,
            all,
            platform: platform.map(str::to_owned),
            status: status.map(str::to_owned),
        }
    }

    fn history_record(
        id: &str,
        title: &str,
        platform: matrixpost_core::Platform,
        state: PublishState,
        recorded_at: chrono::DateTime<Utc>,
        draft: bool,
        scheduled: bool,
    ) -> HistoryRecord {
        HistoryRecord {
            id: id.into(),
            request: PublishRequest {
                source: MediaSource::LocalFile("/private/video.mp4".into()),
                title: title.into(),
                short_title: None,
                tags: Vec::new(),
                address: None,
                draft,
                bt2: None,
                scheduled_at: scheduled.then(|| LocalSchedule("2030-01-02 03:04:05".into())),
                task_name: None,
                account: AccountSelection {
                    phone: Some("private-route".into()),
                    partition: Some("persist:private".into()),
                },
                wechat_link: Default::default(),
                overrides: Vec::new(),
                targets: vec![platform],
            },
            state,
            recorded_at,
            detail: Some("private detail".into()),
        }
    }

    #[test]
    fn snapshot_is_credential_free_and_reports_unavailable_providers() {
        let snapshot = service().snapshot().expect("snapshot");

        assert_eq!(snapshot.platforms.len(), 8);
        assert!(snapshot.accounts.is_empty());
        assert!(snapshot.article_accounts.is_empty());
        assert_eq!(snapshot.history_count, 0);
        assert!(!snapshot.provider_automation_available);
    }

    #[test]
    fn saving_a_draft_forces_draft_state_without_remote_dispatch() {
        let service = service();
        let saved = service
            .save_local_draft(SaveDraftInput {
                title: "Local planning only".into(),
                media_path: "/media/example.mp4".into(),
                targets: vec!["dy".into()],
                scheduled_at: None,
            })
            .expect("local draft");

        assert_eq!(saved.state, "draft");
        assert!(!saved.remote_publish_attempted);
        let job = service
            .repository
            .job(&saved.id)
            .expect("job lookup")
            .expect("saved job");
        assert_eq!(job.state, PublishState::Draft);
    }

    #[test]
    fn saving_account_metadata_persists_without_credentials() {
        let service = service();
        let saved = service
            .save_account(SaveAccountInput {
                platform: "dy".into(),
                display_name: "Studio account".into(),
                status: "logged_out".into(),
                phone: "route-01".into(),
                partition: "persist:studio".into(),
            })
            .expect("safe account metadata");

        assert_eq!(saved.id, "dy-studio-account");
        assert_eq!(
            service.snapshot().expect("snapshot").accounts,
            vec![super::AccountEntry {
                id: saved.id,
                platform: "dy",
                display_name: "Studio account".into(),
                status: "logged_out",
            }]
        );
        let rendered = format!("{:?}", service.snapshot().expect("snapshot").accounts);
        assert!(!rendered.contains("route-01"));
        assert!(!rendered.contains("persist:studio"));
    }

    #[test]
    fn saving_account_rejects_invalid_routing_metadata() {
        let error = service()
            .save_account(SaveAccountInput {
                platform: "dy".into(),
                display_name: "Studio account".into(),
                status: "logged_out".into(),
                phone: "".into(),
                partition: "not-a-partition".into(),
            })
            .expect_err("invalid route must fail");

        assert!(
            error
                .to_string()
                .contains("partition must start with persist:")
        );
    }

    #[test]
    fn account_input_rejects_secret_named_unknown_fields() {
        let input = [
            ("platform", "dy"),
            ("displayName", "Studio account"),
            ("status", "logged_out"),
            ("phone", "route-01"),
            ("partition", "persist:studio"),
            ("password", "must-not-be-accepted"),
        ]
        .into_iter()
        .map(|(key, value)| {
            (
                StringDeserializer::<ValueError>::new(key.to_owned()),
                StringDeserializer::<ValueError>::new(value.to_owned()),
            )
        });
        let error = SaveAccountInput::deserialize(MapDeserializer::new(input))
            .expect_err("secret-named unknown field must fail");

        assert!(error.to_string().contains("unknown field `password`"));
    }

    #[test]
    fn saving_juejin_article_metadata_persists_only_the_safe_desktop_entry() {
        let service = service();
        let saved = service
            .save_article_account(SaveArticleAccountInput {
                display_name: "Juejin Notes".into(),
                status: "logged_out".into(),
                phone: "route-jj-01".into(),
                partition: "persist:juejin-notes".into(),
            })
            .expect("safe Juejin metadata");
        assert_eq!(saved.id, "juejin-juejin-notes");
        assert_eq!(saved.status, "logged_out");
        assert_eq!(
            service.snapshot().expect("snapshot").article_accounts,
            vec![super::ArticleAccountEntry {
                id: "juejin-juejin-notes".into(),
                display_name: "Juejin Notes".into(),
                status: "logged_out",
            }]
        );
    }

    #[test]
    fn saving_juejin_article_metadata_rejects_invalid_routing() {
        let error = service()
            .save_article_account(SaveArticleAccountInput {
                display_name: "Juejin Notes".into(),
                status: "logged_out".into(),
                phone: String::new(),
                partition: "not-a-partition".into(),
            })
            .expect_err("invalid route must fail");
        assert!(
            error
                .to_string()
                .contains("partition must start with persist:")
        );
    }

    #[test]
    fn article_account_input_rejects_secret_named_unknown_fields() {
        let input = [
            ("displayName", "Juejin Notes"),
            ("status", "logged_out"),
            ("phone", "route-jj-01"),
            ("partition", "persist:juejin-notes"),
            ("token", "must-not-be-accepted"),
        ]
        .into_iter()
        .map(|(key, value)| {
            (
                StringDeserializer::<ValueError>::new(key.to_owned()),
                StringDeserializer::<ValueError>::new(value.to_owned()),
            )
        });
        let error = SaveArticleAccountInput::deserialize(MapDeserializer::new(input))
            .expect_err("secret-named unknown field must fail");
        assert!(error.to_string().contains("unknown field `token`"));
    }

    #[test]
    fn history_query_accepts_camel_case_fields_and_rejects_secret_unknown_fields() {
        let parse = |fields: Vec<(&str, bool)>| {
            HistoryQueryInput::deserialize(MapDeserializer::new(fields.into_iter().map(
                |(key, value)| {
                    (
                        StringDeserializer::<ValueError>::new(key.to_owned()),
                        BoolDeserializer::<ValueError>::new(value),
                    )
                },
            )))
        };
        let query = parse(vec![("all", false)]).expect("valid camelCase history query");
        assert_eq!(query.days, None);
        assert!(!query.all);
        assert_eq!(query.platform, None);
        assert_eq!(query.status, None);

        let error = parse(vec![("all", false), ("token", true)])
            .expect_err("secret-named unknown field must fail");
        assert!(error.to_string().contains("unknown field `token`"));
    }

    #[test]
    fn history_defaults_to_seven_days_and_all_removes_the_cutoff() {
        let service = service();
        let now = Utc
            .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
            .single()
            .expect("fixed clock");
        service
            .repository
            .append_history(&history_record(
                "recent",
                "Recent",
                matrixpost_core::Platform::Douyin,
                PublishState::Published,
                now - Duration::days(7),
                false,
                false,
            ))
            .expect("recent history");
        service
            .repository
            .append_history(&history_record(
                "old",
                "Old",
                matrixpost_core::Platform::Douyin,
                PublishState::Published,
                now - Duration::days(8),
                false,
                false,
            ))
            .expect("old history");

        assert_eq!(
            service
                .history_entries(history_input(None, false, None, None), now)
                .expect("default history")
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["recent"]
        );
        assert_eq!(
            service
                .history_entries(history_input(None, true, None, None), now)
                .expect("all history")
                .len(),
            2
        );
    }

    #[test]
    fn history_intersects_platform_and_status_and_scheduled_excludes_drafts() {
        let service = service();
        let now = Utc
            .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
            .single()
            .expect("fixed clock");
        for record in [
            history_record(
                "dy-success",
                "Dy success",
                matrixpost_core::Platform::Douyin,
                PublishState::Published,
                now,
                false,
                false,
            ),
            history_record(
                "dy-failed",
                "Dy failed",
                matrixpost_core::Platform::Douyin,
                PublishState::Failed,
                now,
                false,
                false,
            ),
            history_record(
                "xhs-success",
                "Xhs success",
                matrixpost_core::Platform::Xiaohongshu,
                PublishState::Published,
                now,
                false,
                false,
            ),
            history_record(
                "draft",
                "Draft",
                matrixpost_core::Platform::Douyin,
                PublishState::Draft,
                now,
                true,
                true,
            ),
            history_record(
                "queued",
                "Queued",
                matrixpost_core::Platform::Douyin,
                PublishState::Queued,
                now,
                false,
                true,
            ),
        ] {
            service.repository.append_history(&record).expect("history");
        }

        assert_eq!(
            service
                .history_entries(history_input(None, true, Some("dy"), Some("success")), now)
                .expect("intersected history")
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dy-success"]
        );
        let scheduled = service
            .history_entries(
                history_input(None, true, Some("dy"), Some("scheduled")),
                now,
            )
            .expect("scheduled history");
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].id, "queued");
        assert!(scheduled[0].scheduled);
    }

    #[test]
    fn history_entries_never_include_media_or_account_routing() {
        let service = service();
        let now = Utc
            .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
            .single()
            .expect("fixed clock");
        service
            .repository
            .append_history(&history_record(
                "safe",
                "Safe title",
                matrixpost_core::Platform::Douyin,
                PublishState::Draft,
                now,
                true,
                false,
            ))
            .expect("history");

        let entry = service
            .history_entries(history_input(None, true, None, None), now)
            .expect("safe history")
            .pop()
            .expect("history entry");
        let rendered = format!("{entry:?}");
        assert!(!rendered.contains("/private/video.mp4"));
        assert!(!rendered.contains("private-route"));
        assert!(!rendered.contains("persist:private"));
        assert!(!rendered.contains("private detail"));
        assert!(entry.draft);
        assert!(!entry.scheduled);
    }

    #[test]
    fn lifecycle_input_rejects_unknown_fields() {
        let input = [("businessObjectId", "object-1"), ("unexpected", "value")]
            .into_iter()
            .map(|(key, value)| {
                (
                    StringDeserializer::<ValueError>::new(key.to_owned()),
                    StringDeserializer::<ValueError>::new(value.to_owned()),
                )
            });
        let error = LifecycleObjectIdInput::deserialize(MapDeserializer::new(input))
            .expect_err("unknown lifecycle field must fail");

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn lifecycle_child_lists_reject_missing_objects_without_exposing_the_identifier() {
        let service = service();

        for result in [
            service
                .lifecycle_ledger_entries("missing-object".into())
                .map(|_| ()),
            service
                .lifecycle_content_attributions("missing-object".into())
                .map(|_| ()),
            service
                .lifecycle_business_relations("missing-object".into())
                .map(|_| ()),
        ] {
            assert_eq!(
                result
                    .expect_err("missing object must not look like an empty list")
                    .to_string(),
                "local lifecycle record was not found: the requested lifecycle record does not exist"
            );
        }
    }

    #[test]
    fn lifecycle_service_round_trips_object_ledger_and_content_attribution() {
        let service = service();
        let object = service
            .create_lifecycle_object(CreateLifecycleObjectInput {
                id: "project-1".into(),
                kind: "project".into(),
                external_id: Some("external-1".into()),
                display_name: "Launch plan".into(),
                attributes: BTreeMap::from([("region".into(), "north".into())]),
            })
            .expect("lifecycle object");
        assert_eq!(object.lifecycle_status, "draft");
        assert_eq!(object.approval_status, "pending");
        assert_eq!(object.revision, 0);
        assert_eq!(service.lifecycle_objects().expect("objects"), vec![object]);

        let entry = service
            .append_lifecycle_ledger_entry(AppendLifecycleLedgerEntryInput {
                id: "entry-1".into(),
                business_object_id: "project-1".into(),
                direction: LifecycleLedgerDirectionInput::Expense,
                category: "materials".into(),
                amount_minor: 1250,
                currency: "CNY".into(),
                approval_status: Some(LifecycleApprovalStatusInput::Approved),
                counterparty: Some("Supplier".into()),
                reference: None,
                description: Some("Sample purchase".into()),
            })
            .expect("ledger entry");
        assert_eq!(entry.direction, "expense");
        assert_eq!(entry.amount_minor, 1250);
        assert_eq!(
            service
                .lifecycle_ledger_entries("project-1".into())
                .expect("ledger entries"),
            vec![entry]
        );

        let recorded_at = Utc
            .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
            .single()
            .expect("fixed clock");
        service
            .repository
            .append_history(&history_record(
                "history-1",
                "Local draft",
                matrixpost_core::Platform::Douyin,
                PublishState::Draft,
                recorded_at,
                true,
                false,
            ))
            .expect("seeded history");
        let attribution = service
            .add_lifecycle_content_attribution(AddLifecycleContentAttributionInput {
                business_object_id: "project-1".into(),
                history_id: "history-1".into(),
            })
            .expect("content attribution");
        assert_eq!(attribution.history_id, "history-1");
        assert_eq!(
            service
                .lifecycle_content_attributions("project-1".into())
                .expect("attributions"),
            vec![attribution]
        );

        service
            .create_lifecycle_object(CreateLifecycleObjectInput {
                id: "customer-1".into(),
                kind: "customer".into(),
                external_id: None,
                display_name: "Example customer".into(),
                attributes: BTreeMap::new(),
            })
            .expect("related object");
        let relation = service
            .add_lifecycle_business_relation(AddLifecycleBusinessRelationInput {
                id: "relation-1".into(),
                source_business_object_id: "project-1".into(),
                target_business_object_id: "customer-1".into(),
                relation_type: "customer_interest".into(),
                attributes: BTreeMap::from([("priority".into(), "high".into())]),
            })
            .expect("business relation");
        assert_eq!(relation.relation_type, "customer_interest");
        assert_eq!(
            service
                .lifecycle_business_relations("customer-1".into())
                .expect("inbound relation"),
            vec![relation]
        );
    }

    #[test]
    fn lifecycle_transition_increments_revision_and_rejects_stale_updates() {
        let service = service();
        service
            .create_lifecycle_object(CreateLifecycleObjectInput {
                id: "asset-1".into(),
                kind: "asset".into(),
                external_id: None,
                display_name: "Reusable asset".into(),
                attributes: BTreeMap::new(),
            })
            .expect("lifecycle object");
        let transitioned = service
            .transition_lifecycle_object(TransitionLifecycleObjectInput {
                id: "asset-1".into(),
                expected_revision: 0,
                lifecycle_status: LifecycleStatusInput::Active,
                approval_status: LifecycleApprovalStatusInput::Pending,
            })
            .expect("transition");
        assert_eq!(transitioned.lifecycle_status, "active");
        assert_eq!(transitioned.revision, 1);

        let error = service
            .transition_lifecycle_object(TransitionLifecycleObjectInput {
                id: "asset-1".into(),
                expected_revision: 0,
                lifecycle_status: LifecycleStatusInput::Completed,
                approval_status: LifecycleApprovalStatusInput::Pending,
            })
            .expect_err("stale transition must fail");
        assert_eq!(
            error.to_string(),
            "invalid local draft: lifecycle request could not be completed"
        );
    }
}
