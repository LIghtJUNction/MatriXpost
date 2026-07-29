use std::sync::Arc;

use chrono::Utc;
use matrixpost_core::{
    Account, ArticleAccount, ArticleHistoryRecord, ArticlePublicationQueue, ArticleRunner,
    BusinessObject, BusinessRelation, ContentAttribution, DomainError, HistoryFilter,
    HistoryStatus, LedgerEntry, LifecycleRepository, Platform, ProviderRegistry, ProviderRunner,
    PublicationHistoryEntry, PublicationQueue, Repository, SqliteRepository,
};
use rmcp::{handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router};
use serde::Serialize;

use crate::{
    AccountsPlatform, AddBusinessRelationInput, AddContentAttributionInput, AppendLedgerEntryInput,
    ApprovalStatusInput, CreateBusinessObjectInput, GetBusinessObjectInput, HistoryPlatform,
    ListAccountsInput, ListArticleHistoryInput, ListBusinessObjectsInput,
    ListBusinessRelationsInput, ListContentAttributionsInput, ListHistoryInput,
    ListLedgerEntriesInput, ListedAccount, PublicationResult, ReviewFanqieStatusInput,
    ReviewStatusResult, ToolFailure, TransitionBusinessObjectInput,
    request::{
        article_dispatch_result, article_request, article_unavailable_result, job_result,
        video_dispatch_result, video_request,
    },
};

#[derive(Clone)]
pub(crate) struct MatrixpostMcp {
    pub(crate) repository: Arc<SqliteRepository>,
    pub(crate) provider_registry: Arc<ProviderRegistry>,
    pub(crate) provider_runners: Arc<Vec<ProviderRunner>>,
    pub(crate) article_runner: Option<ArticleRunner>,
}

#[tool_router(server_handler, vis = "pub(crate)")]
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
        description = "List terminal scheduled-article local runner workflow history. Records never prove remote publication."
    )]
    async fn list_article_history(
        &self,
        Parameters(_input): Parameters<ListArticleHistoryInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_article_history_result() {
            Ok(result) => structured(result),
            Err(message) => tool_error("internal_error", message),
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
        Parameters(input): Parameters<crate::PublishVideoInput>,
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
        Parameters(input): Parameters<crate::PublishArticleInput>,
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
    pub(crate) fn list_business_objects_result(&self) -> Result<Vec<BusinessObject>, DomainError> {
        self.repository.business_objects()
    }

    pub(crate) fn get_business_object_result(
        &self,
        input: GetBusinessObjectInput,
    ) -> Result<BusinessObject, DomainError> {
        self.repository
            .business_object(&input.id)?
            .ok_or(DomainError::UnknownBusinessObject(input.id))
    }

    pub(crate) fn create_business_object_result(
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
                .unwrap_or(crate::LifecycleStatusInput::Draft)
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

    pub(crate) fn list_ledger_entries_result(
        &self,
        input: ListLedgerEntriesInput,
    ) -> Result<Vec<LedgerEntry>, DomainError> {
        self.repository.ledger_entries(&input.business_object_id)
    }

    pub(crate) fn append_ledger_entry_result(
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

    pub(crate) fn list_content_attributions_result(
        &self,
        input: ListContentAttributionsInput,
    ) -> Result<Vec<ContentAttribution>, DomainError> {
        self.repository
            .content_attributions(&input.business_object_id)
    }

    pub(crate) fn add_content_attribution_result(
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

    pub(crate) fn list_business_relations_result(
        &self,
        input: ListBusinessRelationsInput,
    ) -> Result<Vec<BusinessRelation>, DomainError> {
        self.repository
            .business_relations(&input.business_object_id)
    }

    pub(crate) fn add_business_relation_result(
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

    pub(crate) fn transition_business_object_result(
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

    pub(crate) fn list_accounts_result(
        &self,
        input: ListAccountsInput,
    ) -> Result<Vec<ListedAccount>, String> {
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

    pub(crate) fn list_history_result(
        &self,
        input: ListHistoryInput,
    ) -> Result<Vec<PublicationHistoryEntry>, String> {
        let filter = HistoryFilter::from_query(
            input.days,
            input.all.unwrap_or(false),
            input.platform.and_then(history_video_platform),
            input.status.map(HistoryStatus::from),
            Utc::now(),
        )
        .map_err(|error| error.to_string())?;
        Ok(filter
            .filter(
                self.repository
                    .history()
                    .map_err(|error| error.to_string())?,
            )
            .into_iter()
            .map(PublicationHistoryEntry::from)
            .collect())
    }

    pub(crate) fn list_article_history_result(&self) -> Result<Vec<ArticleHistoryRecord>, String> {
        self.repository
            .article_history()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn review_fanqie_status_result(
        &self,
        input: ReviewFanqieStatusInput,
    ) -> ReviewStatusResult {
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

    pub(crate) fn publish_video_result(
        &self,
        input: crate::PublishVideoInput,
    ) -> Result<PublicationResult, String> {
        let request = video_request(input)?;
        if request.draft || request.scheduled_at.is_some() {
            return self.persist_local_video_job(&request);
        }
        let report = self
            .provider_registry
            .dispatch_all(&request)
            .map_err(|error| error.to_string())?;
        self.repository
            .record_provider_dispatch_history(&request, &report, Utc::now())
            .map_err(|_| "local dispatch result could not be persisted".to_owned())?;
        let mut result = video_dispatch_result(report);
        result.persisted = true;
        Ok(result)
    }

    fn persist_local_video_job(
        &self,
        request: &matrixpost_core::PublishRequest,
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

    pub(crate) fn publish_article_result(
        &self,
        input: crate::PublishArticleInput,
    ) -> Result<PublicationResult, String> {
        let request = article_request(input)?;
        if request.scheduled_at.is_some() {
            self.repository
                .enqueue_article(&request, Utc::now())
                .map_err(|error| error.to_string())?;
            return Ok(PublicationResult {
                outcome: "scheduled_locally",
                provider_available: false,
                remote_publish_attempted: false,
                persisted: true,
                job: None,
                providers: None,
                message: "scheduled article was persisted for local runner work; no remote publishing was attempted",
            });
        }
        let Some(runner) = &self.article_runner else {
            return Ok(article_unavailable_result());
        };
        Ok(article_dispatch_result(
            runner
                .dispatch(&request)
                .map_err(|error| error.to_string())?,
        ))
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

pub(crate) fn structured<T: Serialize>(value: T) -> CallToolResult {
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

pub(crate) fn tool_error(code: &'static str, message: String) -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!(ToolFailure {
        outcome: "rejected",
        code,
        message
    }))
}
