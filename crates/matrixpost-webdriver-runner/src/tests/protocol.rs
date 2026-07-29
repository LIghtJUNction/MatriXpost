use super::support::*;
use crate::{config::*, profiles::*, service::*, webdriver::*};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use matrixpost_core::*;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tower::ServiceExt;
use url::Url;

pub(crate) struct Accepted;
impl PublicationExecutor for Accepted {
    fn publish(&self, _: Platform, _: &PublishRequest) -> Result<String, String> {
        Ok("job-1".into())
    }
}
impl ArticlePublicationExecutor for Accepted {
    fn publish_article(&self, _: &PublishArticleRequest) -> Result<String, ArticleExecutionError> {
        Ok("article-job-1".into())
    }
}

struct RecordingPublicationExecutor {
    calls: AtomicU64,
    local_paths: Mutex<Vec<PathBuf>>,
    fail: bool,
}

impl RecordingPublicationExecutor {
    fn new(fail: bool) -> Self {
        Self {
            calls: AtomicU64::new(0),
            local_paths: Mutex::new(Vec::new()),
            fail,
        }
    }
}

impl PublicationExecutor for RecordingPublicationExecutor {
    fn publish(&self, _: Platform, request: &PublishRequest) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let MediaSource::LocalFile(path) = &request.source else {
            return Err("remote media reached WebDriver executor".into());
        };
        self.local_paths.lock().unwrap().push(path.clone());
        if self.fail {
            Err("mock WebDriver upload failure".into())
        } else {
            Ok("job-1".into())
        }
    }
}

struct SentinelFailureExecutor;

impl PublicationExecutor for SentinelFailureExecutor {
    fn publish(&self, _: Platform, _: &PublishRequest) -> Result<String, String> {
        Err(
            "webdriver failed for https://example.invalid/video.mp4 at /private/staging/video.mp4"
                .into(),
        )
    }
}

struct TestStagedMedia {
    path: PathBuf,
    cleanup_attempted: Arc<AtomicBool>,
    cleanup_fails: bool,
}

impl StagedMedia for TestStagedMedia {
    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(self: Box<Self>) -> Result<(), matrixpost_core::DomainError> {
        self.cleanup_attempted.store(true, Ordering::Relaxed);
        if self.cleanup_fails {
            Err(matrixpost_core::DomainError::RemoteMedia(
                "mock cleanup failure".into(),
            ))
        } else {
            Ok(())
        }
    }
}

struct TestRemoteMediaStager {
    path: PathBuf,
    stage_calls: AtomicU64,
    cleanup_attempted: Arc<AtomicBool>,
    stage_fails: bool,
    cleanup_fails: bool,
}

impl TestRemoteMediaStager {
    fn succeeding(path: PathBuf) -> Self {
        Self {
            path,
            stage_calls: AtomicU64::new(0),
            cleanup_attempted: Arc::new(AtomicBool::new(false)),
            stage_fails: false,
            cleanup_fails: false,
        }
    }
}

impl RemoteMediaStager for TestRemoteMediaStager {
    fn stage(
        &self,
        _: &RemoteMediaRequest,
        _: &dyn matrixpost_core::RemoteMediaPolicy,
    ) -> Result<Box<dyn StagedMedia>, matrixpost_core::DomainError> {
        self.stage_calls.fetch_add(1, Ordering::Relaxed);
        if self.stage_fails {
            return Err(matrixpost_core::DomainError::RemoteMedia(
                "raw remote URL must not escape".into(),
            ));
        }
        Ok(Box::new(TestStagedMedia {
            path: self.path.clone(),
            cleanup_attempted: Arc::clone(&self.cleanup_attempted),
            cleanup_fails: self.cleanup_fails,
        }))
    }
}

fn test_remote_media_support(stager: Arc<dyn RemoteMediaStager>) -> RemoteMediaSupport {
    RemoteMediaSupport {
        policy: MediaStagingPolicy {
            max_bytes: MAX_REMOTE_VIDEO_BYTES,
            allowed_content_types: REMOTE_VIDEO_CONTENT_TYPES
                .iter()
                .map(|item| (*item).to_owned())
                .collect(),
        },
        stager,
    }
}

pub(crate) struct AcceptedLogin;

impl LoginNavigationExecutor for AcceptedLogin {
    fn open_manual_login(&self, _: Platform) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) struct FailingLogin;

impl LoginNavigationExecutor for FailingLogin {
    fn open_manual_login(&self, _: Platform) -> Result<(), String> {
        Err("raw webdriver failure".into())
    }
}

struct StaticAccountStatus(bool);
impl AccountStatusExecutor for StaticAccountStatus {
    fn account_readiness(&self, _: Platform) -> Result<bool, String> {
        Ok(self.0)
    }
}

struct StaticReviewStatus(ReviewStatus);
impl ReviewStatusExecutor for StaticReviewStatus {
    fn review_status(&self, _: &str) -> Result<ReviewStatus, String> {
        Ok(self.0)
    }
}

#[tokio::test]
async fn account_status_endpoint_is_safe_and_reports_unavailable_without_executor() {
    let unavailable = app(Arc::new(runner_service(None, None)))
        .oneshot(
            Request::post("/v1/account-status")
                .header("content-type", "application/json")
                .body(Body::from(json!({"version":1,"platform":"dy"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<AccountStatusRunnerResponse>(
            &to_bytes(unavailable.into_body(), usize::MAX).await.unwrap()
        )
        .unwrap(),
        AccountStatusRunnerResponse::Unavailable {
            version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin
        }
    );
    let ready_service = RunnerService {
        executor: None,
        login_executor: None,
        account_status_executor: Some(Arc::new(StaticAccountStatus(true))),
        review_status_executor: None,
        article_executor: None,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    };
    let ready = app(Arc::new(ready_service))
        .oneshot(
            Request::post("/v1/account-status")
                .header("content-type", "application/json")
                .body(Body::from(json!({"version":1,"platform":"dy"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<AccountStatusRunnerResponse>(
            &to_bytes(ready.into_body(), usize::MAX).await.unwrap()
        )
        .unwrap(),
        AccountStatusRunnerResponse::Ready {
            version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin
        }
    );
}

#[tokio::test]
async fn review_status_endpoint_is_opt_in_safe_and_rejects_non_fanqie_requests() {
    let unavailable = app(Arc::new(runner_service(None, None)))
        .oneshot(
            Request::post("/v1/review-status")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"version":1,"platform":"fqsp","title_query":"title"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<ReviewStatusRunnerResponse>(
            &to_bytes(unavailable.into_body(), usize::MAX).await.unwrap()
        )
        .unwrap(),
        ReviewStatusRunnerResponse::Unavailable {
            version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::FanqieVideo,
        }
    );
    let service = RunnerService {
        executor: None,
        login_executor: None,
        account_status_executor: None,
        review_status_executor: Some(Arc::new(StaticReviewStatus(ReviewStatus::Published))),
        article_executor: None,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    };
    let rejected = app(Arc::new(service))
        .oneshot(
            Request::post("/v1/review-status")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"version":1,"platform":"dy","title_query":"title"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_slice::<ReviewStatusRunnerResponse>(
            &to_bytes(rejected.into_body(), usize::MAX).await.unwrap()
        )
        .unwrap(),
        ReviewStatusRunnerResponse::Rejected {
            version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::FanqieVideo,
        }
    );
}

pub(crate) struct FailingArticleExecutor;

impl ArticlePublicationExecutor for FailingArticleExecutor {
    fn publish_article(&self, _: &PublishArticleRequest) -> Result<String, ArticleExecutionError> {
        Err(ArticleExecutionError::attempted("mock automation failure"))
    }
}

pub(crate) struct LocalValidationArticleExecutor;

impl ArticlePublicationExecutor for LocalValidationArticleExecutor {
    fn publish_article(&self, _: &PublishArticleRequest) -> Result<String, ArticleExecutionError> {
        Err(ArticleExecutionError::local(
            "mock local validation failure",
        ))
    }
}

pub(crate) struct CountingArticleExecutor(pub(crate) AtomicU64);

impl ArticlePublicationExecutor for CountingArticleExecutor {
    fn publish_article(&self, _: &PublishArticleRequest) -> Result<String, ArticleExecutionError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok("article-job-1".into())
    }
}
#[tokio::test]
async fn protocol_accepts_only_versioned_targeted_requests() {
    let router = app(Arc::new(runner_service(Some(Arc::new(Accepted)), None)));
    let runner_request = ProviderRunnerRequest {
        version: PROVIDER_RUNNER_PROTOCOL_VERSION,
        platform: Platform::Douyin,
        request: request(),
    };
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&runner_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap(),
        ProviderRunnerResponse::Queued {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            job_id: "job-1".into()
        }
    );
    let mut invalid = serde_json::to_value(runner_request).unwrap();
    invalid["version"] = json!(999);
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut routed = request();
    routed.account.phone = Some("runner-forbidden".into());
    let response = router
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ProviderRunnerRequest {
                        version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                        request: routed,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn no_debugger_address_returns_unavailable_without_starting_a_session() {
    let router = app(Arc::new(runner_service(None, None)));
    let request = ProviderRunnerRequest {
        version: PROVIDER_RUNNER_PROTOCOL_VERSION,
        platform: Platform::Douyin,
        request: request(),
    };
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(matches!(
        serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap(),
        ProviderRunnerResponse::Unavailable { .. }
    ));
}

fn remote_request() -> PublishRequest {
    let mut value = request();
    value.source =
        MediaSource::RemoteUrl(Url::parse("https://media.example.invalid/movie.mp4").unwrap());
    value
}

#[test]
fn remote_media_directory_must_be_explicit_and_absolute() {
    assert!(build_remote_media_support(None).unwrap().is_none());
    assert!(build_remote_media_support(Some(PathBuf::from("relative-staging"))).is_err());
    assert!(
        build_remote_media_support(Some(PathBuf::from("/explicit/staging")))
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn remote_media_without_configured_directory_rejects_before_webdriver_session() {
    let executor = Arc::new(RecordingPublicationExecutor::new(false));
    let service = RunnerService {
        executor: Some(executor.clone()),
        login_executor: None,
        account_status_executor: None,
        review_status_executor: None,
        article_executor: None,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    };
    let response = app(Arc::new(service))
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ProviderRunnerRequest {
                        version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                        request: remote_request(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response = serde_json::from_slice::<ProviderRunnerResponse>(&body).unwrap();
    assert!(matches!(response, ProviderRunnerResponse::Rejected { .. }));
    assert_eq!(executor.calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn remote_media_http_rejection_never_reflects_url_or_staged_path() {
    let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
        "/private/staging/video.mp4",
    )));
    let service = RunnerService {
        executor: Some(Arc::new(SentinelFailureExecutor)),
        login_executor: None,
        account_status_executor: None,
        review_status_executor: None,
        article_executor: None,
        remote_media: Some(test_remote_media_support(stager)),
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    };
    let mut request = request();
    request.source =
        MediaSource::RemoteUrl(Url::parse("https://example.invalid/video.mp4").unwrap());
    let response = app(Arc::new(service))
        .oneshot(
            Request::post("/v1/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ProviderRunnerRequest {
                        version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                        request,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let serialized = String::from_utf8(body.to_vec()).unwrap();
    assert!(!serialized.contains("https://example.invalid/video.mp4"));
    assert!(!serialized.contains("/private/staging/video.mp4"));
    assert_eq!(
        serde_json::from_str::<ProviderRunnerResponse>(&serialized).unwrap(),
        ProviderRunnerResponse::Rejected {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            reason: REMOTE_MEDIA_EXECUTION_REJECTION.into(),
        }
    );
}

#[test]
fn configured_remote_media_stages_a_local_path_and_cleans_it_after_webdriver_outcome() {
    let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
        "/explicit/staging/movie.mp4",
    )));
    let executor = RecordingPublicationExecutor::new(false);
    let result = publish_with_staged_media(
        &executor,
        Some(&test_remote_media_support(stager.clone())),
        Platform::Douyin,
        &remote_request(),
    );
    assert_eq!(result.unwrap(), "job-1");
    assert_eq!(stager.stage_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        executor.local_paths.lock().unwrap().as_slice(),
        [PathBuf::from("/explicit/staging/movie.mp4")]
    );
    assert!(stager.cleanup_attempted.load(Ordering::Relaxed));

    let failed_executor = RecordingPublicationExecutor::new(true);
    let stager = Arc::new(TestRemoteMediaStager::succeeding(PathBuf::from(
        "/explicit/staging/failing-movie.mp4",
    )));
    assert!(
        publish_with_staged_media(
            &failed_executor,
            Some(&test_remote_media_support(stager.clone())),
            Platform::Douyin,
            &remote_request(),
        )
        .is_err()
    );
    assert_eq!(failed_executor.calls.load(Ordering::Relaxed), 1);
    assert!(stager.cleanup_attempted.load(Ordering::Relaxed));
}

#[test]
fn remote_staging_fails_closed_before_webdriver_and_cleanup_failure_is_rejected() {
    let failing_stager = Arc::new(TestRemoteMediaStager {
        path: PathBuf::from("/explicit/staging/never-uploaded.mp4"),
        stage_calls: AtomicU64::new(0),
        cleanup_attempted: Arc::new(AtomicBool::new(false)),
        stage_fails: true,
        cleanup_fails: false,
    });
    let executor = RecordingPublicationExecutor::new(false);
    let error = publish_with_staged_media(
        &executor,
        Some(&test_remote_media_support(failing_stager.clone())),
        Platform::Douyin,
        &remote_request(),
    )
    .unwrap_err();
    assert_eq!(error, "remote media staging failed");
    assert_eq!(failing_stager.stage_calls.load(Ordering::Relaxed), 1);
    assert_eq!(executor.calls.load(Ordering::Relaxed), 0);

    let cleanup_failure = Arc::new(TestRemoteMediaStager {
        path: PathBuf::from("/explicit/staging/cleanup-failure.mp4"),
        stage_calls: AtomicU64::new(0),
        cleanup_attempted: Arc::new(AtomicBool::new(false)),
        stage_fails: false,
        cleanup_fails: true,
    });
    let error = publish_with_staged_media(
        &executor,
        Some(&test_remote_media_support(cleanup_failure.clone())),
        Platform::Douyin,
        &remote_request(),
    )
    .unwrap_err();
    assert_eq!(error, "staged remote media cleanup failed");
    assert!(cleanup_failure.cleanup_attempted.load(Ordering::Relaxed));
}
