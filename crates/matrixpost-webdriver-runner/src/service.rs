use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

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
    TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION, TerminalQrLoginCancelRequest,
    TerminalQrLoginRefreshRequest, TerminalQrLoginRunnerRequest, TerminalQrLoginRunnerResponse,
};
use rand::{Rng, distr::Alphanumeric};
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
    pub(crate) terminal_qr_login_executor: Option<Arc<dyn TerminalQrLoginExecutor>>,
    pub(crate) terminal_qr_attempts: Arc<TerminalQrAttempts>,
    pub(crate) account_status_executor: Option<Arc<dyn AccountStatusExecutor>>,
    pub(crate) review_status_executor: Option<Arc<dyn ReviewStatusExecutor>>,
    pub(crate) article_executor: Option<Arc<dyn ArticlePublicationExecutor>>,
    pub(crate) remote_media: Option<RemoteMediaSupport>,
    pub(crate) browser_debugger_address: Option<SocketAddr>,
    pub(crate) debugger_probe: Arc<dyn BrowserDebuggerProbe>,
}

const TERMINAL_QR_ATTEMPT_LIFETIME: Duration = Duration::from_secs(15 * 60);
const TERMINAL_QR_MAX_ACTIVE_ATTEMPTS: usize = 4;

pub(crate) struct TerminalQrAttempts {
    reserved_slots: AtomicU64,
    lifetime: Duration,
    attempts: Mutex<HashMap<String, TerminalQrAttemptRecord>>,
}

struct TerminalQrAttemptRecord {
    platform: Platform,
    created_at: Instant,
    attempt: Box<dyn TerminalQrLoginAttempt>,
}

impl TerminalQrAttempts {
    pub(crate) fn new() -> Self {
        Self::with_lifetime(TERMINAL_QR_ATTEMPT_LIFETIME)
    }

    pub(crate) fn with_lifetime(lifetime: Duration) -> Self {
        Self {
            reserved_slots: AtomicU64::new(0),
            lifetime,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    fn reserve_slot(&self) -> bool {
        let mut slots = self.reserved_slots.load(Ordering::Acquire);
        loop {
            if slots >= TERMINAL_QR_MAX_ACTIVE_ATTEMPTS as u64 {
                return false;
            }
            match self.reserved_slots.compare_exchange_weak(
                slots,
                slots + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => slots = current,
            }
        }
    }

    fn release_slot(&self) {
        let _ = self
            .reserved_slots
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |slots| {
                slots.checked_sub(1)
            });
    }

    fn remove_expired(&self) {
        let expired = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("terminal QR attempt lock poisoned");
            let now = Instant::now();
            let tokens = attempts
                .iter()
                .filter(|(_, record)| now.duration_since(record.created_at) >= self.lifetime)
                .map(|(token, _)| token.clone())
                .collect::<Vec<_>>();
            tokens
                .into_iter()
                .filter_map(|token| attempts.remove(&token))
                .collect::<Vec<_>>()
        };
        for mut record in expired {
            let _ = record.attempt.close();
            self.release_slot();
        }
    }

    fn schedule_expiry(self: Arc<Self>, token: String) {
        tokio::spawn(async move {
            tokio::time::sleep(self.lifetime).await;
            let expired = {
                let mut attempts = self
                    .attempts
                    .lock()
                    .expect("terminal QR attempt lock poisoned");
                match attempts.get(&token) {
                    Some(record) if record.created_at.elapsed() >= self.lifetime => {
                        attempts.remove(&token)
                    }
                    _ => None,
                }
            };
            if let Some(mut record) = expired {
                let _ = record.attempt.close();
                self.release_slot();
            }
        });
    }

    pub(crate) fn start(
        &self,
        executor: Arc<dyn TerminalQrLoginExecutor>,
        platform: Platform,
    ) -> TerminalQrLoginRunnerResponse {
        self.remove_expired();
        if !self.reserve_slot() {
            return terminal_qr_rejected(platform);
        }
        let mut attempt = match executor.start_terminal_qr_login(platform) {
            Ok(attempt) => attempt,
            Err(_) => {
                self.release_slot();
                return terminal_qr_rejected(platform);
            }
        };
        if attempt.platform() != platform {
            let _ = attempt.close();
            self.release_slot();
            return terminal_qr_rejected(platform);
        }
        let png_base64 = match attempt.capture_qr_png_base64() {
            Ok(png_base64) => png_base64,
            Err(_) => {
                let _ = attempt.close();
                self.release_slot();
                return terminal_qr_rejected(platform);
            }
        };
        let token = match self.attempts.lock() {
            Ok(mut attempts) => {
                let token = terminal_qr_attempt_token(&attempts);
                attempts.insert(
                    token.clone(),
                    TerminalQrAttemptRecord {
                        platform,
                        created_at: Instant::now(),
                        attempt,
                    },
                );
                token
            }
            Err(_) => {
                let _ = attempt.close();
                self.release_slot();
                return terminal_qr_rejected(platform);
            }
        };
        TerminalQrLoginRunnerResponse::QrAvailable {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform,
            attempt_token: token,
            png_base64,
        }
    }

    fn refresh(&self, platform: Platform, token: &str) -> TerminalQrLoginRunnerResponse {
        let mut expired = None;
        let response = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("terminal QR attempt lock poisoned");
            let Some(record) = attempts.get(token) else {
                return terminal_qr_rejected(platform);
            };
            if record.platform != platform {
                return terminal_qr_rejected(platform);
            }
            let is_expired = record.created_at.elapsed() >= self.lifetime;
            if is_expired {
                expired = attempts.remove(token);
                TerminalQrLoginRunnerResponse::TimedOut {
                    version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
                    platform,
                }
            } else {
                let capture = attempts
                    .get_mut(token)
                    .expect("terminal QR attempt disappeared while locked")
                    .attempt
                    .capture_qr_png_base64();
                match capture {
                    Ok(png_base64) => TerminalQrLoginRunnerResponse::QrAvailable {
                        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
                        platform,
                        attempt_token: token.to_owned(),
                        png_base64,
                    },
                    Err(_) => {
                        expired = attempts.remove(token);
                        terminal_qr_rejected(platform)
                    }
                }
            }
        };
        if let Some(mut record) = expired {
            let _ = record.attempt.close();
            self.release_slot();
        }
        response
    }

    pub(crate) fn cancel(&self, platform: Platform, token: &str) -> TerminalQrLoginRunnerResponse {
        let record = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("terminal QR attempt lock poisoned");
            match attempts.get(token) {
                Some(record) if record.platform == platform => attempts.remove(token),
                _ => None,
            }
        };
        let Some(mut record) = record else {
            return terminal_qr_rejected(platform);
        };
        let closed = record.attempt.close().is_ok();
        self.release_slot();
        if !closed {
            return terminal_qr_rejected(platform);
        }
        TerminalQrLoginRunnerResponse::Cancelled {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform,
        }
    }
}

fn terminal_qr_attempt_token(attempts: &HashMap<String, TerminalQrAttemptRecord>) -> String {
    loop {
        let token = rand::rng()
            .sample_iter(Alphanumeric)
            .take(32)
            .map(char::from)
            .collect::<String>();
        if !attempts.contains_key(&token) {
            return token;
        }
    }
}

impl Default for TerminalQrAttempts {
    fn default() -> Self {
        Self::new()
    }
}

fn terminal_qr_rejected(platform: Platform) -> TerminalQrLoginRunnerResponse {
    TerminalQrLoginRunnerResponse::Rejected {
        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        platform,
    }
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

pub(crate) async fn terminal_qr_login(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<TerminalQrLoginRunnerRequest>,
) -> impl IntoResponse {
    if !body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(terminal_qr_rejected(body.platform)),
        );
    }
    let response = match &state.terminal_qr_login_executor {
        Some(executor) => state
            .terminal_qr_attempts
            .start(Arc::clone(executor), body.platform),
        None => TerminalQrLoginRunnerResponse::Unavailable {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
        },
    };
    if let TerminalQrLoginRunnerResponse::QrAvailable { attempt_token, .. } = &response {
        Arc::clone(&state.terminal_qr_attempts).schedule_expiry(attempt_token.clone());
    }
    (StatusCode::OK, Json(response))
}

pub(crate) async fn refresh_terminal_qr_login(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<TerminalQrLoginRefreshRequest>,
) -> impl IntoResponse {
    if !body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(terminal_qr_rejected(body.platform)),
        );
    }
    let response = if state.terminal_qr_login_executor.is_some() {
        state
            .terminal_qr_attempts
            .refresh(body.platform, &body.attempt_token)
    } else {
        TerminalQrLoginRunnerResponse::Unavailable {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
        }
    };
    (StatusCode::OK, Json(response))
}

pub(crate) async fn cancel_terminal_qr_login(
    State(state): State<Arc<RunnerService>>,
    Json(body): Json<TerminalQrLoginCancelRequest>,
) -> impl IntoResponse {
    if !body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(terminal_qr_rejected(body.platform)),
        );
    }
    let response = if state.terminal_qr_login_executor.is_some() {
        state
            .terminal_qr_attempts
            .cancel(body.platform, &body.attempt_token)
    } else {
        TerminalQrLoginRunnerResponse::Unavailable {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: body.platform,
        }
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
        .route("/v1/login/terminal-qr", post(terminal_qr_login))
        .route(
            "/v1/login/terminal-qr/refresh",
            post(refresh_terminal_qr_login),
        )
        .route(
            "/v1/login/terminal-qr/cancel",
            post(cancel_terminal_qr_login),
        )
        .route("/v1/account-status", post(account_status))
        .route("/v1/review-status", post(review_status))
        .with_state(service)
}
