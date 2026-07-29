use std::num::NonZeroUsize;

use chrono::{Duration, Local, TimeZone, Utc};
use clap::Parser;
use matrixpost_core::{
    AccountSelection, HistoryRecord, HistoryStatus, MediaSource, Platform, PublishArticleRequest,
    PublishRequest, PublishState,
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
        Command::Accounts(args) if args.json
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
            phone: None,
            limit: NonZeroUsize::new(50).unwrap(),
            since: None,
            until: None,
            all: false
        })
        .unwrap_err(),
        "days must be greater than zero unless all is true"
    );
}
#[test]
fn account_query_arguments_accept_video_aliases_and_reject_conflicts() {
    let parsed = Cli::try_parse_from([
        "matrixpost",
        "accounts",
        "--json",
        "-p",
        "douyin",
        "--phone",
        "13800138000",
        "--logged-in",
    ])
    .unwrap();
    assert!(matches!(
        parsed.command,
        Command::Accounts(args)
            if args.json
                && args.platform == Some(Platform::Douyin)
                && args.phone.as_deref() == Some("13800138000")
                && args.logged_in
                && !args.logged_out
    ));
    assert!(Cli::try_parse_from(["matrixpost", "accounts", "--platform", "juejin"]).is_err());
    assert!(
        Cli::try_parse_from(["matrixpost", "accounts", "--logged-in", "--logged-out",]).is_err()
    );
}
#[test]
fn history_arguments_support_local_date_phone_and_limit() {
    let parsed = Cli::try_parse_from([
        "matrixpost",
        "history",
        "--phone",
        "13800138000",
        "-n",
        "2",
        "--since",
        "2026-01-01",
        "--until",
        "2026-01-31",
    ])
    .unwrap();
    let Command::History(args) = parsed.command else {
        panic!("expected history")
    };
    assert_eq!(args.phone.as_deref(), Some("13800138000"));
    assert_eq!(args.limit.get(), 2);
    assert_eq!(args.since.unwrap().to_string(), "2026-01-01");
    assert_eq!(args.until.unwrap().to_string(), "2026-01-31");
    assert!(Cli::try_parse_from(["matrixpost", "history", "--since", "01-01-2026"]).is_err());
    assert!(Cli::try_parse_from(["matrixpost", "history", "--since", "2026-1-1"]).is_err());
    assert!(Cli::try_parse_from(["matrixpost", "history", "--until", "2026-01-1"]).is_err());
    assert!(Cli::try_parse_from(["matrixpost", "history", "--until", "+2026-01-01"]).is_err());
    assert!(Cli::try_parse_from(["matrixpost", "history", "--limit", "0"]).is_err());
}
fn history_record(
    id: &str,
    phone: &str,
    platform: Platform,
    state: PublishState,
    recorded_at: chrono::DateTime<Utc>,
) -> HistoryRecord {
    HistoryRecord {
        id: id.into(),
        request: PublishRequest {
            source: MediaSource::LocalFile("movie.mp4".into()),
            title: "title".into(),
            short_title: None,
            tags: Vec::new(),
            address: None,
            draft: false,
            bt2: None,
            scheduled_at: None,
            task_name: None,
            account: AccountSelection {
                phone: Some(phone.into()),
                partition: None,
            },
            wechat_link: Default::default(),
            overrides: Vec::new(),
            targets: vec![platform],
        },
        state,
        recorded_at,
        detail: None,
    }
}
#[test]
fn history_query_composes_filters_sorts_and_date_bounds_override_days() {
    let since = Local::now().date_naive();
    let now = Local
        .from_local_datetime(&since.and_hms_opt(12, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let args = Cli::try_parse_from([
        "matrixpost",
        "history",
        "--days",
        "0",
        "--since",
        &since.to_string(),
        "--until",
        &since.to_string(),
        "--phone",
        "keep",
        "--platform",
        "dy",
        "--status",
        "scheduled",
        "--limit",
        "2",
    ])
    .unwrap();
    let Command::History(args) = args.command else {
        panic!("expected history")
    };
    let retained = parse_history_filter(&args).unwrap().filter(vec![
        history_record("later", "keep", Platform::Douyin, PublishState::Queued, now),
        history_record(
            "earlier",
            "keep",
            Platform::Douyin,
            PublishState::Queued,
            now - Duration::minutes(1),
        ),
        history_record(
            "oldest",
            "keep",
            Platform::Douyin,
            PublishState::Queued,
            now - Duration::minutes(2),
        ),
        history_record(
            "before-range",
            "keep",
            Platform::Douyin,
            PublishState::Queued,
            now - Duration::days(1),
        ),
        history_record(
            "after-range",
            "keep",
            Platform::Douyin,
            PublishState::Queued,
            now + Duration::days(1),
        ),
        history_record(
            "wrong-phone",
            "other",
            Platform::Douyin,
            PublishState::Queued,
            now,
        ),
        history_record(
            "wrong-platform",
            "keep",
            Platform::Bilibili,
            PublishState::Queued,
            now,
        ),
        history_record(
            "wrong-status",
            "keep",
            Platform::Douyin,
            PublishState::Published,
            now,
        ),
    ]);
    assert_eq!(
        retained
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        vec!["later", "earlier"]
    );
}
#[test]
fn history_query_rejects_an_inverted_date_range() {
    let parsed = Cli::try_parse_from([
        "matrixpost",
        "history",
        "--since",
        "2026-02-01",
        "--until",
        "2026-01-31",
    ])
    .unwrap();
    let Command::History(args) = parsed.command else {
        panic!("expected history")
    };
    assert_eq!(
        parse_history_filter(&args).unwrap_err(),
        "since must not be later than until"
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
