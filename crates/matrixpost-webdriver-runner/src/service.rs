use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use matrixpost_core::{
    ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION, ARTICLE_RUNNER_PROTOCOL_VERSION,
    AccountStatusRunnerRequest, AccountStatusRunnerResponse, ArticlePlatform, ArticleRunnerRequest,
    ArticleRunnerResponse, HttpRemoteMediaStager, LOGIN_RUNNER_PROTOCOL_VERSION,
    LoginRunnerRequest, LoginRunnerResponse, MediaSource, MediaStagingPolicy,
    PROVIDER_RUNNER_PROTOCOL_VERSION, Platform, ProviderRunnerRequest, ProviderRunnerResponse,
    PublishRequest, REVIEW_STATUS_RUNNER_PROTOCOL_VERSION, RemoteMediaRequest, RemoteMediaStager,
    ReviewStatus, ReviewStatusRunnerRequest, ReviewStatusRunnerResponse, StagedMedia,
};
use serde_json::{Value, json};
use url::Url;

use crate::{profiles::*, webdriver::*};

pub(crate) struct RemoteMediaSupport {
    pub(crate) policy: MediaStagingPolicy,
    pub(crate) stager: Arc<dyn RemoteMediaStager>,
}

impl RemoteMediaSupport {
    pub(crate) fn configured(directory: PathBuf) -> Self {
        Self {
            policy: MediaStagingPolicy {
                max_bytes: MAX_REMOTE_VIDEO_BYTES,
                allowed_content_types: REMOTE_VIDEO_CONTENT_TYPES
                    .iter()
                    .map(|item| (*item).to_owned())
                    .collect(),
            },
            stager: Arc::new(HttpRemoteMediaStager::new(directory)),
        }
    }

    pub(crate) fn stage(&self, source: &MediaSource) -> Result<Box<dyn StagedMedia>, String> {
        let MediaSource::RemoteUrl(url) = source else {
            return Err("remote media staging requires an HTTP(S) media URL".into());
        };
        let request = RemoteMediaRequest::new(url.clone(), &self.policy)
            .map_err(|_| "remote media URL is not supported".to_owned())?;
        self.stager
            .stage(&request, &self.policy)
            // Transport errors may contain the submitted URL. The local runner
            // must not reflect it through the provider response.
            .map_err(|_| "remote media staging failed".to_owned())
    }
}

pub(crate) struct RunnerService {
    pub(crate) executor: Option<Arc<dyn PublicationExecutor>>,
    pub(crate) login_executor: Option<Arc<dyn LoginNavigationExecutor>>,
    pub(crate) account_status_executor: Option<Arc<dyn AccountStatusExecutor>>,
    pub(crate) review_status_executor: Option<Arc<dyn ReviewStatusExecutor>>,
    pub(crate) article_executor: Option<Arc<dyn ArticlePublicationExecutor>>,
    pub(crate) remote_media: Option<RemoteMediaSupport>,
    pub(crate) browser_debugger_address: Option<SocketAddr>,
    pub(crate) debugger_probe: Arc<dyn BrowserDebuggerProbe>,
}

pub(crate) trait BrowserDebuggerProbe: Send + Sync {
    fn is_ready(&self, address: SocketAddr) -> bool;
}

pub(crate) struct HttpBrowserDebuggerProbe;

impl BrowserDebuggerProbe for HttpBrowserDebuggerProbe {
    fn is_ready(&self, address: SocketAddr) -> bool {
        if !address.ip().is_loopback() {
            return false;
        }
        let endpoint = format!("http://{address}/json/version");
        ureq::AgentBuilder::new()
            .timeout(DEBUGGER_PROBE_TIMEOUT)
            .build()
            .get(&endpoint)
            .call()
            .ok()
            .and_then(|response| response.into_string().ok())
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .is_some_and(|response| valid_chrome_devtools_version(&response))
    }
}

pub(crate) fn valid_chrome_devtools_version(response: &Value) -> bool {
    response
        .get("Browser")
        .and_then(Value::as_str)
        .is_some_and(|browser| browser.starts_with("Chrome/"))
        && response
            .get("Protocol-Version")
            .and_then(Value::as_str)
            .is_some_and(|version| !version.is_empty())
        && response
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .and_then(|url| Url::parse(url).ok())
            .is_some_and(|url| matches!(url.scheme(), "ws" | "wss"))
}

pub(crate) async fn health(State(state): State<Arc<RunnerService>>) -> impl IntoResponse {
    let browser_debugger_configured = state.browser_debugger_address.is_some();
    let attached_browser = match (state.executor.is_some(), state.browser_debugger_address) {
        (true, Some(address)) => {
            let probe = Arc::clone(&state.debugger_probe);
            tokio::task::spawn_blocking(move || probe.is_ready(address))
                .await
                .unwrap_or(false)
        }
        _ => false,
    };
    Json(
        json!({"ok":true,"service":"matrixpost-webdriver-runner","protocol_version":PROVIDER_RUNNER_PROTOCOL_VERSION,"browser_debugger_configured":browser_debugger_configured,"attached_browser":attached_browser}),
    )
}

pub(crate) async fn publish(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<ProviderRunnerRequest>,
) -> impl IntoResponse {
    if body.version != PROVIDER_RUNNER_PROTOCOL_VERSION
        || !body.request.targets.contains(&body.platform)
        || body.request.validate().is_err()
        || body.request.has_account_routing()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "invalid version, platform, or publish request".into(),
            }),
        );
    }
    let remote_media_requested = matches!(&body.request.source, MediaSource::RemoteUrl(_));
    let response = match &state.executor {
        None => ProviderRunnerResponse::Unavailable {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
            reason: "browser debugger address is not configured; no browser session was started"
                .into(),
        },
        Some(executor) => match publish_with_staged_media(
            executor.as_ref(),
            state.remote_media.as_ref(),
            body.platform,
            &body.request,
        ) {
            Ok(job_id) if !job_id.trim().is_empty() => ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                job_id,
            },
            Ok(_) => ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: if remote_media_requested {
                    REMOTE_MEDIA_EXECUTION_REJECTION.into()
                } else {
                    "runner completed without a valid job identifier".into()
                },
            },
            Err(reason) => ProviderRunnerResponse::Rejected {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: if remote_media_requested {
                    REMOTE_MEDIA_EXECUTION_REJECTION.into()
                } else {
                    reason
                },
            },
        },
    };
    (StatusCode::OK, Json(response))
}

pub(crate) fn publish_with_staged_media(
    executor: &dyn PublicationExecutor,
    remote_media: Option<&RemoteMediaSupport>,
    platform: Platform,
    request: &PublishRequest,
) -> Result<String, String> {
    let MediaSource::RemoteUrl(_) = &request.source else {
        return executor.publish(platform, request);
    };
    let support = remote_media.ok_or_else(|| {
        "remote media staging is disabled; start the runner with --remote-media-dir".to_owned()
    })?;
    // Stage before invoking the executor, which in turn is the only code path
    // allowed to create a WebDriver session.
    let staged = support.stage(&request.source)?;
    let mut local_request = request.clone();
    local_request.source = MediaSource::LocalFile(staged.path().to_path_buf());
    let publish = executor.publish(platform, &local_request);
    let cleanup = staged
        .cleanup()
        .map_err(|_| "staged remote media cleanup failed".to_owned());
    match (publish, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(job_id), Ok(())) => Ok(job_id),
    }
}

pub(crate) async fn publish_article(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<ArticleRunnerRequest>,
) -> impl IntoResponse {
    let platform = body
        .request
        .article_platform()
        .unwrap_or(ArticlePlatform::Juejin);
    if body.version != ARTICLE_RUNNER_PROTOCOL_VERSION
        || body.request.validate().is_err()
        || body.request.has_account_routing()
        || body.request.scheduled_at.is_some()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform,
                reason: "invalid version, Juejin article request, or unsupported article schedule"
                    .into(),
                automation_attempted: false,
            }),
        );
    }
    let response = match &state.article_executor {
        None => ArticleRunnerResponse::Unavailable {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            reason: "browser debugger address is not configured; no browser session was started"
                .into(),
            automation_attempted: false,
        },
        Some(executor) => match executor.publish_article(&body.request) {
            Ok(job_id) if !job_id.trim().is_empty() => ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id,
                automation_attempted: true,
            },
            Ok(_) => ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                reason: "runner completed without a valid job identifier".into(),
                automation_attempted: true,
            },
            Err(error) => ArticleRunnerResponse::Rejected {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                reason: error.reason,
                automation_attempted: error.automation_attempted,
            },
        },
    };
    (StatusCode::OK, Json(response))
}

pub(crate) async fn login(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<LoginRunnerRequest>,
) -> impl IntoResponse {
    if body.version != LOGIN_RUNNER_PROTOCOL_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginRunnerResponse::Rejected {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "invalid manual login request version".into(),
            }),
        );
    }
    let response = match &state.login_executor {
        None => LoginRunnerResponse::Unavailable {
            version: LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
            reason: "manual login navigation is not enabled or no browser session is attached"
                .into(),
        },
        Some(executor) => match executor.open_manual_login(body.platform) {
            Ok(()) => LoginRunnerResponse::Opened {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                manual_login_required: true,
            },
            Err(_) => LoginRunnerResponse::Rejected {
                version: LOGIN_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
                reason: "manual login navigation could not be completed".into(),
            },
        },
    };
    (StatusCode::OK, Json(response))
}

pub(crate) async fn account_status(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<AccountStatusRunnerRequest>,
) -> impl IntoResponse {
    if body.version != ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(AccountStatusRunnerResponse::Rejected {
                version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
            }),
        );
    }
    let response = match &state.account_status_executor {
        None => AccountStatusRunnerResponse::Unavailable {
            version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
        },
        Some(executor) => match executor.account_readiness(body.platform) {
            Ok(true) => AccountStatusRunnerResponse::Ready {
                version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
            },
            Ok(false) => AccountStatusRunnerResponse::NotReady {
                version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
            },
            Err(_) => AccountStatusRunnerResponse::Rejected {
                version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: body.platform,
            },
        },
    };
    (StatusCode::OK, Json(response))
}

pub(crate) async fn review_status(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<ReviewStatusRunnerRequest>,
) -> impl IntoResponse {
    if body.version != REVIEW_STATUS_RUNNER_PROTOCOL_VERSION
        || body.platform != Platform::FanqieVideo
        || !body.validate()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ReviewStatusRunnerResponse::Rejected {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            }),
        );
    }
    let response = match &state.review_status_executor {
        None => ReviewStatusRunnerResponse::Unavailable {
            version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::FanqieVideo,
        },
        Some(executor) => match executor.review_status(&body.title_query) {
            Ok(ReviewStatus::Published) => ReviewStatusRunnerResponse::Published {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            },
            Ok(ReviewStatus::UnderReview) => ReviewStatusRunnerResponse::UnderReview {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            },
            Ok(ReviewStatus::Rejected) => ReviewStatusRunnerResponse::Rejected {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            },
            Ok(ReviewStatus::NotFound) => ReviewStatusRunnerResponse::NotFound {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            },
            Ok(ReviewStatus::Unavailable) | Err(_) => ReviewStatusRunnerResponse::Rejected {
                version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
                platform: Platform::FanqieVideo,
            },
        },
    };
    (StatusCode::OK, Json(response))
}

pub(crate) fn app(service: Arc<RunnerService>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/publish", post(publish))
        .route("/v1/publish-article", post(publish_article))
        .route("/v1/login", post(login))
        .route("/v1/account-status", post(account_status))
        .route("/v1/review-status", post(review_status))
        .with_state(service)
}
