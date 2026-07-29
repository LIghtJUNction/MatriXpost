use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use matrixpost_core::{
    ApprovalStatus, BusinessObject, BusinessObjectStatus, BusinessRelation, ContentAttribution,
    DispatchOutcome, DomainError, LedgerEntry, LifecycleRepository, Platform,
    ProviderDispatchReport, PublishRequest, Repository, UpstreamPublishDto,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

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
pub(crate) fn dispatch_response(
    request: PublishRequest,
    report: ProviderDispatchReport,
) -> Response {
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
pub(crate) fn parse_publish(body: Bytes) -> Result<PublishRequest, Box<Response>> {
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
pub(crate) fn app(state: AppState) -> Router {
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
