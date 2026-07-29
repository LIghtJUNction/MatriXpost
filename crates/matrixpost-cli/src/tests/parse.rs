use clap::Parser;
use matrixpost_core::{
    AccountSelection, HistoryStatus, MediaSource, Platform, PublishArticleRequest,
};

use crate::{
    args::{Cli, Command, HistoryArgs},
    query::{parse_history_filter, parse_request},
};

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
            all: false
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
    let report = matrixpost_core::ProviderRegistry::new()
        .dispatch_all(&parse_request(args).unwrap())
        .unwrap();
    assert_eq!(
        crate::output::emit_dispatch(report),
        std::process::ExitCode::from(3)
    );
}
#[test]
fn all_queued_runner_report_is_honestly_accepted_but_mixed_is_rejected() {
    let queued = matrixpost_core::ProviderDispatchReport {
        outcomes: [(
            Platform::Douyin,
            matrixpost_core::DispatchOutcome::Queued {
                job_id: "job".into(),
            },
        )]
        .into_iter()
        .collect(),
    };
    assert_eq!(
        crate::output::emit_dispatch(queued),
        std::process::ExitCode::SUCCESS
    );
    let mixed = matrixpost_core::ProviderDispatchReport {
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
    assert_eq!(
        crate::output::emit_dispatch(mixed),
        std::process::ExitCode::from(4)
    );
}
