use std::{path::PathBuf, process::ExitCode, str::FromStr};

use clap::Parser;
use matrixpost_core::{
    AccountSelection, ArticlePublicationQueue, ArticleRunner, Platform, PublishArticleRequest,
    Repository, SqliteRepository,
};

use crate::{
    args::{Cli, Command},
    batch,
    lifecycle::{execute_lifecycle, lifecycle_repository_error},
    output::{emit, emit_article_dispatch_outcome, emit_dispatch},
    query::{parse_history_filter, parse_request},
    runners::{
        accounts_with_query_readiness, article_runner, dispatch_fanqie_review_status,
        dispatch_manual_login, provider_registry, provider_runners,
    },
};

fn open(path: PathBuf) -> Result<SqliteRepository, String> {
    SqliteRepository::open(path).map_err(|error| error.to_string())
}
fn dispatch_publish(
    registry: &matrixpost_core::ProviderRegistry,
    request: &matrixpost_core::PublishRequest,
) -> ExitCode {
    match registry.dispatch_all(request) {
        Ok(report) => emit_dispatch(report),
        Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
    }
}
pub(crate) fn dispatch_article(
    runner: Option<&ArticleRunner>,
    request: &PublishArticleRequest,
) -> ExitCode {
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

pub(crate) fn schedule_article(
    repository: &SqliteRepository,
    request: &PublishArticleRequest,
) -> ExitCode {
    match repository.enqueue_article(request, chrono::Utc::now()) {
        Ok(job) => emit(
            0,
            serde_json::json!({
                "outcome": "scheduled_locally",
                "platform": "juejin",
                "job": { "id": job.id, "state": job.state, "due_at": job.due_at, "revision": job.revision },
            }),
            Some(
                "scheduled article was persisted for local runner work; no remote publishing was attempted",
            ),
        ),
        Err(error) => emit(4, serde_json::Value::Null, Some(&error.to_string())),
    }
}

pub(crate) fn run() -> ExitCode {
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
        Command::Publish(args) => {
            if args.dir.is_some() {
                match batch::prepare(args) {
                    Ok(plan) => batch::dispatch(&registry, plan),
                    Err(error) => emit(2, serde_json::Value::Null, Some(&error)),
                }
            } else {
                match parse_request(args) {
                    Ok(request) => dispatch_publish(&registry, &request),
                    Err(error) => emit(2, serde_json::Value::Null, Some(&error)),
                }
            }
        }
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
                Ok(()) if request.scheduled_at.is_some() => match open(cli.state_path) {
                    Ok(repository) => schedule_article(&repository, &request),
                    Err(error) => emit(4, serde_json::Value::Null, Some(&error)),
                },
                Ok(()) => dispatch_article(article_runner.as_ref(), &request),
                Err(error) => emit(2, serde_json::Value::Null, Some(&error.to_string())),
            }
        }
        Command::Accounts(args) => match open(cli.state_path)
            .and_then(|repository| repository.accounts().map_err(|error| error.to_string()))
        {
            Ok(accounts) => emit(
                0,
                serde_json::json!({ "accounts": accounts_with_query_readiness(accounts, &runners, &args) }),
                None,
            ),
            Err(error) => emit(4, serde_json::Value::Null, Some(&error)),
        },
        Command::ReviewStatus { title } => dispatch_fanqie_review_status(&runners, &title),
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
        Command::ArticleHistory => match open(cli.state_path).and_then(|repository| {
            repository
                .article_history()
                .map_err(|error| error.to_string())
        }) {
            Ok(history) => emit(0, serde_json::json!({ "history": history }), None),
            Err(error) => emit(4, serde_json::Value::Null, Some(&error)),
        },
        Command::Providers { json: _ } => emit(0, registry.availability_report(), None),
        Command::Lifecycle(args) => match open(cli.state_path) {
            Ok(repository) => execute_lifecycle(args.command, &repository),
            Err(error) => lifecycle_repository_error(error),
        },
    }
}
