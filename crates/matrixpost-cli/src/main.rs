//! JSON-first command-line adapter for the portable MatriXpost core.

use std::{collections::BTreeMap, path::PathBuf, process::ExitCode, str::FromStr};

use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use matrixpost_core::{
    AccountSelection, ApprovalStatus, ArticleDispatchOutcome, ArticleRunner, BusinessObject,
    BusinessObjectStatus, BusinessRelation, ContentAttribution, HistoryFilter, HistoryStatus,
    LedgerDirection, LedgerEntry, LifecycleRepository, LocalSchedule, ManualLoginOutcome,
    MediaSource, Platform, PlatformOverride, ProviderDispatchReport, ProviderRegistry,
    ProviderRunner, PublishArticleRequest, PublishRequest, Repository, SqliteRepository,
    WechatLink,
};
use serde::Serialize;

/// MatriXpost CLI. Mutating commands never claim that a provider published media.
#[derive(Debug, Parser)]
#[command(name = "matrixpost", version, about)]
struct Cli {
    #[arg(long, global = true, default_value = "matrixpost.db")]
    state_path: PathBuf,
    /// Declare a local runner without executing it: PLATFORM=unix:/path,
    /// PLATFORM=pipe:\\\\.\\pipe\\name, or PLATFORM=tcp:127.0.0.1:PORT.
    #[arg(long, global = true, value_name = "RUNNER")]
    provider_runner: Vec<String>,
    /// Declare the explicit Juejin article runner: tcp:127.0.0.1:PORT.
    #[arg(long, global = true, value_name = "RUNNER")]
    article_runner: Vec<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Login {
        #[arg(short, long)]
        platform: String,
    },
    Publish(PublishArgs),
    #[command(name = "publish-article")]
    PublishArticle {
        #[arg(short, long, alias = "juejin", alias = "掘金")]
        platform: String,
        #[arg(short, long)]
        title: String,
        #[arg(long)]
        phone: Option<String>,
        #[arg(long)]
        partition: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        cover: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long = "publish-at")]
        publish_at: Option<LocalSchedule>,
    },
    Accounts {
        #[arg(long)]
        json: bool,
    },
    History(HistoryArgs),
    /// Show deterministic availability for every supported platform.
    Providers {
        #[arg(long)]
        json: bool,
    },
    /// Manage generic objects, immutable financial entries, and content attribution.
    Lifecycle(LifecycleArgs),
}

#[derive(Debug, Args)]
struct LifecycleArgs {
    #[command(subcommand)]
    command: LifecycleCommand,
}

#[derive(Debug, Subcommand)]
enum LifecycleCommand {
    /// List all generic business objects.
    Objects,
    /// Create or inspect a generic business object.
    Object(ObjectArgs),
    /// List or append immutable ledger entries.
    Ledger(LedgerArgs),
    /// List or create links from published content to an object.
    Attribution(AttributionArgs),
    /// List or create immutable directed links between generic objects.
    Relation(RelationArgs),
    /// Change controlled object lifecycle and approval states.
    Transition(TransitionArgs),
}

#[derive(Debug, Args)]
struct ObjectArgs {
    #[command(subcommand)]
    command: ObjectCommand,
}

#[derive(Debug, Subcommand)]
enum ObjectCommand {
    /// Read one object by stable ID.
    Get {
        #[arg(long)]
        id: String,
    },
    /// Create an object from a caller-defined template kind.
    Create(ObjectCreateArgs),
}

#[derive(Debug, Args)]
struct ObjectCreateArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    kind: String,
    #[arg(long)]
    display_name: String,
    #[arg(long)]
    external_id: Option<String>,
    #[arg(long, default_value = "draft", value_parser = parse_business_object_status)]
    lifecycle_status: BusinessObjectStatus,
    #[arg(long, default_value = "pending", value_parser = parse_approval_status)]
    approval_status: ApprovalStatus,
    /// Object metadata as KEY=VALUE. Repeat the flag for multiple attributes.
    #[arg(long = "attribute", value_name = "KEY=VALUE")]
    attributes: Vec<String>,
}

#[derive(Debug, Args)]
struct LedgerArgs {
    #[command(subcommand)]
    command: LedgerCommand,
}

#[derive(Debug, Subcommand)]
enum LedgerCommand {
    /// List immutable ledger entries for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Append an immutable cost or income entry.
    Add(LedgerAddArgs),
}

#[derive(Debug, Args)]
struct LedgerAddArgs {
    #[arg(long)]
    id: String,
    #[arg(long = "object")]
    business_object_id: String,
    #[arg(long, value_parser = parse_ledger_direction)]
    direction: LedgerDirection,
    #[arg(long)]
    category: String,
    #[arg(long, value_parser = parse_positive_minor_amount)]
    amount_minor: i64,
    #[arg(long, value_parser = parse_currency)]
    currency: String,
    #[arg(long, default_value = "pending", value_parser = parse_approval_status)]
    approval_status: ApprovalStatus,
    #[arg(long, value_parser = parse_rfc3339)]
    occurred_at: Option<DateTime<Utc>>,
    #[arg(long)]
    counterparty: Option<String>,
    #[arg(long)]
    reference: Option<String>,
    #[arg(long)]
    description: Option<String>,
}

#[derive(Debug, Args)]
struct AttributionArgs {
    #[command(subcommand)]
    command: AttributionCommand,
}

#[derive(Debug, Subcommand)]
enum AttributionCommand {
    /// List publication-history links for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Link one existing publication-history record to an object.
    Add(AttributionAddArgs),
}

#[derive(Debug, Args)]
struct AttributionAddArgs {
    #[arg(long = "object")]
    business_object_id: String,
    #[arg(long = "history")]
    history_id: String,
    #[arg(long, value_parser = parse_rfc3339)]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
struct RelationArgs {
    #[command(subcommand)]
    command: RelationCommand,
}

#[derive(Debug, Subcommand)]
enum RelationCommand {
    /// List both incoming and outgoing relations for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Add an immutable directed relation between two existing objects.
    Add(RelationAddArgs),
}

#[derive(Debug, Args)]
struct RelationAddArgs {
    #[arg(long)]
    id: String,
    #[arg(long = "source")]
    source_business_object_id: String,
    #[arg(long = "target")]
    target_business_object_id: String,
    #[arg(long = "type")]
    relation_type: String,
    /// Relation metadata as KEY=VALUE. Repeat the flag for multiple attributes.
    #[arg(long = "attribute", value_name = "KEY=VALUE")]
    attributes: Vec<String>,
}

#[derive(Debug, Args)]
struct TransitionArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    expected_revision: u64,
    #[arg(long, value_parser = parse_business_object_status)]
    lifecycle_status: BusinessObjectStatus,
    #[arg(long, value_parser = parse_approval_status)]
    approval_status: ApprovalStatus,
    #[arg(long, value_parser = parse_rfc3339)]
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
struct HistoryArgs {
    #[arg(long)]
    json: bool,
    /// Number of trailing days; defaults to seven unless --all is supplied.
    #[arg(long)]
    days: Option<u16>,
    /// Exact upstream history platform code (Fanqie video is not part of this query).
    #[arg(long, value_parser = parse_history_platform)]
    platform: Option<Platform>,
    /// One of success, failed, publishing, or scheduled.
    #[arg(long)]
    status: Option<HistoryStatus>,
    /// Return all local history without a trailing-days cutoff.
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct PublishArgs {
    #[arg(short = 'p', long = "platform", required = true)]
    platforms: Vec<String>,
    #[arg(short = 'f', long)]
    file: String,
    #[arg(short = 't', long)]
    title: String,
    #[arg(long = "short-title")]
    short_title: Option<String>,
    #[arg(long = "tags", alias = "bq", value_delimiter = ',')]
    tags: Vec<String>,
    #[arg(long)]
    phone: Option<String>,
    #[arg(long)]
    partition: Option<String>,
    #[arg(long = "name", alias = "book-name")]
    task_name: Option<String>,
    #[arg(long)]
    bt2: Option<String>,
    #[arg(long)]
    address: Option<String>,
    #[arg(long = "publish-at")]
    publish_at: Option<LocalSchedule>,
    #[arg(long)]
    draft: bool,
    #[arg(long = "sph-product-id")]
    sph_product_id: Option<String>,
    #[arg(long = "sph-link-type")]
    sph_link_type: Option<String>,
    #[arg(long = "sph-link-value")]
    sph_link_value: Option<String>,
    /// JSON `PlatformOverride`; repeat once per platform override.
    #[arg(long = "platform-override")]
    platform_overrides: Vec<String>,
    /// Applies the same declaration statement to every selected platform.
    #[arg(long = "creative-statement")]
    creative_statement: Option<String>,
}

#[derive(Serialize)]
struct Output<'a, T: Serialize> {
    ok: bool,
    code: u8,
    result: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}
fn emit<T: Serialize>(code: u8, result: T, message: Option<&str>) -> ExitCode {
    let output = Output {
        ok: code == 0,
        code,
        result,
        message,
    };
    match serde_json::to_string(&output) {
        Ok(text) => println!("{text}"),
        Err(_) => {
            println!(r#"{{"ok":false,"code":4,"result":null,"message":"serialization failure"}}"#)
        }
    }
    ExitCode::from(code)
}
fn unavailable(platforms: Vec<Platform>) -> ExitCode {
    emit(
        3,
        serde_json::json!({ "outcome": "unavailable", "platforms": platforms }),
        Some("no provider implementation is configured; no publishing was attempted"),
    )
}

/// Translates the provider boundary into the stable CLI unavailable response.
///
/// A successful result means the local runner completed its WebDriver workflow,
/// not that a remote platform has finished processing the submission.
fn emit_dispatch(report: ProviderDispatchReport) -> ExitCode {
    let platforms = report.outcomes.keys().copied().collect::<Vec<_>>();
    if report.outcomes.values().all(|outcome| {
        matches!(
            outcome,
            matrixpost_core::DispatchOutcome::Unavailable { .. }
        )
    }) {
        return unavailable(platforms);
    }

    if report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, matrixpost_core::DispatchOutcome::Queued { .. }))
    {
        return emit(
            0,
            serde_json::json!({ "outcome": "queued", "providers": report.outcomes }),
            Some(
                "local runner completed its WebDriver workflow; remote platform processing is not confirmed",
            ),
        );
    }

    emit(
        4,
        serde_json::json!({ "outcome": "rejected", "providers": report.outcomes }),
        Some("provider dispatch was incomplete; no overall publication success is claimed"),
    )
}

fn dispatch_publish(registry: &ProviderRegistry, request: &PublishRequest) -> ExitCode {
    match registry.dispatch_all(request) {
        Ok(report) => emit_dispatch(report),
        Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
    }
}

fn dispatch_article(runner: Option<&ArticleRunner>, request: &PublishArticleRequest) -> ExitCode {
    let Some(runner) = runner else {
        return emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": "juejin" }),
            Some("no article runner is configured; no publishing was attempted"),
        );
    };
    match runner.dispatch(request) {
        Ok(outcome) => emit_article_dispatch_outcome(outcome),
        Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
    }
}

fn emit_article_dispatch_outcome(outcome: ArticleDispatchOutcome) -> ExitCode {
    match outcome {
        ArticleDispatchOutcome::Queued { job_id } => emit(
            0,
            serde_json::json!({ "outcome": "queued", "platform": "juejin", "job_id": job_id }),
            Some(
                "local article runner completed its WebDriver workflow; remote platform processing is not confirmed",
            ),
        ),
        ArticleDispatchOutcome::Unavailable { reason } => emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": "juejin", "reason": reason }),
            Some("article runner was unavailable; no remote publication success is claimed"),
        ),
        ArticleDispatchOutcome::Rejected { reason, .. } => emit(
            4,
            serde_json::json!({ "outcome": "rejected", "platform": "juejin", "reason": reason }),
            Some("article runner dispatch was rejected; no remote publication success is claimed"),
        ),
    }
}

fn provider_registry(values: &[String]) -> Result<ProviderRegistry, String> {
    ProviderRegistry::from_runners(provider_runners(values)?).map_err(|error| error.to_string())
}

fn provider_runners(values: &[String]) -> Result<Vec<ProviderRunner>, String> {
    let runners = values
        .iter()
        .map(|value| ProviderRunner::parse_cli(value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(runners)
}

fn login_runner(runners: &[ProviderRunner], platform: Platform) -> Option<&ProviderRunner> {
    runners.iter().find(|runner| runner.platform == platform)
}

fn dispatch_manual_login(runners: &[ProviderRunner], platform: Platform) -> ExitCode {
    let Some(runner) = login_runner(runners, platform) else {
        return emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": platform }),
            Some("no local runner is configured for this platform; no login was attempted"),
        );
    };
    match runner.request_manual_login() {
        Ok(ManualLoginOutcome::Opened) => emit(
            0,
            serde_json::json!({
                "outcome": "opened",
                "platform": platform,
                "manual_login_required": true,
            }),
            Some("local runner opened the platform page; finish login manually before publishing"),
        ),
        Ok(ManualLoginOutcome::Unavailable) => emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": platform }),
            Some("local runner is unavailable; no login success is asserted"),
        ),
        Ok(ManualLoginOutcome::Rejected) | Err(_) => emit(
            4,
            serde_json::json!({ "outcome": "rejected", "platform": platform }),
            Some("local runner login request was rejected; no login success is asserted"),
        ),
    }
}
fn article_runner(values: &[String]) -> Result<Option<ArticleRunner>, String> {
    match values {
        [] => Ok(None),
        [value] => ArticleRunner::parse_cli(value)
            .map(Some)
            .map_err(|error| error.to_string()),
        _ => Err("--article-runner may be supplied only once".into()),
    }
}
fn parse_history_platform(value: &str) -> Result<Platform, String> {
    let platform = Platform::from_str(value).map_err(|error| error.to_string())?;
    if platform == Platform::FanqieVideo {
        return Err("history platform must be dy, ks, blbl, bjh, tt, sph, or xhs".into());
    }
    Ok(platform)
}

fn parse_business_object_status(value: &str) -> Result<BusinessObjectStatus, String> {
    match value {
        "draft" => Ok(BusinessObjectStatus::Draft),
        "active" => Ok(BusinessObjectStatus::Active),
        "completed" => Ok(BusinessObjectStatus::Completed),
        "archived" => Ok(BusinessObjectStatus::Archived),
        _ => Err("lifecycle status must be draft, active, completed, or archived".into()),
    }
}

fn parse_approval_status(value: &str) -> Result<ApprovalStatus, String> {
    match value {
        "pending" => Ok(ApprovalStatus::Pending),
        "approved" => Ok(ApprovalStatus::Approved),
        "rejected" => Ok(ApprovalStatus::Rejected),
        _ => Err("approval status must be pending, approved, or rejected".into()),
    }
}

fn parse_ledger_direction(value: &str) -> Result<LedgerDirection, String> {
    match value {
        "expense" => Ok(LedgerDirection::Expense),
        "revenue" => Ok(LedgerDirection::Revenue),
        _ => Err("ledger direction must be expense or revenue".into()),
    }
}

fn parse_positive_minor_amount(value: &str) -> Result<i64, String> {
    let amount = value
        .parse::<i64>()
        .map_err(|_| "amount minor must be a positive integer".to_owned())?;
    if amount <= 0 {
        return Err("amount minor must be a positive integer".into());
    }
    Ok(amount)
}

fn parse_currency(value: &str) -> Result<String, String> {
    if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(value.to_owned())
    } else {
        Err("currency must be a three-letter uppercase ISO code".into())
    }
}

fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| "timestamp must use RFC3339 format".into())
}

fn parse_attributes(values: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut attributes = BTreeMap::new();
    for value in values {
        let Some((key, value)) = value.split_once('=') else {
            return Err("attribute must use KEY=VALUE".into());
        };
        if key.trim().is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(format!("attribute key is invalid: {key}"));
        }
        if value.trim().is_empty() {
            return Err(format!("attribute value must not be empty: {key}"));
        }
        if attributes
            .insert(key.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(format!("attribute key is repeated: {key}"));
        }
    }
    Ok(attributes)
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn require_optional_non_empty(field: &str, value: &Option<String>) -> Result<(), String> {
    if let Some(value) = value {
        require_non_empty(field, value)?;
    }
    Ok(())
}

fn lifecycle_input_error(error: String) -> ExitCode {
    emit(2, serde_json::Value::Null, Some(&error))
}

fn lifecycle_repository_error(error: impl ToString) -> ExitCode {
    let error = error.to_string();
    emit(4, serde_json::Value::Null, Some(&error))
}

fn object_or_not_found(
    repository: &impl LifecycleRepository,
    id: &str,
) -> Result<BusinessObject, ExitCode> {
    require_non_empty("object id", id).map_err(lifecycle_input_error)?;
    match repository.business_object(id) {
        Ok(Some(object)) => Ok(object),
        Ok(None) => Err(emit(
            4,
            serde_json::Value::Null,
            Some("business object was not found"),
        )),
        Err(error) => Err(lifecycle_repository_error(error)),
    }
}

fn execute_lifecycle(command: LifecycleCommand, repository: &impl LifecycleRepository) -> ExitCode {
    match command {
        LifecycleCommand::Objects => match repository.business_objects() {
            Ok(objects) => emit(0, serde_json::json!({ "objects": objects }), None),
            Err(error) => lifecycle_repository_error(error),
        },
        LifecycleCommand::Object(args) => match args.command {
            ObjectCommand::Get { id } => match object_or_not_found(repository, &id) {
                Ok(object) => emit(0, serde_json::json!({ "object": object }), None),
                Err(exit_code) => exit_code,
            },
            ObjectCommand::Create(args) => {
                for (field, value) in [
                    ("object id", &args.id),
                    ("object kind", &args.kind),
                    ("object display name", &args.display_name),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                if let Err(error) =
                    require_optional_non_empty("object external id", &args.external_id)
                {
                    return lifecycle_input_error(error);
                }
                let attributes = match parse_attributes(args.attributes) {
                    Ok(attributes) => attributes,
                    Err(error) => return lifecycle_input_error(error),
                };
                let now = Utc::now();
                let object = BusinessObject {
                    id: args.id,
                    kind: args.kind,
                    external_id: args.external_id,
                    display_name: args.display_name,
                    lifecycle_status: args.lifecycle_status,
                    approval_status: args.approval_status,
                    revision: 0,
                    attributes,
                    created_at: now,
                    updated_at: now,
                };
                match repository.insert_business_object(&object) {
                    Ok(()) => emit(0, serde_json::json!({ "object": object }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Ledger(args) => match args.command {
            LedgerCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.ledger_entries(&business_object_id) {
                    Ok(entries) => emit(0, serde_json::json!({ "ledger_entries": entries }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            LedgerCommand::Add(args) => {
                for (field, value) in [
                    ("ledger entry id", &args.id),
                    ("object id", &args.business_object_id),
                    ("ledger category", &args.category),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                for (field, value) in [
                    ("ledger counterparty", &args.counterparty),
                    ("ledger reference", &args.reference),
                    ("ledger description", &args.description),
                ] {
                    if let Err(error) = require_optional_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let now = Utc::now();
                let entry = LedgerEntry {
                    id: args.id,
                    business_object_id: args.business_object_id,
                    direction: args.direction,
                    category: args.category,
                    amount_minor: args.amount_minor,
                    currency: args.currency,
                    occurred_at: args.occurred_at.unwrap_or(now),
                    approval_status: args.approval_status,
                    counterparty: args.counterparty,
                    reference: args.reference,
                    description: args.description,
                    created_at: now,
                };
                match repository.insert_ledger_entry(&entry) {
                    Ok(()) => emit(0, serde_json::json!({ "ledger_entry": entry }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Attribution(args) => match args.command {
            AttributionCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.content_attributions(&business_object_id) {
                    Ok(attributions) => emit(
                        0,
                        serde_json::json!({ "content_attributions": attributions }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            AttributionCommand::Add(args) => {
                for (field, value) in [
                    ("object id", &args.business_object_id),
                    ("history id", &args.history_id),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let attribution = ContentAttribution {
                    business_object_id: args.business_object_id,
                    history_id: args.history_id,
                    created_at: args.created_at.unwrap_or_else(Utc::now),
                };
                match repository.insert_content_attribution(&attribution) {
                    Ok(()) => emit(
                        0,
                        serde_json::json!({ "content_attribution": attribution }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Relation(args) => match args.command {
            RelationCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.business_relations(&business_object_id) {
                    Ok(relations) => emit(
                        0,
                        serde_json::json!({ "business_relations": relations }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            RelationCommand::Add(args) => {
                for (field, value) in [
                    ("business relation id", &args.id),
                    ("source object id", &args.source_business_object_id),
                    ("target object id", &args.target_business_object_id),
                    ("business relation type", &args.relation_type),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let attributes = match parse_attributes(args.attributes) {
                    Ok(attributes) => attributes,
                    Err(error) => return lifecycle_input_error(error),
                };
                let relation = BusinessRelation {
                    id: args.id,
                    source_business_object_id: args.source_business_object_id,
                    target_business_object_id: args.target_business_object_id,
                    relation_type: args.relation_type,
                    attributes,
                    created_at: Utc::now(),
                };
                match repository.insert_business_relation(&relation) {
                    Ok(()) => emit(
                        0,
                        serde_json::json!({ "business_relation": relation }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Transition(args) => {
            if let Err(error) = require_non_empty("object id", &args.id) {
                return lifecycle_input_error(error);
            }
            match repository.transition_business_object(
                &args.id,
                args.expected_revision,
                args.lifecycle_status,
                args.approval_status,
                args.updated_at.unwrap_or_else(Utc::now),
            ) {
                Ok(object) => emit(0, serde_json::json!({ "object": object }), None),
                Err(error) => lifecycle_repository_error(error),
            }
        }
    }
}

fn parse_history_filter(args: &HistoryArgs) -> Result<HistoryFilter, String> {
    HistoryFilter::from_query(
        args.days,
        args.all,
        args.platform,
        args.status,
        chrono::Utc::now(),
    )
    .map_err(|error| error.to_string())
}
fn parse_request(args: PublishArgs) -> Result<PublishRequest, String> {
    let targets = args
        .platforms
        .iter()
        .map(|value| Platform::from_str(value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut overrides = args
        .platform_overrides
        .into_iter()
        .map(|value| {
            serde_json::from_str::<PlatformOverride>(&value)
                .map_err(|error| format!("invalid --platform-override JSON: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(statement) = args.creative_statement {
        for platform in &targets {
            if let Some(override_value) =
                overrides.iter_mut().find(|item| item.platform == *platform)
            {
                override_value.creative_statement = Some(statement.clone());
            } else {
                overrides.push(PlatformOverride {
                    platform: *platform,
                    title: None,
                    short_title: None,
                    tags: None,
                    creative_statement: Some(statement.clone()),
                    account: None,
                    wechat_link: None,
                });
            }
        }
    }
    let source = match url::Url::parse(&args.file) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
        Ok(url) => {
            return Err(format!(
                "unsupported remote source scheme: {}",
                url.scheme()
            ));
        }
        Err(_) => MediaSource::LocalFile(args.file.into()),
    };
    let request = PublishRequest {
        source,
        title: args.title,
        short_title: args.short_title,
        tags: args.tags,
        address: args.address,
        draft: args.draft,
        bt2: args.bt2,
        scheduled_at: args.publish_at,
        task_name: args.task_name,
        account: AccountSelection {
            phone: args.phone,
            partition: args.partition,
        },
        wechat_link: WechatLink {
            product_id: args.sph_product_id,
            link_type: args.sph_link_type,
            link_value: args.sph_link_value,
        },
        overrides,
        targets,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}
fn open(path: PathBuf) -> Result<SqliteRepository, String> {
    SqliteRepository::open(path).map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runners = match provider_runners(&cli.provider_runner) {
        Ok(runners) => runners,
        Err(error) => return emit(2, serde_json::Value::Null, Some(&error)),
    };
    let registry = match provider_registry(&cli.provider_runner) {
        Ok(registry) => registry,
        Err(error) => return emit(2, serde_json::Value::Null, Some(&error)),
    };
    let article_runner = match article_runner(&cli.article_runner) {
        Ok(runner) => runner,
        Err(error) => return emit(2, serde_json::Value::Null, Some(&error)),
    };
    match cli.command {
        Command::Login { platform } => match Platform::from_str(&platform) {
            Ok(value) => dispatch_manual_login(&runners, value),
            Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
        },
        Command::Publish(args) => match parse_request(args) {
            Ok(request) => dispatch_publish(&registry, &request),
            Err(error) => emit(2, serde_json::Value::Null, Some(&error)),
        },
        Command::PublishArticle {
            platform,
            title,
            phone,
            partition,
            content,
            file,
            cover,
            category,
            tags,
            summary,
            publish_at,
        } => {
            let request = PublishArticleRequest {
                platform: platform.clone(),
                account: AccountSelection { phone, partition },
                title,
                content,
                file,
                cover,
                category,
                tags,
                summary,
                scheduled_at: publish_at,
            };
            match request.validate() {
                Ok(()) => dispatch_article(article_runner.as_ref(), &request),
                Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
            }
        }
        Command::Accounts { json: _ } => match open(cli.state_path)
            .and_then(|repository| repository.accounts().map_err(|error| error.to_string()))
        {
            Ok(accounts) => emit(0, serde_json::json!({ "accounts": accounts }), None),
            Err(error) => emit(4, serde_json::Value::Null, Some(&error)),
        },
        Command::History(args) => match parse_history_filter(&args) {
            Ok(filter) => match open(cli.state_path).and_then(|repository| {
                repository
                    .history()
                    .map(|history| filter.filter(history))
                    .map_err(|error| error.to_string())
            }) {
                Ok(history) => emit(0, serde_json::json!({ "history": history }), None),
                Err(error) => emit(4, serde_json::Value::Null, Some(&error)),
            },
            Err(error) => emit(2, serde_json::Value::Null, Some(&error)),
        },
        Command::Providers { json: _ } => emit(0, registry.availability_report(), None),
        Command::Lifecycle(args) => match open(cli.state_path) {
            Ok(repository) => execute_lifecycle(args.command, &repository),
            Err(error) => lifecycle_repository_error(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publish_arguments_preserve_upstream_fields() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "dy",
            "-f",
            "movie.mp4",
            "-t",
            "Title",
            "--short-title",
            "Short",
            "--name",
            "task",
            "--publish-at",
            "2026-01-02 03:04:05",
            "--phone",
            "account",
            "--partition",
            "one",
            "--sph-product-id",
            "p",
            "--creative-statement",
            "original",
        ])
        .unwrap();
        let Command::Publish(args) = parsed.command else {
            panic!("expected publish command")
        };
        let request = parse_request(args).unwrap();
        assert_eq!(request.task_name.as_deref(), Some("task"));
        assert_eq!(request.overrides.len(), 1);
        assert_eq!(request.scheduled_at.unwrap().0, "2026-01-02 03:04:05");
    }
    #[test]
    fn query_commands_accept_json_flag() {
        assert!(matches!(
            Cli::try_parse_from(["matrixpost", "accounts", "--json"])
                .unwrap()
                .command,
            Command::Accounts { json: true }
        ));
        assert!(matches!(
            Cli::try_parse_from(["matrixpost", "history", "--json"])
                .unwrap()
                .command,
            Command::History(HistoryArgs { json: true, .. })
        ));
    }
    #[test]
    fn history_arguments_are_typed_and_invalid_input_is_rejected() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "history",
            "--days",
            "3",
            "--platform",
            "dy",
            "--status",
            "scheduled",
            "--all",
        ])
        .unwrap();
        let Command::History(args) = parsed.command else {
            panic!("expected history")
        };
        assert_eq!(args.days, Some(3));
        assert_eq!(args.platform, Some(Platform::Douyin));
        assert_eq!(args.status, Some(HistoryStatus::Scheduled));
        assert!(args.all);
        assert!(Cli::try_parse_from(["matrixpost", "history", "--platform", "fqsp"]).is_err());
        assert!(Cli::try_parse_from(["matrixpost", "history", "--status", "unknown"]).is_err());
        assert_eq!(
            parse_history_filter(&HistoryArgs {
                json: false,
                days: Some(0),
                platform: None,
                status: None,
                all: false,
            })
            .unwrap_err(),
            "days must be greater than zero unless all is true"
        );
    }
    #[test]
    fn publish_url_bt2_and_fq_reach_typed_request() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "fq",
            "-f",
            "https://example.invalid/v.mp4",
            "-t",
            "T",
            "--bt2",
            "legacy",
        ])
        .unwrap();
        let Command::Publish(args) = parsed.command else {
            panic!("expected publish")
        };
        let request = parse_request(args).unwrap();
        assert!(matches!(request.source, MediaSource::RemoteUrl(_)));
        assert_eq!(request.bt2.as_deref(), Some("legacy"));
        assert_eq!(request.targets, vec![Platform::FanqieVideo]);
    }
    #[test]
    fn article_arguments_reach_typed_request() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "publish-article",
            "-p",
            "juejin",
            "-t",
            "T",
            "--phone",
            "p",
            "--partition",
            "x",
            "--content",
            "body",
            "--cover",
            "cover",
            "--category",
            "cat",
            "--tags",
            "a,b",
            "--summary",
            "sum",
            "--publish-at",
            "2026-01-02 03:04:05",
        ])
        .unwrap();
        let Command::PublishArticle {
            platform,
            title,
            phone,
            partition,
            content,
            file,
            cover,
            category,
            tags,
            summary,
            publish_at,
        } = parsed.command
        else {
            panic!("expected article")
        };
        let request = PublishArticleRequest {
            platform,
            account: AccountSelection { phone, partition },
            title,
            content,
            file,
            cover,
            category,
            tags,
            summary,
            scheduled_at: publish_at,
        };
        assert!(request.validate().is_ok());
        assert_eq!(request.cover.as_deref(), Some("cover"));
    }
    #[test]
    fn empty_registry_keeps_valid_publish_unavailable() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "dy",
            "-f",
            "movie.mp4",
            "-t",
            "Title",
        ])
        .unwrap();
        let Command::Publish(args) = parsed.command else {
            panic!("expected publish command")
        };
        let request = parse_request(args).unwrap();
        let report = ProviderRegistry::new().dispatch_all(&request).unwrap();
        assert_eq!(emit_dispatch(report), ExitCode::from(3));
    }

    #[test]
    fn all_queued_runner_report_is_honestly_accepted_but_mixed_is_rejected() {
        let queued = ProviderDispatchReport {
            outcomes: [(
                Platform::Douyin,
                matrixpost_core::DispatchOutcome::Queued {
                    job_id: "job".into(),
                },
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(emit_dispatch(queued), ExitCode::SUCCESS);
        let mixed = ProviderDispatchReport {
            outcomes: [
                (
                    Platform::Douyin,
                    matrixpost_core::DispatchOutcome::Queued {
                        job_id: "job".into(),
                    },
                ),
                (
                    Platform::Kuaishou,
                    matrixpost_core::DispatchOutcome::Unavailable {
                        reason: "offline".into(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(emit_dispatch(mixed), ExitCode::from(4));
    }

    #[test]
    fn tcp_runner_argument_builds_an_execution_registry() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "--provider-runner",
            "dy=tcp:127.0.0.1:39001",
            "providers",
            "--json",
        ])
        .unwrap();
        assert!(matches!(parsed.command, Command::Providers { json: true }));
        let registry = provider_registry(&parsed.provider_runner).unwrap();
        assert_eq!(
            registry.availability_report()[&Platform::Douyin],
            matrixpost_core::ProviderAvailability::Available
        );
    }

    #[test]
    fn login_parser_selects_only_the_runner_for_its_platform() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "--provider-runner",
            "dy=tcp:127.0.0.1:39001",
            "--provider-runner",
            "ks=unix:/tmp/matrixpost-ks.sock",
            "login",
            "--platform",
            "dy",
        ])
        .unwrap();
        let Command::Login { platform } = parsed.command else {
            panic!("expected login command")
        };
        let platform = Platform::from_str(&platform).unwrap();
        let runners = provider_runners(&parsed.provider_runner).unwrap();
        let selected = login_runner(&runners, platform).unwrap();
        assert_eq!(selected.platform, Platform::Douyin);
        assert_eq!(
            selected.loopback_tcp_address(),
            Some("127.0.0.1:39001".parse().unwrap())
        );
        assert!(login_runner(&runners, Platform::Bilibili).is_none());
    }

    #[test]
    fn login_without_a_loopback_tcp_runner_is_safely_unavailable() {
        let runners = provider_runners(&["dy=unix:/tmp/matrixpost-dy.sock".into()]).unwrap();
        assert_eq!(
            dispatch_manual_login(&runners, Platform::Douyin),
            ExitCode::from(3)
        );
    }

    #[test]
    fn article_runner_argument_is_explicit_loopback_only_and_optional() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "--article-runner",
            "tcp:127.0.0.1:39002",
            "publish-article",
            "-p",
            "juejin",
            "-t",
            "T",
            "--content",
            "body",
        ])
        .unwrap();
        assert!(article_runner(&parsed.article_runner).unwrap().is_some());
        assert!(article_runner(&[]).unwrap().is_none());
        assert!(article_runner(&["tcp:192.0.2.1:39002".into()]).is_err());
    }

    #[test]
    fn article_dispatch_reports_default_unavailable_and_runner_outcomes_honestly() {
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: Default::default(),
            title: "T".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        assert_eq!(dispatch_article(None, &request), ExitCode::from(3));
        assert_eq!(
            emit_article_dispatch_outcome(ArticleDispatchOutcome::Queued {
                job_id: "article-job".into(),
            }),
            ExitCode::SUCCESS
        );
        assert_eq!(
            emit_article_dispatch_outcome(ArticleDispatchOutcome::Unavailable {
                reason: "not enabled".into(),
            }),
            ExitCode::from(3)
        );
        assert_eq!(
            emit_article_dispatch_outcome(ArticleDispatchOutcome::Rejected {
                reason: "schedule unsupported".into(),
                automation_attempted: false,
            }),
            ExitCode::from(4)
        );
    }

    #[test]
    fn runner_argument_never_echoes_a_credential_like_value() {
        let error = provider_registry(&["dy=unix:/run/matrixpost/token.sock".into()])
            .err()
            .unwrap();
        assert_eq!(
            error,
            "provider runner endpoint must not contain credential-like data"
        );
    }

    #[test]
    fn lifecycle_object_create_parses_generic_attributes_and_defaults() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "object",
            "create",
            "--id",
            "campaign-42",
            "--kind",
            "campaign",
            "--display-name",
            "Launch",
            "--attribute",
            "region=cn",
            "--attribute",
            "channel=video",
        ])
        .unwrap();
        let Command::Lifecycle(LifecycleArgs {
            command:
                LifecycleCommand::Object(ObjectArgs {
                    command: ObjectCommand::Create(args),
                }),
        }) = parsed.command
        else {
            panic!("expected lifecycle object create")
        };
        assert_eq!(args.lifecycle_status, BusinessObjectStatus::Draft);
        assert_eq!(args.approval_status, ApprovalStatus::Pending);
        assert_eq!(
            parse_attributes(args.attributes).unwrap(),
            BTreeMap::from([
                ("channel".to_owned(), "video".to_owned()),
                ("region".to_owned(), "cn".to_owned()),
            ])
        );
    }

    #[test]
    fn lifecycle_ledger_parsing_rejects_invalid_direction_and_minor_amount() {
        let valid = Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "ledger",
            "add",
            "--id",
            "entry-1",
            "--object",
            "campaign-42",
            "--direction",
            "expense",
            "--category",
            "media",
            "--amount-minor",
            "1250",
            "--currency",
            "CNY",
        ]);
        assert!(valid.is_ok());
        assert!(
            Cli::try_parse_from([
                "matrixpost",
                "lifecycle",
                "ledger",
                "add",
                "--id",
                "entry-1",
                "--object",
                "campaign-42",
                "--direction",
                "cost",
                "--category",
                "media",
                "--amount-minor",
                "1250",
                "--currency",
                "CNY",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "matrixpost",
                "lifecycle",
                "ledger",
                "add",
                "--id",
                "entry-1",
                "--object",
                "campaign-42",
                "--direction",
                "expense",
                "--category",
                "media",
                "--amount-minor",
                "0",
                "--currency",
                "CNY",
            ])
            .is_err()
        );
    }

    #[test]
    fn lifecycle_attribution_and_transition_arguments_are_typed() {
        let attribution = Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "attribution",
            "add",
            "--object",
            "campaign-42",
            "--history",
            "publication-7",
            "--created-at",
            "2026-07-29T01:02:03Z",
        ])
        .unwrap();
        let Command::Lifecycle(LifecycleArgs {
            command:
                LifecycleCommand::Attribution(AttributionArgs {
                    command: AttributionCommand::Add(args),
                }),
        }) = attribution.command
        else {
            panic!("expected lifecycle attribution add")
        };
        assert_eq!(args.business_object_id, "campaign-42");
        assert_eq!(args.history_id, "publication-7");
        assert_eq!(
            args.created_at.unwrap().to_rfc3339(),
            "2026-07-29T01:02:03+00:00"
        );

        let transition = Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "transition",
            "--id",
            "campaign-42",
            "--expected-revision",
            "7",
            "--lifecycle-status",
            "active",
            "--approval-status",
            "approved",
        ])
        .unwrap();
        let Command::Lifecycle(LifecycleArgs {
            command: LifecycleCommand::Transition(args),
        }) = transition.command
        else {
            panic!("expected lifecycle transition")
        };
        assert_eq!(args.id, "campaign-42");
        assert_eq!(args.expected_revision, 7);
        assert_eq!(args.lifecycle_status, BusinessObjectStatus::Active);
        assert_eq!(args.approval_status, ApprovalStatus::Approved);
    }

    #[test]
    fn lifecycle_relation_arguments_accept_safe_generic_attributes() {
        let parsed = Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "relation",
            "add",
            "--id",
            "interest-1",
            "--source",
            "asset-1",
            "--target",
            "customer-1",
            "--type",
            "customer_interest",
            "--attribute",
            "priority=high",
        ])
        .unwrap();
        let Command::Lifecycle(LifecycleArgs {
            command:
                LifecycleCommand::Relation(RelationArgs {
                    command: RelationCommand::Add(args),
                }),
        }) = parsed.command
        else {
            panic!("expected lifecycle relation add")
        };
        assert_eq!(args.id, "interest-1");
        assert_eq!(args.source_business_object_id, "asset-1");
        assert_eq!(args.target_business_object_id, "customer-1");
        assert_eq!(args.relation_type, "customer_interest");
        assert_eq!(
            parse_attributes(args.attributes).unwrap(),
            BTreeMap::from([("priority".to_owned(), "high".to_owned())])
        );
    }

    #[test]
    fn lifecycle_commands_persist_a_generic_object_and_immutable_ledger_entry() {
        let repository = SqliteRepository::in_memory().unwrap();
        let create = LifecycleCommand::Object(ObjectArgs {
            command: ObjectCommand::Create(ObjectCreateArgs {
                id: "campaign-42".into(),
                kind: "campaign".into(),
                display_name: "Launch".into(),
                external_id: None,
                lifecycle_status: BusinessObjectStatus::Draft,
                approval_status: ApprovalStatus::Pending,
                attributes: vec!["channel=video".into()],
            }),
        });
        assert_eq!(execute_lifecycle(create, &repository), ExitCode::SUCCESS);
        let add = LifecycleCommand::Ledger(LedgerArgs {
            command: LedgerCommand::Add(LedgerAddArgs {
                id: "entry-1".into(),
                business_object_id: "campaign-42".into(),
                direction: LedgerDirection::Expense,
                category: "media".into(),
                amount_minor: 1250,
                currency: "CNY".into(),
                approval_status: ApprovalStatus::Pending,
                occurred_at: None,
                counterparty: None,
                reference: None,
                description: None,
            }),
        });
        assert_eq!(execute_lifecycle(add, &repository), ExitCode::SUCCESS);
        assert_eq!(repository.business_objects().unwrap().len(), 1);
        assert_eq!(repository.ledger_entries("campaign-42").unwrap().len(), 1);
    }

    #[test]
    fn lifecycle_relation_commands_persist_and_list_directed_relations() {
        let repository = SqliteRepository::in_memory().unwrap();
        for (id, kind, display_name) in [
            ("asset-1", "asset", "Asset"),
            ("customer-1", "customer", "Customer"),
        ] {
            assert_eq!(
                execute_lifecycle(
                    LifecycleCommand::Object(ObjectArgs {
                        command: ObjectCommand::Create(ObjectCreateArgs {
                            id: id.into(),
                            kind: kind.into(),
                            display_name: display_name.into(),
                            external_id: None,
                            lifecycle_status: BusinessObjectStatus::Draft,
                            approval_status: ApprovalStatus::Pending,
                            attributes: vec![],
                        }),
                    }),
                    &repository,
                ),
                ExitCode::SUCCESS
            );
        }
        let add = LifecycleCommand::Relation(RelationArgs {
            command: RelationCommand::Add(RelationAddArgs {
                id: "interest-1".into(),
                source_business_object_id: "asset-1".into(),
                target_business_object_id: "customer-1".into(),
                relation_type: "customer_interest".into(),
                attributes: vec!["priority=high".into()],
            }),
        });
        assert_eq!(execute_lifecycle(add, &repository), ExitCode::SUCCESS);
        let list = LifecycleCommand::Relation(RelationArgs {
            command: RelationCommand::List {
                business_object_id: "customer-1".into(),
            },
        });
        assert_eq!(execute_lifecycle(list, &repository), ExitCode::SUCCESS);
        assert_eq!(
            repository.business_relations("customer-1").unwrap().len(),
            1
        );
    }

    #[test]
    fn lifecycle_relation_list_rejects_a_missing_object() {
        let repository = SqliteRepository::in_memory().unwrap();
        let command = LifecycleCommand::Relation(RelationArgs {
            command: RelationCommand::List {
                business_object_id: "missing-object".into(),
            },
        });
        assert_eq!(execute_lifecycle(command, &repository), ExitCode::from(4));
    }

    #[test]
    fn lifecycle_missing_object_is_a_generic_not_found_failure() {
        let repository = SqliteRepository::in_memory().unwrap();
        let command = LifecycleCommand::Object(ObjectArgs {
            command: ObjectCommand::Get {
                id: "missing-object".into(),
            },
        });
        assert_eq!(execute_lifecycle(command, &repository), ExitCode::from(4));
    }
}
