use std::{path::PathBuf, str::FromStr, sync::Arc};

use chrono::Utc;
use matrixpost_core::{
    Account, AccountReadiness, AccountStatus, ApprovalStatus, ArticleAccount, ArticlePlatform,
    BusinessObject, BusinessObjectStatus, BusinessRelation, ContentAttribution, HistoryFilter,
    HistoryStatus, LedgerEntry, LifecycleRepository, LocalSchedule, MediaSource, Platform,
    PublicationHistoryEntry, PublicationQueue, PublishRequest, Repository, ReviewStatus,
    SqliteRepository,
};

use crate::{
    AccountEntry, AccountReadinessInput, AccountReadinessReport, AccountSaved,
    AddLifecycleBusinessRelationInput, AddLifecycleContentAttributionInput,
    AppendLifecycleLedgerEntryInput, ArticleAccountEntry, ArticleAccountSaved,
    CreateLifecycleObjectInput, DesktopError, DesktopSnapshot, DispatchToLocalRunnerInput,
    DraftSaved, FanqieReviewStatusInput, FanqieReviewStatusReport, HistoryEntry, HistoryQueryInput,
    LifecycleApprovalStatusInput, LifecycleBusinessRelationEntry, LifecycleContentAttributionEntry,
    LifecycleLedgerEntry, LifecycleObjectEntry, LocalRunnerDispatchReport, SaveAccountInput,
    SaveArticleAccountInput, SaveDraftInput, TransitionLifecycleObjectInput,
    projection::{
        account_id, article_account_id, article_account_status, article_account_status_label,
    },
    runner::{
        account_readiness_label, lifecycle_error, local_probe_runner, local_runner_dispatch_report,
        local_runner_registry, review_status_label, valid_review_title_query,
    },
};

/// Testable local application service, independent of the Tauri runtime.
#[derive(Clone)]
pub struct DesktopService {
    #[cfg(not(test))]
    repository: Arc<SqliteRepository>,
    #[cfg(test)]
    pub(crate) repository: Arc<SqliteRepository>,
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

    /// Dispatches once to explicitly declared local runners without storing any
    /// runner, browser, account, or scheduling configuration.
    pub fn dispatch_to_local_runner(
        &self,
        input: DispatchToLocalRunnerInput,
    ) -> Result<LocalRunnerDispatchReport, DesktopError> {
        if !input.confirmed {
            return Err(DesktopError::InvalidRequest(
                "explicit local runner confirmation is required".into(),
            ));
        }
        if input.scheduled_at.is_some() {
            return Err(DesktopError::InvalidRequest(
                "scheduled dispatch is not available; save a local draft instead".into(),
            ));
        }

        let request = PublishRequest {
            source: MediaSource::LocalFile(PathBuf::from(input.media_path.trim())),
            title: input.title,
            short_title: None,
            tags: Vec::new(),
            address: None,
            draft: false,
            bt2: None,
            scheduled_at: None,
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

        let registry = local_runner_registry(&input.provider_runners, &request.targets)?;

        local_runner_dispatch_report(self.repository.as_ref(), &registry, &request)
    }

    /// Probes an explicitly declared local runner once for an upload-form
    /// inference. This neither starts nor discovers a runner or browser.
    pub fn account_readiness(
        &self,
        input: AccountReadinessInput,
    ) -> Result<AccountReadinessReport, DesktopError> {
        if !input.confirmed {
            return Err(DesktopError::InvalidRequest(
                "explicit account readiness confirmation is required".into(),
            ));
        }
        let platform = Platform::from_str(&input.platform)?;
        let Some(runner) = local_probe_runner(input.provider_runner.as_deref(), platform)? else {
            return Ok(AccountReadinessReport {
                state: account_readiness_label(AccountReadiness::Unavailable),
            });
        };
        let readiness = runner.account_readiness().map_err(|_| {
            DesktopError::InvalidRequest("account readiness request could not be completed".into())
        })?;
        Ok(AccountReadinessReport {
            state: account_readiness_label(readiness),
        })
    }

    /// Queries a bounded Fanqie title through an explicitly declared local
    /// runner. It never persists or returns the title or page data.
    pub fn fanqie_review_status(
        &self,
        input: FanqieReviewStatusInput,
    ) -> Result<FanqieReviewStatusReport, DesktopError> {
        if !input.confirmed {
            return Err(DesktopError::InvalidRequest(
                "explicit Fanqie review confirmation is required".into(),
            ));
        }
        if !valid_review_title_query(&input.title_query) {
            return Err(DesktopError::InvalidRequest(
                "review title query must be non-empty and within the local limit".into(),
            ));
        }
        let Some(runner) =
            local_probe_runner(input.provider_runner.as_deref(), Platform::FanqieVideo)?
        else {
            return Ok(FanqieReviewStatusReport {
                state: review_status_label(ReviewStatus::Unavailable),
            });
        };
        let status = runner
            .fanqie_review_status(&input.title_query)
            .map_err(|_| {
                DesktopError::InvalidRequest("Fanqie review request could not be completed".into())
            })?;
        Ok(FanqieReviewStatusReport {
            state: review_status_label(status),
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
            .map(PublicationHistoryEntry::from)
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
