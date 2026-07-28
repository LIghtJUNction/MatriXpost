//! Headless HTTP adapter backed by the durable core repository.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use matrixpost_core::{
    DispatchOutcome, Platform, ProviderDispatchReport, ProviderRegistry, ProviderRunner,
    PublishRequest, Repository, SqliteRepository, UpstreamPublishDto,
};
use serde::{Deserialize, Serialize};

/// Secret-free daemon configuration read from TOML.
#[derive(Debug, Clone, Deserialize)]
struct DaemonConfig {
    #[serde(default = "default_bind")]
    bind: SocketAddr,
    #[serde(default = "default_state_path")]
    state_path: PathBuf,
    /// Local runner declarations. They are validated but never executed here.
    #[serde(default)]
    provider_runners: Vec<ProviderRunner>,
}
fn default_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8788))
}
fn default_state_path() -> PathBuf {
    PathBuf::from("matrixpost.db")
}
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            state_path: default_state_path(),
            provider_runners: Vec::new(),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "matrixpostd",
    version,
    about = "Headless MatriXpost API daemon"
)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    bind: Option<SocketAddr>,
    #[arg(long)]
    state_path: Option<PathBuf>,
}

impl DaemonConfig {
    fn load(args: Args) -> Result<Self, String> {
        let mut config = match args.config {
            Some(path) => toml::from_str(
                &std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
            )
            .map_err(|error| format!("invalid TOML configuration: {error}"))?,
            None => Self::default(),
        };
        if let Some(bind) = args.bind {
            config.bind = bind;
        }
        if let Some(path) = args.state_path {
            config.state_path = path;
        }
        Ok(config)
    }
}

#[derive(Clone)]
struct AppState {
    repository: Arc<SqliteRepository>,
    providers: Arc<ProviderRegistry>,
}
#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    ok: bool,
    outcome: &'static str,
    data: T,
    message: String,
}

fn response(
    status: StatusCode,
    outcome: &'static str,
    data: serde_json::Value,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(ApiResponse {
            ok: status.is_success(),
            outcome,
            data,
            message: message.into(),
        }),
    )
        .into_response()
}
fn invalid(message: impl Into<String>) -> Response {
    response(
        StatusCode::BAD_REQUEST,
        "rejected",
        serde_json::Value::Null,
        message,
    )
}
fn unavailable(data: serde_json::Value) -> Response {
    response(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        data,
        "no provider implementation is configured; no publishing was attempted",
    )
}
/// Maps a provider dispatch report without overstating browser-side success.
fn dispatch_response(request: PublishRequest, report: ProviderDispatchReport) -> Response {
    if report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. }))
    {
        return unavailable(serde_json::json!({ "accepted": false, "request": request }));
    }

    if report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. }))
    {
        return response(
            StatusCode::ACCEPTED,
            "queued",
            serde_json::json!({ "accepted": true, "request": request, "providers": report.outcomes }),
            "local runner completed its WebDriver workflow; remote platform processing is not confirmed",
        );
    }

    response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "rejected",
        serde_json::json!({ "accepted": false, "request": request, "providers": report.outcomes }),
        "provider dispatch was incomplete; no overall publication success is claimed",
    )
}
fn parse_publish(body: Bytes) -> Result<PublishRequest, Box<Response>> {
    let dto = serde_json::from_slice::<UpstreamPublishDto>(&body)
        .map_err(|error| Box::new(invalid(format!("invalid JSON publish request: {error}"))))?;
    PublishRequest::try_from(dto).map_err(|error| Box::new(invalid(error.to_string())))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true, "service": "matrixpostd", "status": "healthy" }))
}
async fn platforms() -> impl IntoResponse {
    Json(
        Platform::ALL
            .iter()
            .map(|item| item.metadata())
            .collect::<Vec<_>>(),
    )
}
async fn providers(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.providers.availability_report())
}
async fn creative_statements() -> Response {
    unavailable(serde_json::json!({ "creative_statements": [] }))
}
async fn change_data(State(state): State<AppState>, body: Bytes) -> Response {
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => return invalid(format!("invalid JSON changeData request: {error}")),
    };
    let action = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let file_name = value
        .get("fileName")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let item = value
        .get("item")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let item_id = item
        .get("id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| item.as_str())
        .map(str::to_owned)
        .unwrap_or_default();
    if !matches!(
        file_name,
        "account" | "pushData" | "config" | "creative-statements" | "platform-options"
    ) || item_id.trim().is_empty()
        || contains_secret(&item)
        || looks_like_secret(file_name)
        || looks_like_secret(&item_id)
    {
        return invalid("changeData fileName or item is not an allowed non-secret domain value");
    }
    let key = format!("{file_name}:{item_id}");
    let result = match action {
        "add" | "update" => serde_json::to_string(&item)
            .map_err(|error| error.to_string())
            .and_then(|item| {
                state
                    .repository
                    .set_config(&key, &item)
                    .map_err(|error| error.to_string())
                    .map(|_| serde_json::json!({"key":key,"updated":true}))
            }),
        "delete" => state
            .repository
            .delete_config(&key)
            .map_err(|error| error.to_string())
            .map(|deleted| serde_json::json!({"key":key,"deleted":deleted})),
        "get" | "config" => state
            .repository
            .config(&key)
            .map_err(|error| error.to_string())
            .map(|value| serde_json::json!({"key":key,"value":value})),
        _ => Err("changeData type must be add, update, delete, get, or config".into()),
    };
    match result {
        Ok(data) => response(StatusCode::OK, "ok", data, "state updated"),
        Err(error) => invalid(error),
    }
}
fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}
fn contains_secret(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(items) => items
            .iter()
            .any(|(key, value)| looks_like_secret(key) || contains_secret(value)),
        serde_json::Value::Array(items) => items.iter().any(contains_secret),
        serde_json::Value::String(value) => looks_like_secret(value),
        _ => false,
    }
}
async fn publish(State(state): State<AppState>, body: Bytes) -> Response {
    match parse_publish(body) {
        Ok(request) => match state.providers.dispatch_all(&request) {
            Ok(report) => dispatch_response(request, report),
            Err(error) => invalid(error.to_string()),
        },
        Err(error) => *error,
    }
}

/// Builds the local HTTP contract used by desktop and server callers.
fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/platforms", get(platforms))
        .route("/providers", get(providers))
        .route("/creative-statements", get(creative_statements))
        .route("/changeData", post(change_data))
        .route("/publish", post(publish))
        .with_state(state)
}

#[tokio::main]
async fn main() -> ExitCode {
    let config = match DaemonConfig::load(Args::parse()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("matrixpostd configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let providers = match ProviderRegistry::from_runners(config.provider_runners) {
        Ok(providers) => Arc::new(providers),
        Err(error) => {
            eprintln!("matrixpostd provider-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let repository = match SqliteRepository::open(&config.state_path) {
        Ok(repository) => Arc::new(repository),
        Err(error) => {
            eprintln!(
                "matrixpostd failed to open {}: {error}",
                config.state_path.display()
            );
            return ExitCode::from(4);
        }
    };
    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("matrixpostd failed to bind {}: {error}", config.bind);
            return ExitCode::from(4);
        }
    };
    match axum::serve(
        listener,
        app(AppState {
            repository,
            providers,
        }),
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("matrixpostd stopped unexpectedly: {error}");
            ExitCode::from(4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    async fn json_response(
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

    fn change_data_request(payload: serde_json::Value) -> Request<Body> {
        Request::post("/changeData")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("changeData request must be valid")
    }

    #[test]
    fn config_defaults_are_secret_free() {
        let config = DaemonConfig::default();
        assert_eq!(config.bind, default_bind());
        assert_eq!(config.state_path, PathBuf::from("matrixpost.db"));
        assert!(config.provider_runners.is_empty());
    }
    #[test]
    fn daemon_config_builds_tcp_runner_execution_registry() {
        let config: DaemonConfig = toml::from_str(
            r#"
            [[provider_runners]]
            platform = 'dy'
            transport = 'tcp'
            address = '127.0.0.1:39001'
            "#,
        )
        .unwrap();
        let registry = ProviderRegistry::from_runners(config.provider_runners)
            .expect("runner declaration must validate");
        assert_eq!(
            registry.availability(Platform::Douyin),
            matrixpost_core::ProviderAvailability::Available
        );
    }
    #[test]
    fn malformed_http_json_is_a_bad_request() {
        assert_eq!(
            parse_publish(Bytes::from_static(b"{"))
                .unwrap_err()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    #[test]
    fn valid_http_request_is_parsed_but_not_queued() {
        let body = Bytes::from_static(br#"{"platform":"dy","file":"movie.mp4","title":"Title"}"#);
        let request = parse_publish(body).expect("valid request must be accepted");
        assert_eq!(request.targets, vec![Platform::Douyin]);
    }
    #[tokio::test]
    async fn all_queued_report_is_accepted_but_mixed_report_fails_closed() {
        let request = parse_publish(Bytes::from_static(
            br#"{"platform":"dy","file":"movie.mp4","title":"Title"}"#,
        ))
        .unwrap();
        let queued = ProviderDispatchReport {
            outcomes: [(
                Platform::Douyin,
                DispatchOutcome::Queued {
                    job_id: "job".into(),
                },
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(
            dispatch_response(request.clone(), queued).status(),
            StatusCode::ACCEPTED
        );
        let mixed = ProviderDispatchReport {
            outcomes: [
                (
                    Platform::Douyin,
                    DispatchOutcome::Queued {
                        job_id: "job".into(),
                    },
                ),
                (
                    Platform::Kuaishou,
                    DispatchOutcome::Rejected {
                        reason: "runner failed".into(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(
            dispatch_response(request, mixed).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    #[test]
    fn platform_contract_is_canonical() {
        assert_eq!(
            Platform::ALL
                .iter()
                .map(|platform| platform.metadata().code)
                .collect::<Vec<_>>(),
            vec!["dy", "sph", "blbl", "bjh", "tt", "ks", "xhs", "fqsp"]
        );
    }
    #[tokio::test]
    async fn change_data_add_get_update_get_delete_get_and_config_roundtrip() {
        for file_name in [
            "account",
            "pushData",
            "config",
            "creative-statements",
            "platform-options",
        ] {
            let state = AppState {
                repository: Arc::new(SqliteRepository::in_memory().unwrap()),
                providers: Arc::new(ProviderRegistry::new()),
            };
            let router = app(state);
            let item = serde_json::json!({ "id": "dy", "platform": "dy", "value": "one" });
            let (status, body) = json_response(
                router.clone(),
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "add",
                    "item": item.clone(),
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["outcome"], "ok");
            assert_eq!(body["data"]["key"], format!("{file_name}:dy"));
            assert_eq!(body["data"]["updated"], true);

            let (status, body) = json_response(
                router.clone(),
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "get",
                    "item": { "id": "dy" },
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["value"], item.to_string());

            let updated = serde_json::json!({ "id": "dy", "platform": "dy", "value": "two" });
            let (status, body) = json_response(
                router.clone(),
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "update",
                    "item": updated.clone(),
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["updated"], true);

            let (status, body) = json_response(
                router.clone(),
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "get",
                    "item": { "id": "dy" },
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["value"], updated.to_string());

            let (status, body) = json_response(
                router.clone(),
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "delete",
                    "item": { "id": "dy" },
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["deleted"], true);

            let (status, body) = json_response(
                router,
                change_data_request(serde_json::json!({
                    "fileName": file_name,
                    "type": "get",
                    "item": { "id": "dy" },
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["value"], serde_json::Value::Null);
        }
    }
    #[tokio::test]
    async fn change_data_rejects_nested_secret_fields() {
        for forbidden in [
            "cookie",
            "token",
            "password",
            "secret",
            "session",
            "authorization",
            "credential",
        ] {
            let router = app(AppState {
                repository: Arc::new(SqliteRepository::in_memory().unwrap()),
                providers: Arc::new(ProviderRegistry::new()),
            });
            let (status, body) = json_response(
                router,
                change_data_request(serde_json::json!({
                    "fileName": "account",
                    "type": "add",
                    "item": { "id": "dy", "nested": [{ (forbidden): "x" }] },
                })),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["ok"], false);
            assert_eq!(body["outcome"], "rejected");
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("allowed non-secret")
            );
        }
    }
    #[tokio::test]
    async fn change_data_rejects_structured_item_without_id() {
        let router = app(AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        });
        let (status, body) = json_response(
            router,
            change_data_request(serde_json::json!({
                "fileName": "account",
                "type": "add",
                "item": { "platform": "dy" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["outcome"], "rejected");
    }
    #[test]
    fn daemon_config_toml_state_path_and_cli_override_precedence() {
        let path =
            std::env::temp_dir().join(format!("matrixpostd-test-{}.toml", std::process::id()));
        std::fs::write(&path, "bind='127.0.0.1:9000'\nstate_path='/tmp/a.db'").unwrap();
        let config = DaemonConfig::load(Args {
            config: Some(path.clone()),
            bind: None,
            state_path: None,
        })
        .unwrap();
        assert_eq!(config.bind, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.state_path, PathBuf::from("/tmp/a.db"));
        let loaded = DaemonConfig::load(Args {
            config: Some(path.clone()),
            bind: Some("127.0.0.1:9001".parse().unwrap()),
            state_path: Some(PathBuf::from("/tmp/b.db")),
        })
        .unwrap();
        assert_eq!(loaded.bind, "127.0.0.1:9001".parse().unwrap());
        assert_eq!(loaded.state_path, PathBuf::from("/tmp/b.db"));
        let _ = std::fs::remove_file(path);
    }
    #[tokio::test]
    async fn router_platform_metadata_publish_503_and_invalid_dto_400() {
        let router = app(AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        });
        let platforms = Request::get("/platforms").body(Body::empty()).unwrap();
        let (status, body) = json_response(router.clone(), platforms).await;
        assert_eq!(status, StatusCode::OK);
        let douyin = body
            .as_array()
            .unwrap()
            .iter()
            .find(|platform| platform["code"] == "dy")
            .expect("Douyin metadata must be present");
        assert_eq!(douyin["name"], "抖音");
        assert!(
            douyin["aliases"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("douyin"))
        );
        let providers = Request::get("/providers").body(Body::empty()).unwrap();
        let (status, body) = json_response(router.clone(), providers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["dy"]["outcome"], "unavailable");
        assert_eq!(body.as_object().unwrap().len(), Platform::ALL.len());
        let publish = Request::post("/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"platforms":[{"platform":"sph","phone":"p"}],"file":"movie.mp4","title":"T","tags":"a,b","creativeStatements":{"视频号":"ok"},"sphProductId":"x","platformOptions":{"sph":{"link":{"type":"product","value":"x"}}}}"#,
            ))
            .unwrap();
        let (status, body) = json_response(router.clone(), publish).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["ok"], false);
        assert_eq!(body["outcome"], "unavailable");
        assert_eq!(body["data"]["accepted"], false);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("no provider implementation")
        );
        let invalid = Request::post("/publish")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let (status, body) = json_response(router, invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], false);
        assert_eq!(body["outcome"], "rejected");
        assert_eq!(body["data"], serde_json::Value::Null);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("invalid JSON publish request")
        );
    }
}
