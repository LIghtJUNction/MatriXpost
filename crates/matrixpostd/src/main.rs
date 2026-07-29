//! Headless HTTP adapter backed by the durable core repository.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use matrixpost_core::{
    ApprovalStatus, BusinessObject, BusinessObjectStatus, BusinessRelation, ContentAttribution,
    DispatchOutcome, DomainError, LedgerEntry, LifecycleRepository, Platform,
    ProviderDispatchReport, ProviderRegistry, ProviderRunner, PublishRequest, Repository,
    SqliteRepository, UpstreamPublishDto,
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

/// Parses a lifecycle payload while rejecting fields the public daemon contract
/// does not recognise. `attributes` remains an opaque map owned by the core.
fn parse_lifecycle_json<T: serde::de::DeserializeOwned>(
    body: Bytes,
    allowed_fields: &[&str],
    resource: &str,
) -> Result<T, Box<Response>> {
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Box::new(invalid(format!("invalid JSON {resource} request: {error}"))))?;
    let fields = value
        .as_object()
        .ok_or_else(|| Box::new(invalid(format!("{resource} request must be a JSON object"))))?;
    if let Some(field) = fields
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(Box::new(invalid(format!(
            "unknown {resource} field: {field}"
        ))));
    }
    serde_json::from_value(value)
        .map_err(|error| Box::new(invalid(format!("invalid JSON {resource} request: {error}"))))
}

fn lifecycle_error() -> Response {
    invalid("lifecycle request could not be completed")
}

/// State changes require the current revision so stale callers cannot overwrite
/// a newer lifecycle or approval decision.
#[derive(Deserialize)]
struct TransitionBusinessObjectRequest {
    expected_revision: u64,
    lifecycle_status: BusinessObjectStatus,
    approval_status: ApprovalStatus,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Camel-case HTTP input for one immutable directed business-object relation.
/// The stored record remains the core model and therefore serializes with its
/// Rust field names on the response.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBusinessRelationRequest {
    id: String,
    source_business_object_id: String,
    target_business_object_id: String,
    relation_type: String,
    #[serde(default)]
    attributes: std::collections::BTreeMap<String, String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn object_not_found() -> Response {
    response(
        StatusCode::NOT_FOUND,
        "not_found",
        serde_json::Value::Null,
        "business object was not found",
    )
}

fn require_business_object(state: &AppState, id: &str) -> Result<(), Box<Response>> {
    match state.repository.business_object(id) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(Box::new(object_not_found())),
        Err(_) => Err(Box::new(lifecycle_error())),
    }
}

async fn list_business_objects(State(state): State<AppState>) -> Response {
    match state.repository.business_objects() {
        Ok(objects) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(objects),
            "business objects listed",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn create_business_object(State(state): State<AppState>, body: Bytes) -> Response {
    let object = match parse_lifecycle_json::<BusinessObject>(
        body,
        &[
            "id",
            "kind",
            "external_id",
            "display_name",
            "lifecycle_status",
            "approval_status",
            "attributes",
            "created_at",
            "updated_at",
        ],
        "business object",
    ) {
        Ok(object) => object,
        Err(response) => return *response,
    };
    match state.repository.insert_business_object(&object) {
        Ok(()) => response(
            StatusCode::CREATED,
            "created",
            serde_json::json!(object),
            "business object created",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn get_business_object(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.repository.business_object(&id) {
        Ok(Some(object)) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(object),
            "business object retrieved",
        ),
        Ok(None) => object_not_found(),
        Err(_) => lifecycle_error(),
    }
}

async fn transition_business_object(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let request = match parse_lifecycle_json::<TransitionBusinessObjectRequest>(
        body,
        &[
            "expected_revision",
            "lifecycle_status",
            "approval_status",
            "updated_at",
        ],
        "business object transition",
    ) {
        Ok(request) => request,
        Err(response) => return *response,
    };

    match state.repository.transition_business_object(
        &id,
        request.expected_revision,
        request.lifecycle_status,
        request.approval_status,
        request.updated_at,
    ) {
        Ok(object) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(object),
            "business object transitioned",
        ),
        Err(DomainError::UnknownBusinessObject(_)) => object_not_found(),
        Err(_) => lifecycle_error(),
    }
}

async fn list_ledger_entries(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    match state.repository.ledger_entries(&id) {
        Ok(entries) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(entries),
            "ledger entries listed",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn create_ledger_entry(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let entry = match parse_lifecycle_json::<LedgerEntry>(
        body,
        &[
            "id",
            "business_object_id",
            "direction",
            "category",
            "amount_minor",
            "currency",
            "occurred_at",
            "approval_status",
            "counterparty",
            "reference",
            "description",
            "created_at",
        ],
        "ledger entry",
    ) {
        Ok(entry) => entry,
        Err(response) => return *response,
    };
    if entry.business_object_id != id {
        return invalid("ledger entry business_object_id must match the path");
    }
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    match state.repository.insert_ledger_entry(&entry) {
        Ok(()) => response(
            StatusCode::CREATED,
            "created",
            serde_json::json!(entry),
            "ledger entry created",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn list_content_attributions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    match state.repository.content_attributions(&id) {
        Ok(attributions) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(attributions),
            "content attributions listed",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn create_content_attribution(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let attribution = match parse_lifecycle_json::<ContentAttribution>(
        body,
        &["business_object_id", "history_id", "created_at"],
        "content attribution",
    ) {
        Ok(attribution) => attribution,
        Err(response) => return *response,
    };
    if attribution.business_object_id != id {
        return invalid("content attribution business_object_id must match the path");
    }
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    match state.repository.insert_content_attribution(&attribution) {
        Ok(()) => response(
            StatusCode::CREATED,
            "created",
            serde_json::json!(attribution),
            "content attribution created",
        ),
        Err(_) => lifecycle_error(),
    }
}

async fn list_business_relations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    match state.repository.business_relations(&id) {
        Ok(relations) => response(
            StatusCode::OK,
            "ok",
            serde_json::json!(relations),
            "business relations listed",
        ),
        Err(DomainError::UnknownBusinessObject(_)) => object_not_found(),
        Err(_) => lifecycle_error(),
    }
}

async fn create_business_relation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let request = match parse_lifecycle_json::<CreateBusinessRelationRequest>(
        body,
        &[
            "id",
            "sourceBusinessObjectId",
            "targetBusinessObjectId",
            "relationType",
            "attributes",
            "createdAt",
        ],
        "business relation",
    ) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    if request.source_business_object_id != id {
        return invalid("business relation sourceBusinessObjectId must match the path");
    }
    if let Err(response) = require_business_object(&state, &id) {
        return *response;
    }
    let relation = BusinessRelation {
        id: request.id,
        source_business_object_id: request.source_business_object_id,
        target_business_object_id: request.target_business_object_id,
        relation_type: request.relation_type,
        attributes: request.attributes,
        created_at: request.created_at.unwrap_or_else(chrono::Utc::now),
    };
    match state.repository.insert_business_relation(&relation) {
        Ok(()) => response(
            StatusCode::CREATED,
            "created",
            serde_json::json!(relation),
            "business relation created",
        ),
        Err(DomainError::UnknownBusinessObject(_)) => object_not_found(),
        Err(_) => lifecycle_error(),
    }
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
        .route(
            "/lifecycle/objects",
            get(list_business_objects).post(create_business_object),
        )
        .route("/lifecycle/objects/{id}", get(get_business_object))
        .route(
            "/lifecycle/objects/{id}/transition",
            post(transition_business_object),
        )
        .route(
            "/lifecycle/objects/{id}/ledger",
            get(list_ledger_entries).post(create_ledger_entry),
        )
        .route(
            "/lifecycle/objects/{id}/attributions",
            get(list_content_attributions).post(create_content_attribution),
        )
        .route(
            "/lifecycle/objects/{id}/relations",
            get(list_business_relations).post(create_business_relation),
        )
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

    fn lifecycle_request(method: &str, uri: &str, payload: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("lifecycle request must be valid")
    }

    fn lifecycle_object_payload(id: &str) -> serde_json::Value {
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

    fn lifecycle_router() -> Router {
        app(AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        })
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

    #[tokio::test]
    async fn lifecycle_object_routes_create_list_and_get() {
        let router = lifecycle_router();
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects",
                lifecycle_object_payload("object-1"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["outcome"], "created");
        assert_eq!(body["data"]["id"], "object-1");

        let (status, body) = json_response(
            router.clone(),
            Request::get("/lifecycle/objects")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"].as_array().unwrap().len(), 1);

        let (status, body) = json_response(
            router,
            Request::get("/lifecycle/objects/object-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["display_name"], "Example object");
    }

    #[tokio::test]
    async fn lifecycle_transition_route_updates_an_object_and_rejects_stale_revisions() {
        let router = lifecycle_router();
        let (status, _) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects",
                lifecycle_object_payload("object-1"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let transition = serde_json::json!({
            "expected_revision": 0,
            "lifecycle_status": "completed",
            "approval_status": "approved",
            "updated_at": "2026-07-29T01:00:00Z"
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects/object-1/transition",
                transition.clone(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "ok");
        assert_eq!(body["data"]["lifecycle_status"], "completed");
        assert_eq!(body["data"]["revision"], 1);

        let (status, body) = json_response(
            router,
            lifecycle_request("POST", "/lifecycle/objects/object-1/transition", transition),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["outcome"], "rejected");
        assert_eq!(body["message"], "lifecycle request could not be completed");
    }

    #[tokio::test]
    async fn lifecycle_transition_route_rejects_unknown_fields_and_missing_objects() {
        let router = lifecycle_router();
        let mut unknown_field = serde_json::json!({
            "expected_revision": 0,
            "lifecycle_status": "completed",
            "approval_status": "approved",
            "updated_at": "2026-07-29T01:00:00Z"
        });
        unknown_field["unexpected"] = serde_json::json!(true);
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects/missing/transition",
                unknown_field,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["outcome"], "rejected");
        assert!(body["message"].as_str().unwrap().contains("unknown"));

        let (status, body) = json_response(
            router,
            lifecycle_request(
                "POST",
                "/lifecycle/objects/missing/transition",
                serde_json::json!({
                    "expected_revision": 0,
                    "lifecycle_status": "completed",
                    "approval_status": "approved",
                    "updated_at": "2026-07-29T01:00:00Z"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["outcome"], "not_found");
        assert_eq!(body["message"], "business object was not found");
    }

    #[tokio::test]
    async fn lifecycle_rejects_unknown_fields_and_unknown_objects_are_404() {
        let router = lifecycle_router();
        let mut payload = lifecycle_object_payload("object-1");
        payload["unexpected"] = serde_json::json!(true);
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects", payload),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["outcome"], "rejected");
        assert!(body["message"].as_str().unwrap().contains("unknown"));

        let (status, body) = json_response(
            router.clone(),
            Request::get("/lifecycle/objects/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["outcome"], "not_found");

        let (status, _) = json_response(
            router,
            Request::get("/lifecycle/objects/missing/ledger")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn lifecycle_ledger_route_rejects_path_mismatch_and_appends_entries() {
        let router = lifecycle_router();
        let (status, _) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects",
                lifecycle_object_payload("object-1"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let mismatched = serde_json::json!({
            "id": "ledger-1", "business_object_id": "other", "direction": "expense",
            "category": "service", "amount_minor": 35000, "currency": "CNY",
            "occurred_at": "2026-07-29T00:00:00Z", "approval_status": "approved",
            "counterparty": null, "reference": null, "description": null,
            "created_at": "2026-07-29T00:00:00Z"
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects/object-1/ledger", mismatched),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["message"].as_str().unwrap().contains("must match"));

        let entry = serde_json::json!({
            "id": "ledger-1", "business_object_id": "object-1", "direction": "expense",
            "category": "service", "amount_minor": 35000, "currency": "CNY",
            "occurred_at": "2026-07-29T00:00:00Z", "approval_status": "approved",
            "counterparty": null, "reference": null, "description": null,
            "created_at": "2026-07-29T00:00:00Z"
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects/object-1/ledger", entry),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["data"]["id"], "ledger-1");

        let (status, body) = json_response(
            router,
            Request::get("/lifecycle/objects/object-1/ledger")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn lifecycle_attribution_route_rejects_path_mismatch_and_missing_history() {
        let router = lifecycle_router();
        let (status, _) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects",
                lifecycle_object_payload("object-1"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects/object-1/attributions",
                serde_json::json!({
                    "business_object_id": "other", "history_id": "history-1",
                    "created_at": "2026-07-29T00:00:00Z"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["message"].as_str().unwrap().contains("must match"));

        let (status, body) = json_response(
            router,
            lifecycle_request(
                "POST",
                "/lifecycle/objects/object-1/attributions",
                serde_json::json!({
                    "business_object_id": "object-1", "history_id": "missing-history",
                    "created_at": "2026-07-29T00:00:00Z"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["outcome"], "rejected");
    }

    #[tokio::test]
    async fn lifecycle_relation_routes_create_and_list_directed_relations() {
        let router = lifecycle_router();
        for (id, external_id) in [
            ("asset-1", "asset-external"),
            ("customer-1", "customer-external"),
        ] {
            let mut object = lifecycle_object_payload(id);
            object["external_id"] = serde_json::json!(external_id);
            let (status, _) = json_response(
                router.clone(),
                lifecycle_request("POST", "/lifecycle/objects", object),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
        }

        let relation = serde_json::json!({
            "id": "relation-1",
            "sourceBusinessObjectId": "asset-1",
            "targetBusinessObjectId": "customer-1",
            "relationType": "customer_interest",
            "attributes": { "priority": "high" }
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects/asset-1/relations", relation),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["data"]["source_business_object_id"], "asset-1");
        assert_eq!(body["data"]["target_business_object_id"], "customer-1");

        let (status, body) = json_response(
            router.clone(),
            Request::get("/lifecycle/objects/asset-1/relations")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"].as_array().unwrap().len(), 1);

        let (status, body) = json_response(
            router,
            Request::get("/lifecycle/objects/customer-1/relations")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn lifecycle_relation_routes_reject_unknown_fields_mismatches_and_missing_objects() {
        let router = lifecycle_router();
        let (status, _) = json_response(
            router.clone(),
            lifecycle_request(
                "POST",
                "/lifecycle/objects",
                lifecycle_object_payload("object-1"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let unknown = serde_json::json!({
            "id": "relation-1",
            "sourceBusinessObjectId": "object-1",
            "targetBusinessObjectId": "missing-target",
            "relationType": "owner",
            "unexpected": true
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects/object-1/relations", unknown),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["message"].as_str().unwrap().contains("unknown"));

        let mismatched = serde_json::json!({
            "id": "relation-1",
            "sourceBusinessObjectId": "other-object",
            "targetBusinessObjectId": "missing-target",
            "relationType": "owner"
        });
        let (status, body) = json_response(
            router.clone(),
            lifecycle_request("POST", "/lifecycle/objects/object-1/relations", mismatched),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["message"].as_str().unwrap().contains("must match"));

        let (status, body) = json_response(
            router.clone(),
            Request::get("/lifecycle/objects/missing/relations")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["outcome"], "not_found");

        let missing_target = serde_json::json!({
            "id": "relation-1",
            "sourceBusinessObjectId": "object-1",
            "targetBusinessObjectId": "missing-target",
            "relationType": "owner"
        });
        let (status, body) = json_response(
            router,
            lifecycle_request(
                "POST",
                "/lifecycle/objects/object-1/relations",
                missing_target,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["outcome"], "not_found");
    }
}
