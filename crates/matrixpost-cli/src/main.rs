//! JSON-first command-line adapter for the portable MatriXpost core.

use std::{path::PathBuf, process::ExitCode, str::FromStr};

use clap::{Args, Parser, Subcommand};
use matrixpost_core::{
    AccountSelection, ArticleDispatchOutcome, ArticleRunner, HistoryFilter, HistoryStatus,
    LocalSchedule, MediaSource, Platform, PlatformOverride, ProviderDispatchReport,
    ProviderRegistry, ProviderRunner, PublishArticleRequest, PublishRequest, Repository,
    SqliteRepository, WechatLink,
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
    let runners = values
        .iter()
        .map(|value| ProviderRunner::parse_cli(value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    ProviderRegistry::from_runners(runners).map_err(|error| error.to_string())
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
            Ok(value) => unavailable(vec![value]),
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
}
