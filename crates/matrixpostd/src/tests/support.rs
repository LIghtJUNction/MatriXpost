pub(super) use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub(super) use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode},
};
pub(super) use matrixpost_core::{
    ArticlePublicationQueue, DispatchOutcome, DomainError, LocalSchedule, Platform,
    ProviderDispatchReport, ProviderRegistry, PublicationQueue, PublishRequest, PublishState,
    Repository, SqliteRepository,
};
pub(super) use std::{path::PathBuf, sync::Arc, time::Duration};
use tower::ServiceExt;

pub(super) use crate::{
    api::app,
    config::{Args, DaemonConfig},
    state::AppState,
};

pub(super) async fn json_response(
    router: Router,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let response = router.oneshot(request).await.expect("router must respond");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body must be readable");
    let value = serde_json::from_slice(&body).expect("response body must be JSON");
    (status, value)
}

pub(super) fn change_data_request(payload: serde_json::Value) -> Request<Body> {
    Request::post("/changeData")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("changeData request must be valid")
}

pub(super) fn lifecycle_request(
    method: &str,
    uri: &str,
    payload: serde_json::Value,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("lifecycle request must be valid")
}

pub(super) fn lifecycle_object_payload(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "kind": "asset",
        "external_id": "external-1",
        "display_name": "Example object",
        "lifecycle_status": "active",
        "approval_status": "approved",
        "attributes": { "source": "manual" },
        "created_at": "2026-07-29T00:00:00Z",
        "updated_at": "2026-07-29T00:00:00Z"
    })
}

pub(super) fn lifecycle_router() -> Router {
    app(AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
        article_runner: None,
    })
}

#[derive(Clone)]
pub(super) struct SchedulerProvider {
    pub(super) platform: Platform,
    pub(super) availability: matrixpost_core::ProviderAvailability,
    pub(super) outcome: DispatchOutcome,
    pub(super) observed: Arc<Mutex<Vec<PublishRequest>>>,
}

impl matrixpost_core::PublishProvider for SchedulerProvider {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn availability(&self) -> matrixpost_core::ProviderAvailability {
        self.availability.clone()
    }

    fn enqueue(&self, request: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        self.observed.lock().unwrap().push(request.clone());
        Ok(self.outcome.clone())
    }
}

/// Test-only persistence seam proving a failed terminal write does not
/// stop subsequent claimed work or strand the failed claim.
pub(super) struct FailFirstCompletionRepository {
    inner: SqliteRepository,
    remaining_failures: AtomicUsize,
}

impl FailFirstCompletionRepository {
    pub(super) fn new() -> Self {
        Self {
            inner: SqliteRepository::in_memory().unwrap(),
            remaining_failures: AtomicUsize::new(1),
        }
    }
}

impl Repository for FailFirstCompletionRepository {
    fn save_account(&self, account: &matrixpost_core::Account) -> Result<(), DomainError> {
        self.inner.save_account(account)
    }

    fn accounts(&self) -> Result<Vec<matrixpost_core::Account>, DomainError> {
        self.inner.accounts()
    }

    fn save_article_account(
        &self,
        account: &matrixpost_core::ArticleAccount,
    ) -> Result<(), DomainError> {
        self.inner.save_article_account(account)
    }

    fn article_accounts(&self) -> Result<Vec<matrixpost_core::ArticleAccount>, DomainError> {
        self.inner.article_accounts()
    }

    fn append_history(&self, record: &matrixpost_core::HistoryRecord) -> Result<(), DomainError> {
        self.inner.append_history(record)
    }

    fn history(&self) -> Result<Vec<matrixpost_core::HistoryRecord>, DomainError> {
        self.inner.history()
    }

    fn insert_job(&self, job: &matrixpost_core::ScheduledJob) -> Result<(), DomainError> {
        self.inner.insert_job(job)
    }

    fn job(&self, id: &str) -> Result<Option<matrixpost_core::ScheduledJob>, DomainError> {
        self.inner.job(id)
    }

    fn transition_job(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ScheduledJob, DomainError> {
        self.inner
            .transition_job(id, expected_revision, next, updated_at)
    }

    fn complete_job_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: chrono::DateTime<chrono::Utc>,
        detail: Option<&str>,
    ) -> Result<
        (
            matrixpost_core::ScheduledJob,
            matrixpost_core::HistoryRecord,
        ),
        DomainError,
    > {
        if self
            .remaining_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(DomainError::Database(
                "injected terminal write failure".into(),
            ));
        }
        self.inner
            .complete_job_with_history(id, expected_revision, next, updated_at, detail)
    }

    fn set_config(&self, key: &str, value: &str) -> Result<(), DomainError> {
        self.inner.set_config(key, value)
    }

    fn config(&self, key: &str) -> Result<Option<String>, DomainError> {
        self.inner.config(key)
    }

    fn delete_config(&self, key: &str) -> Result<bool, DomainError> {
        self.inner.delete_config(key)
    }

    fn article_history(&self) -> Result<Vec<matrixpost_core::ArticleHistoryRecord>, DomainError> {
        self.inner.article_history()
    }
}

impl PublicationQueue for FailFirstCompletionRepository {
    fn enqueue(
        &self,
        request: &PublishRequest,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ScheduledJob, DomainError> {
        self.inner.enqueue(request, now)
    }

    fn advance(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ScheduledJob, DomainError> {
        self.inner.advance(id, expected_revision, next, now)
    }

    fn claim_due(
        &self,
        due_through: &LocalSchedule,
        now: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<matrixpost_core::ScheduledJob>, DomainError> {
        self.inner.claim_due(due_through, now, limit)
    }

    fn requeue_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ScheduledJob, DomainError> {
        self.inner.requeue_claim(id, expected_revision, now)
    }
}

impl ArticlePublicationQueue for FailFirstCompletionRepository {
    fn enqueue_article(
        &self,
        request: &matrixpost_core::PublishArticleRequest,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ArticleScheduledJob, DomainError> {
        self.inner.enqueue_article(request, now)
    }

    fn claim_due_articles(
        &self,
        due_through: &LocalSchedule,
        now: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<matrixpost_core::ArticleScheduledJob>, DomainError> {
        self.inner.claim_due_articles(due_through, now, limit)
    }

    fn complete_article_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: chrono::DateTime<chrono::Utc>,
        detail: Option<&str>,
    ) -> Result<
        (
            matrixpost_core::ArticleScheduledJob,
            matrixpost_core::ArticleHistoryRecord,
        ),
        DomainError,
    > {
        self.inner
            .complete_article_with_history(id, expected_revision, next, updated_at, detail)
    }

    fn requeue_article_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<matrixpost_core::ArticleScheduledJob, DomainError> {
        self.inner.requeue_article_claim(id, expected_revision, now)
    }
}

pub(super) fn scheduled_request() -> PublishRequest {
    PublishRequest {
        source: matrixpost_core::MediaSource::LocalFile("movie.mp4".into()),
        title: "Scheduled title".into(),
        short_title: None,
        tags: Vec::new(),
        address: None,
        draft: false,
        bt2: None,
        scheduled_at: Some(LocalSchedule::parse("2026-07-29 09:00:00").unwrap()),
        task_name: None,
        account: Default::default(),
        wechat_link: Default::default(),
        overrides: Vec::new(),
        targets: vec![Platform::Douyin],
    }
}

pub(super) fn scheduler_state(
    availability: matrixpost_core::ProviderAvailability,
    outcome: DispatchOutcome,
) -> (AppState, Arc<Mutex<Vec<PublishRequest>>>) {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut providers = ProviderRegistry::new();
    providers
        .register(Box::new(SchedulerProvider {
            platform: Platform::Douyin,
            availability,
            outcome,
            observed: Arc::clone(&observed),
        }))
        .unwrap();
    (
        AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(providers),
            article_runner: None,
        },
        observed,
    )
}
