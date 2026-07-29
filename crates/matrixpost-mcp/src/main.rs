//! Local stdio MCP adapter for MatriXpost's credential-free SQLite state.
//!
//! The server never starts a browser, provider, shell, or daemon. Video
//! publication can use only an explicitly declared loopback local runner; it
//! never reports remote publication success.

use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf, process::ExitCode, sync::Arc};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use matrixpost_core::{
    Account, AccountSelection, ApprovalStatus, ArticleAccount, ArticleDispatchOutcome,
    ArticleRunner, BusinessObject, BusinessObjectStatus, BusinessRelation, ContentAttribution,
    DispatchOutcome, DomainError, HistoryFilter, HistoryRecord, HistoryStatus, LedgerDirection,
    LedgerEntry, LifecycleRepository, LocalSchedule, MediaSource, Platform, PlatformOverride,
    ProviderDispatchReport, ProviderRegistry, ProviderRunner, PublicationQueue,
    PublishArticleRequest, PublishRequest, PublishState, Repository, ReviewStatus, ScheduledJob,
    SqliteRepository, WechatLink,
};
use rmcp::{
    ServiceExt, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const DEFAULT_STATE_PATH: &str = "matrixpost.db";
const STATE_PATH_ENV: &str = "MATRIXPOST_STATE_PATH";
const LOG_ENV: &str = "MATRIXPOST_MCP_LOG";
const PROVIDER_MESSAGE: &str =
    "no local provider runner is configured; no remote publishing was attempted";

/// Exact upstream account-query platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AccountsPlatform {
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
enum VideoPlatform {
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
enum ArticlePlatformInput {
    Juejin,
}

/// Exact documented history filter set; Fanqie is intentionally absent.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum HistoryPlatform {
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
enum HistoryStatusInput {
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
struct ListAccountsInput {
    /// Optional exact upstream platform code.
    platform: Option<AccountsPlatform>,
}

/// The upstream-compatible history query. Filters operate only on local state.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListHistoryInput {
    /// Number of trailing days to return; defaults to seven unless `all` is true.
    days: Option<u16>,
    /// Optional exact upstream platform code.
    platform: Option<HistoryPlatform>,
    /// One of `success`, `failed`, `publishing`, or `scheduled`.
    status: Option<HistoryStatusInput>,
    /// When true, do not apply the default trailing-seven-day filter.
    all: Option<bool>,
}

/// Bounded Fanqie title lookup. The tool returns only a finite status label;
/// it never returns the submitted title or any page content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewFanqieStatusInput {
    /// A bounded title fragment used only inside the local runner.
    title: String,
}

/// Video-link metadata accepted by MatrixMedia's WeChat Channels request form.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum SphLinkInput {
    /// Explicitly disables a link.
    None {},
    /// Links to a product and therefore requires its provider value.
    Product { value: String },
}

/// Upstream-compatible video publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishVideoInput {
    /// One exact upstream video platform code.
    platform: VideoPlatform,
    /// Local absolute video path or an `http`/`https` URL.
    file: String,
    /// Publication title.
    title: String,
    /// Account phone/partition selector used only as local routing metadata.
    phone: String,
    /// Upstream secondary-title field.
    bt2: Option<String>,
    /// Comma- or whitespace-separated tags.
    tags: Option<String>,
    /// Optional publication address.
    address: Option<String>,
    /// Upstream schedule in `YYYY-MM-DD HH:MM` or `YYYY-MM-DD HH:MM:SS` form.
    publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    show: Option<bool>,
    /// Record the job as a draft instead of a queued local intent.
    draft: Option<bool>,
    /// Optional platform-specific creative declaration.
    creative_statement: Option<String>,
    /// WeChat Channels product identifier.
    sph_product_id: Option<String>,
    /// WeChat Channels link data.
    sph_link: Option<SphLinkInput>,
}

/// Upstream-compatible Juejin article publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishArticleInput {
    /// The only upstream MCP article target: `juejin`.
    platform: ArticlePlatformInput,
    /// Account phone/partition selector used only as local routing metadata.
    phone: String,
    /// Article title.
    title: String,
    /// Inline article body; required when `file` is omitted.
    content: Option<String>,
    /// Markdown file path; required when `content` is omitted.
    file: Option<String>,
    /// Optional cover image path.
    cover: Option<String>,
    /// Optional Juejin category.
    category: Option<String>,
    /// Upstream single-string tags, normalized into the typed core vector.
    tags: Option<String>,
    /// Optional article summary.
    summary: Option<String>,
    /// CLI-compatible `HH:MM`, `YYYY-MM-DD HH:MM`, or `YYYY-MM-DD HH:MM:SS` schedule.
    publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    show: Option<bool>,
}

/// Generic lifecycle state accepted at the MCP boundary.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LifecycleStatusInput {
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
enum ApprovalStatusInput {
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
enum LedgerDirectionInput {
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
struct GetBusinessObjectInput {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListBusinessObjectsInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBusinessObjectInput {
    id: String,
    kind: String,
    display_name: String,
    external_id: Option<String>,
    lifecycle_status: Option<LifecycleStatusInput>,
    approval_status: Option<ApprovalStatusInput>,
    #[schemars(with = "Option<BTreeMap<String, String>>")]
    attributes: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListLedgerEntriesInput {
    business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppendLedgerEntryInput {
    id: String,
    business_object_id: String,
    direction: LedgerDirectionInput,
    category: String,
    amount_minor: i64,
    currency: String,
    approval_status: Option<ApprovalStatusInput>,
    occurred_at: Option<DateTime<Utc>>,
    counterparty: Option<String>,
    reference: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListContentAttributionsInput {
    business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddContentAttributionInput {
    business_object_id: String,
    history_id: String,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListBusinessRelationsInput {
    business_object_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddBusinessRelationInput {
    id: String,
    source_business_object_id: String,
    target_business_object_id: String,
    relation_type: String,
    #[schemars(with = "Option<BTreeMap<String, String>>")]
    attributes: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransitionBusinessObjectInput {
    id: String,
    expected_revision: u64,
    lifecycle_status: LifecycleStatusInput,
    approval_status: ApprovalStatusInput,
    updated_at: Option<DateTime<Utc>>,
}

/// Exact upstream account-list item contract.
#[derive(Debug, Serialize)]
struct ListedAccount {
    phone: String,
    platform: &'static str,
    partition: String,
}

/// The non-sensitive, durable part of a locally queued video job.
#[derive(Debug, Serialize)]
struct JobResult {
    id: String,
    state: PublishState,
    due_at: Option<LocalSchedule>,
    revision: u64,
}

/// Explicit provider boundary returned by both publication tools.
#[derive(Debug, Serialize)]
struct PublicationResult {
    outcome: &'static str,
    provider_available: bool,
    remote_publish_attempted: bool,
    persisted: bool,
    job: Option<JobResult>,
    providers: Option<BTreeMap<Platform, SafeProviderOutcome>>,
    message: &'static str,
}

/// Reason-free result of a Fanqie local review-status lookup.
#[derive(Debug, Serialize)]
struct ReviewStatusResult {
    outcome: &'static str,
    platform: &'static str,
    message: &'static str,
}

impl ReviewStatusResult {
    fn unavailable() -> Self {
        Self {
            outcome: "unavailable",
            platform: "fqsp",
            message: "no local Fanqie runner is configured; no browser review-status probe was attempted",
        }
    }

    fn rejected() -> Self {
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
///
/// This is deliberately not [`DispatchOutcome`]: runner response reasons can
/// include transport details that do not belong in MCP tool output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum SafeProviderOutcome {
    Queued,
    Unavailable,
    Rejected,
}

/// Typed, inspectable validation result for an MCP tool call.
#[derive(Debug, Serialize)]
struct ToolFailure {
    outcome: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Clone)]
struct MatrixpostMcp {
    repository: Arc<SqliteRepository>,
    provider_registry: Arc<ProviderRegistry>,
    provider_runners: Arc<Vec<ProviderRunner>>,
    article_runner: Option<ArticleRunner>,
}

#[tool_router(server_handler)]
impl MatrixpostMcp {
    #[tool(
        description = "List credential-free local MatriXpost accounts, optionally filtered by platform."
    )]
    async fn list_accounts(
        &self,
        Parameters(input): Parameters<ListAccountsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_accounts_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "List local MatriXpost publication history with upstream-compatible filters."
    )]
    async fn list_history(
        &self,
        Parameters(input): Parameters<ListHistoryInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_history_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "Query a bounded Fanqie title only through an explicitly configured loopback local runner. The result is a safe review-status label and never proves remote publication acceptance."
    )]
    async fn review_fanqie_status(
        &self,
        Parameters(input): Parameters<ReviewFanqieStatusInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(structured(self.review_fanqie_status_result(input)))
    }

    #[tool(
        description = "Dispatch an immediate video only through an explicitly configured local runner. Drafts and scheduled jobs remain local. A queued result proves only local runner completion, never remote publication."
    )]
    async fn publish_video(
        &self,
        Parameters(input): Parameters<PublishVideoInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.publish_video_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "Validate a Juejin article request and, only with an explicit local article runner, dispatch it through that runner. A queued result confirms local workflow completion only, never remote publication."
    )]
    async fn publish_article(
        &self,
        Parameters(input): Parameters<PublishArticleInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.publish_article_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(description = "List generic lifecycle business objects from local MatriXpost state.")]
    async fn list_business_objects(
        &self,
        Parameters(_input): Parameters<ListBusinessObjectsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_business_objects_result() {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(description = "Get one generic lifecycle business object by its stable identifier.")]
    async fn get_business_object(
        &self,
        Parameters(input): Parameters<GetBusinessObjectInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.get_business_object_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(description = "Create a generic lifecycle business object in local MatriXpost state.")]
    async fn create_business_object(
        &self,
        Parameters(input): Parameters<CreateBusinessObjectInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.create_business_object_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(description = "List immutable ledger entries for a generic business object.")]
    async fn list_ledger_entries(
        &self,
        Parameters(input): Parameters<ListLedgerEntriesInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_ledger_entries_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(
        description = "Append an immutable expense or revenue ledger entry to a generic business object."
    )]
    async fn append_ledger_entry(
        &self,
        Parameters(input): Parameters<AppendLedgerEntryInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.append_ledger_entry_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(description = "List local content-attribution links for a generic business object.")]
    async fn list_content_attributions(
        &self,
        Parameters(input): Parameters<ListContentAttributionsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_content_attributions_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(
        description = "Link an existing local publication-history record to a generic business object."
    )]
    async fn add_content_attribution(
        &self,
        Parameters(input): Parameters<AddContentAttributionInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.add_content_attribution_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(
        description = "List incoming and outgoing immutable relations for a generic business object."
    )]
    async fn list_business_relations(
        &self,
        Parameters(input): Parameters<ListBusinessRelationsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_business_relations_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(
        description = "Create an immutable directed relation between two existing generic business objects in local MatriXpost state."
    )]
    async fn add_business_relation(
        &self,
        Parameters(input): Parameters<AddBusinessRelationInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.add_business_relation_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }

    #[tool(
        description = "Transition a generic business object's lifecycle and approval state using optimistic revision control."
    )]
    async fn transition_business_object(
        &self,
        Parameters(input): Parameters<TransitionBusinessObjectInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.transition_business_object_result(input) {
            Ok(result) => structured(result),
            Err(error) => lifecycle_tool_error(error),
        })
    }
}

impl MatrixpostMcp {
    fn list_business_objects_result(&self) -> Result<Vec<BusinessObject>, DomainError> {
        self.repository.business_objects()
    }

    fn get_business_object_result(
        &self,
        input: GetBusinessObjectInput,
    ) -> Result<BusinessObject, DomainError> {
        self.repository
            .business_object(&input.id)?
            .ok_or(DomainError::UnknownBusinessObject(input.id))
    }

    fn create_business_object_result(
        &self,
        input: CreateBusinessObjectInput,
    ) -> Result<BusinessObject, DomainError> {
        let now = Utc::now();
        let object = BusinessObject {
            id: input.id,
            kind: input.kind,
            external_id: input.external_id,
            display_name: input.display_name,
            lifecycle_status: input
                .lifecycle_status
                .unwrap_or(LifecycleStatusInput::Draft)
                .into(),
            approval_status: input
                .approval_status
                .unwrap_or(ApprovalStatusInput::Pending)
                .into(),
            revision: 0,
            attributes: input.attributes.unwrap_or_default(),
            created_at: now,
            updated_at: now,
        };
        self.repository.insert_business_object(&object)?;
        Ok(object)
    }

    fn list_ledger_entries_result(
        &self,
        input: ListLedgerEntriesInput,
    ) -> Result<Vec<LedgerEntry>, DomainError> {
        self.repository.ledger_entries(&input.business_object_id)
    }

    fn append_ledger_entry_result(
        &self,
        input: AppendLedgerEntryInput,
    ) -> Result<LedgerEntry, DomainError> {
        let now = Utc::now();
        let entry = LedgerEntry {
            id: input.id,
            business_object_id: input.business_object_id,
            direction: input.direction.into(),
            category: input.category,
            amount_minor: input.amount_minor,
            currency: input.currency,
            occurred_at: input.occurred_at.unwrap_or(now),
            approval_status: input
                .approval_status
                .unwrap_or(ApprovalStatusInput::Pending)
                .into(),
            counterparty: input.counterparty,
            reference: input.reference,
            description: input.description,
            created_at: now,
        };
        self.repository.insert_ledger_entry(&entry)?;
        Ok(entry)
    }

    fn list_content_attributions_result(
        &self,
        input: ListContentAttributionsInput,
    ) -> Result<Vec<ContentAttribution>, DomainError> {
        self.repository
            .content_attributions(&input.business_object_id)
    }

    fn add_content_attribution_result(
        &self,
        input: AddContentAttributionInput,
    ) -> Result<ContentAttribution, DomainError> {
        let attribution = ContentAttribution {
            business_object_id: input.business_object_id,
            history_id: input.history_id,
            created_at: input.created_at.unwrap_or_else(Utc::now),
        };
        self.repository.insert_content_attribution(&attribution)?;
        Ok(attribution)
    }

    fn list_business_relations_result(
        &self,
        input: ListBusinessRelationsInput,
    ) -> Result<Vec<BusinessRelation>, DomainError> {
        self.repository
            .business_relations(&input.business_object_id)
    }

    fn add_business_relation_result(
        &self,
        input: AddBusinessRelationInput,
    ) -> Result<BusinessRelation, DomainError> {
        let relation = BusinessRelation {
            id: input.id,
            source_business_object_id: input.source_business_object_id,
            target_business_object_id: input.target_business_object_id,
            relation_type: input.relation_type,
            attributes: input.attributes.unwrap_or_default(),
            created_at: Utc::now(),
        };
        self.repository.insert_business_relation(&relation)?;
        Ok(relation)
    }

    fn transition_business_object_result(
        &self,
        input: TransitionBusinessObjectInput,
    ) -> Result<BusinessObject, DomainError> {
        self.repository.transition_business_object(
            &input.id,
            input.expected_revision,
            input.lifecycle_status.into(),
            input.approval_status.into(),
            input.updated_at.unwrap_or_else(Utc::now),
        )
    }

    fn list_accounts_result(&self, input: ListAccountsInput) -> Result<Vec<ListedAccount>, String> {
        let video_filter = input.platform.and_then(accounts_video_platform);
        let include_articles = input
            .platform
            .is_none_or(|platform| matches!(platform, AccountsPlatform::Juejin));
        let mut accounts = self
            .repository
            .accounts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|account| video_filter.is_none_or(|platform| account.platform == platform))
            .map(listed_video_account)
            .collect::<Vec<_>>();
        if include_articles {
            accounts.extend(
                self.repository
                    .article_accounts()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(listed_article_account),
            );
        }
        Ok(accounts)
    }

    fn list_history_result(&self, input: ListHistoryInput) -> Result<Vec<HistoryRecord>, String> {
        let platform = input.platform.and_then(history_video_platform);
        let status = input.status.map(HistoryStatus::from);
        let filter = HistoryFilter::from_query(
            input.days,
            input.all.unwrap_or(false),
            platform,
            status,
            Utc::now(),
        )
        .map_err(|error| error.to_string())?;
        let history = self
            .repository
            .history()
            .map_err(|error| error.to_string())?;
        Ok(filter.filter(history))
    }

    fn review_fanqie_status_result(&self, input: ReviewFanqieStatusInput) -> ReviewStatusResult {
        let Some(runner) = self
            .provider_runners
            .iter()
            .find(|runner| runner.platform == Platform::FanqieVideo)
        else {
            return ReviewStatusResult::unavailable();
        };
        match runner.fanqie_review_status(&input.title) {
            Ok(status) => ReviewStatusResult::from(status),
            Err(_) => ReviewStatusResult::rejected(),
        }
    }

    fn publish_video_result(&self, input: PublishVideoInput) -> Result<PublicationResult, String> {
        let request = video_request(input)?;
        if request.draft || request.scheduled_at.is_some() {
            return self.persist_local_video_job(&request);
        }
        let report = self
            .provider_registry
            .dispatch_all(&request)
            .map_err(|error| error.to_string())?;
        Ok(video_dispatch_result(report))
    }

    fn persist_local_video_job(
        &self,
        request: &PublishRequest,
    ) -> Result<PublicationResult, String> {
        let job = self
            .repository
            .enqueue(request, Utc::now())
            .map_err(|error| error.to_string())?;
        Ok(PublicationResult {
            outcome: if request.draft {
                "draft_locally"
            } else {
                "scheduled_locally"
            },
            provider_available: false,
            remote_publish_attempted: false,
            persisted: true,
            job: Some(job_result(job)),
            providers: None,
            message: if request.draft {
                "local draft was persisted; no remote publishing was attempted"
            } else {
                "local scheduled job was persisted; no remote publishing was attempted"
            },
        })
    }

    fn publish_article_result(
        &self,
        input: PublishArticleInput,
    ) -> Result<PublicationResult, String> {
        let request = article_request(input)?;
        let Some(runner) = &self.article_runner else {
            return Ok(article_unavailable_result());
        };
        let outcome = runner
            .dispatch(&request)
            .map_err(|error| error.to_string())?;
        Ok(article_dispatch_result(outcome))
    }
}

fn article_unavailable_result() -> PublicationResult {
    PublicationResult {
        outcome: "unavailable",
        provider_available: false,
        remote_publish_attempted: false,
        persisted: false,
        job: None,
        providers: None,
        message: "no article runner is configured; no remote publishing was attempted",
    }
}

fn article_dispatch_result(outcome: ArticleDispatchOutcome) -> PublicationResult {
    match outcome {
        ArticleDispatchOutcome::Queued { .. } => PublicationResult {
            outcome: "queued",
            provider_available: true,
            remote_publish_attempted: true,
            persisted: false,
            job: None,
            providers: None,
            message: "local article runner completed its WebDriver workflow; remote publication is not confirmed",
        },
        ArticleDispatchOutcome::Unavailable { .. } => PublicationResult {
            outcome: "unavailable",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: false,
            job: None,
            providers: None,
            message: "article runner was unavailable; no remote publishing was attempted",
        },
        ArticleDispatchOutcome::Rejected {
            automation_attempted,
            ..
        } => PublicationResult {
            outcome: "rejected",
            provider_available: false,
            remote_publish_attempted: automation_attempted,
            persisted: false,
            job: None,
            providers: None,
            message: "article runner rejected the request; no remote publication success is claimed",
        },
    }
}

fn video_dispatch_result(report: ProviderDispatchReport) -> PublicationResult {
    let providers = report
        .outcomes
        .iter()
        .map(|(platform, outcome)| (*platform, safe_provider_outcome(outcome)))
        .collect::<BTreeMap<_, _>>();
    let all_queued = report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. }));
    let all_unavailable = report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. }));
    let remote_publish_attempted = report.outcomes.values().any(|outcome| {
        matches!(
            outcome,
            DispatchOutcome::Queued { .. } | DispatchOutcome::Rejected { .. }
        )
    });

    if all_queued {
        return PublicationResult {
            outcome: "queued",
            provider_available: true,
            remote_publish_attempted: true,
            persisted: false,
            job: None,
            providers: Some(providers),
            message: "local provider runner completed its WebDriver workflow; remote publication is not confirmed",
        };
    }
    if all_unavailable {
        return PublicationResult {
            outcome: "unavailable",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: false,
            job: None,
            providers: Some(providers),
            message: PROVIDER_MESSAGE,
        };
    }
    PublicationResult {
        outcome: "rejected",
        provider_available: false,
        remote_publish_attempted,
        persisted: false,
        job: None,
        providers: Some(providers),
        message: "local provider runner dispatch was incomplete; no remote publication success is claimed",
    }
}

fn safe_provider_outcome(outcome: &DispatchOutcome) -> SafeProviderOutcome {
    match outcome {
        DispatchOutcome::Queued { .. } => SafeProviderOutcome::Queued,
        DispatchOutcome::Unavailable { .. } => SafeProviderOutcome::Unavailable,
        DispatchOutcome::Rejected { .. } => SafeProviderOutcome::Rejected,
    }
}

fn accounts_video_platform(value: AccountsPlatform) -> Option<Platform> {
    Some(match value {
        AccountsPlatform::Dy => Platform::Douyin,
        AccountsPlatform::Ks => Platform::Kuaishou,
        AccountsPlatform::Blbl => Platform::Bilibili,
        AccountsPlatform::Bjh => Platform::Baijiahao,
        AccountsPlatform::Tt => Platform::Toutiao,
        AccountsPlatform::Sph => Platform::WechatChannels,
        AccountsPlatform::Xhs => Platform::Xiaohongshu,
        AccountsPlatform::Fqsp => Platform::FanqieVideo,
        AccountsPlatform::Juejin => return None,
    })
}

fn listed_video_account(account: Account) -> ListedAccount {
    ListedAccount {
        phone: account.phone,
        platform: account.platform.as_str(),
        partition: account.partition,
    }
}

fn listed_article_account(account: ArticleAccount) -> ListedAccount {
    ListedAccount {
        phone: account.phone,
        platform: "juejin",
        partition: account.partition,
    }
}

fn history_video_platform(value: HistoryPlatform) -> Option<Platform> {
    Some(match value {
        HistoryPlatform::Dy => Platform::Douyin,
        HistoryPlatform::Ks => Platform::Kuaishou,
        HistoryPlatform::Blbl => Platform::Bilibili,
        HistoryPlatform::Bjh => Platform::Baijiahao,
        HistoryPlatform::Tt => Platform::Toutiao,
        HistoryPlatform::Sph => Platform::WechatChannels,
        HistoryPlatform::Xhs => Platform::Xiaohongshu,
    })
}

fn video_request(input: PublishVideoInput) -> Result<PublishRequest, String> {
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let platform = video_platform(input.platform);
    let _ = input.show;
    let source = match url::Url::parse(&input.file) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
        Ok(url) => {
            return Err(format!(
                "unsupported remote source scheme: {}",
                url.scheme()
            ));
        }
        Err(_) => MediaSource::LocalFile(PathBuf::from(&input.file)),
    };
    let scheduled_at = input
        .publish_at
        .as_deref()
        .map(parse_video_schedule)
        .transpose()?;
    let wechat_link = if platform == Platform::WechatChannels {
        effective_sph_link(input.sph_product_id, input.sph_link)?
    } else {
        WechatLink::default()
    };
    let overrides = input.creative_statement.map(|creative_statement| {
        vec![PlatformOverride {
            platform,
            title: None,
            short_title: None,
            tags: None,
            creative_statement: Some(creative_statement),
            account: None,
            wechat_link: None,
        }]
    });
    let request = PublishRequest {
        source,
        title: input.title,
        short_title: None,
        tags: split_tags(input.tags),
        address: input.address,
        draft: input.draft.unwrap_or(false),
        bt2: input.bt2,
        scheduled_at,
        task_name: None,
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        wechat_link,
        overrides: overrides.unwrap_or_default(),
        targets: vec![platform],
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn video_platform(value: VideoPlatform) -> Platform {
    match value {
        VideoPlatform::Dy => Platform::Douyin,
        VideoPlatform::Ks => Platform::Kuaishou,
        VideoPlatform::Blbl => Platform::Bilibili,
        VideoPlatform::Bjh => Platform::Baijiahao,
        VideoPlatform::Tt => Platform::Toutiao,
        VideoPlatform::Sph => Platform::WechatChannels,
    }
}

fn parse_video_schedule(value: &str) -> Result<LocalSchedule, String> {
    normalize_full_schedule(
        value,
        "publishAt must use YYYY-MM-DD HH:mm or YYYY-MM-DD HH:mm:ss",
    )
}

fn parse_article_schedule(value: &str, today: NaiveDate) -> Result<LocalSchedule, String> {
    if let Ok(time) = NaiveTime::parse_from_str(value, "%H:%M") {
        return LocalSchedule::parse(&format!("{} {}:00", today, time.format("%H:%M")))
            .map_err(|error| error.to_string());
    }
    normalize_full_schedule(
        value,
        "publishAt must use HH:mm, YYYY-MM-DD HH:mm, or YYYY-MM-DD HH:mm:ss",
    )
}

fn normalize_full_schedule(value: &str, message: &str) -> Result<LocalSchedule, String> {
    if let Ok(schedule) = LocalSchedule::parse(value) {
        return Ok(schedule);
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")
        .map(|time| LocalSchedule(time.format("%Y-%m-%d %H:%M:%S").to_string()))
        .map_err(|_| message.to_owned())
}

fn effective_sph_link(
    product_id: Option<String>,
    link: Option<SphLinkInput>,
) -> Result<WechatLink, String> {
    if let Some(product_id) = product_id {
        if product_id.trim().is_empty() {
            return Err("sphProductId must not be empty".into());
        }
        return Ok(WechatLink {
            link_type: Some("product".into()),
            link_value: Some(product_id.clone()),
            product_id: Some(product_id),
        });
    }
    match link {
        None => Ok(WechatLink::default()),
        Some(SphLinkInput::None {}) => Ok(WechatLink {
            product_id: None,
            link_type: Some("none".into()),
            link_value: None,
        }),
        Some(SphLinkInput::Product { value }) if !value.trim().is_empty() => Ok(WechatLink {
            product_id: None,
            link_type: Some("product".into()),
            link_value: Some(value),
        }),
        Some(SphLinkInput::Product { .. }) => {
            Err("sphLink.value must not be empty when sphLink.type is product".into())
        }
    }
}

fn article_request(input: PublishArticleInput) -> Result<PublishArticleRequest, String> {
    let _ = input.platform;
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let _ = input.show;
    let request = PublishArticleRequest {
        platform: "juejin".into(),
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        title: input.title,
        content: input.content,
        file: input.file.map(PathBuf::from),
        cover: input.cover,
        category: input.category,
        tags: split_tags(input.tags),
        summary: input.summary,
        scheduled_at: input
            .publish_at
            .as_deref()
            .map(|value| parse_article_schedule(value, Local::now().date_naive()))
            .transpose()?,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn split_tags(tags: Option<String>) -> Vec<String> {
    tags.unwrap_or_default()
        .split([' ', ','])
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn job_result(job: ScheduledJob) -> JobResult {
    JobResult {
        id: job.id,
        state: job.state,
        due_at: job.due_at,
        revision: job.revision,
    }
}

fn structured<T: Serialize>(value: T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => tool_error("serialization_failure", error.to_string()),
    }
}

fn lifecycle_tool_error(error: DomainError) -> CallToolResult {
    let code = match error {
        DomainError::UnknownBusinessObject(_) | DomainError::UnknownHistoryRecord(_) => "not_found",
        DomainError::Database(_)
        | DomainError::Serialization(_)
        | DomainError::Io(_)
        | DomainError::RepositoryPoisoned
        | DomainError::CorruptState(_)
        | DomainError::ConcurrentBusinessObjectUpdate(_)
        | DomainError::BusinessObjectRevisionOverflow(_) => "failed",
        _ => "invalid_input",
    };
    let message = match code {
        "not_found" => "the requested lifecycle record does not exist".into(),
        "failed" => "the lifecycle operation could not be completed".into(),
        _ => "the lifecycle input is invalid or conflicts with existing state".into(),
    };
    tool_error(code, message)
}

fn tool_error(code: &'static str, message: String) -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!(ToolFailure {
        outcome: "rejected",
        code,
        message,
    }))
}

struct McpConfig {
    state_path: PathBuf,
    provider_registry: Arc<ProviderRegistry>,
    provider_runners: Arc<Vec<ProviderRunner>>,
    article_runner: Option<ArticleRunner>,
}

fn mcp_config(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<McpConfig, String> {
    let mut state_path = None;
    let mut provider_runners = Vec::new();
    let mut article_runner = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--state-path" {
            let value = args
                .next()
                .ok_or_else(|| "--state-path requires a path".to_owned())?;
            if state_path.replace(PathBuf::from(value)).is_some() {
                return Err("--state-path may be supplied only once".into());
            }
        } else if let Some(value) = argument.strip_prefix("--state-path=") {
            if value.is_empty() || state_path.replace(PathBuf::from(value)).is_some() {
                return Err("--state-path must be supplied once with a non-empty path".into());
            }
        } else if argument == "--provider-runner" {
            let value = args.next().ok_or_else(|| {
                "--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".to_owned()
            })?;
            provider_runners.push(mcp_provider_runner(&value)?);
        } else if let Some(value) = argument.strip_prefix("--provider-runner=") {
            if value.is_empty() {
                return Err("--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".into());
            }
            provider_runners.push(mcp_provider_runner(value)?);
        } else if argument == "--article-runner" {
            let value = args
                .next()
                .ok_or_else(|| "--article-runner requires tcp:127.0.0.1:PORT".to_owned())?;
            if article_runner.is_some() {
                return Err("--article-runner may be supplied only once".into());
            }
            article_runner =
                Some(ArticleRunner::parse_cli(&value).map_err(|error| error.to_string())?);
        } else if let Some(value) = argument.strip_prefix("--article-runner=") {
            if value.is_empty() || article_runner.is_some() {
                return Err(
                    "--article-runner must be supplied once with tcp:127.0.0.1:PORT".into(),
                );
            }
            article_runner =
                Some(ArticleRunner::parse_cli(value).map_err(|error| error.to_string())?);
        } else {
            return Err(format!("unsupported argument: {argument}"));
        }
    }
    let provider_registry = ProviderRegistry::from_runners(provider_runners.clone())
        .map_err(|error| error.to_string())?;
    Ok(McpConfig {
        state_path: state_path
            .or_else(|| env_path.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_PATH)),
        provider_registry: Arc::new(provider_registry),
        provider_runners: Arc::new(provider_runners),
        article_runner,
    })
}

fn mcp_provider_runner(value: &str) -> Result<ProviderRunner, String> {
    let runner = ProviderRunner::parse_cli(value).map_err(|error| error.to_string())?;
    if runner.loopback_tcp_address().is_none() {
        return Err("--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".into());
    }
    Ok(runner)
}

#[cfg(test)]
fn state_path(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<PathBuf, String> {
    mcp_config(args, env_path).map(|config| config.state_path)
}

fn logging_enabled(value: Option<&OsStr>) -> bool {
    matches!(value.and_then(OsStr::to_str), Some("1" | "true" | "yes"))
}

fn log_error(message: impl std::fmt::Display) {
    if logging_enabled(std::env::var_os(LOG_ENV).as_deref()) {
        eprintln!("matrixpost-mcp: {message}");
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let config = match mcp_config(
        std::env::args().skip(1),
        std::env::var_os(STATE_PATH_ENV).as_deref(),
    ) {
        Ok(config) => config,
        Err(error) => {
            log_error(error);
            return ExitCode::from(2);
        }
    };
    let repository = match SqliteRepository::open(&config.state_path) {
        Ok(repository) => Arc::new(repository),
        Err(error) => {
            log_error(format!(
                "failed to open {}: {error}",
                config.state_path.display()
            ));
            return ExitCode::from(4);
        }
    };
    let service = match (MatrixpostMcp {
        repository,
        provider_registry: config.provider_registry,
        provider_runners: config.provider_runners,
        article_runner: config.article_runner,
    })
    .serve(stdio())
    .await
    {
        Ok(service) => service,
        Err(error) => {
            log_error(format!("stdio service failed: {error}"));
            return ExitCode::from(4);
        }
    };
    match service.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            log_error(format!("stdio service stopped unexpectedly: {error}"));
            ExitCode::from(4)
        }
    }
}

#[cfg(test)]
mod tests;
