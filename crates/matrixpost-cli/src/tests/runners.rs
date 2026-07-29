use std::process::ExitCode;

use clap::Parser;
use matrixpost_core::{AccountReadiness, ArticleDispatchOutcome, Platform, ProviderRunner};

use crate::{
    app::dispatch_article,
    args::{Cli, Command},
    output::emit_article_dispatch_outcome,
    runners::{
        accounts_with_readiness_using, article_runner, dispatch_fanqie_review_status,
        dispatch_manual_login, login_runner, provider_registry, provider_runners,
    },
};

#[test]
fn review_status_parser_is_deterministic_and_has_no_runner_fallback() {
    let parsed = Cli::try_parse_from(["matrixpost", "review-status", "--title", "Title"]).unwrap();
    assert!(matches!(parsed.command, Command::ReviewStatus { title } if title == "Title"));
    assert_eq!(
        dispatch_fanqie_review_status(&[], "Title"),
        ExitCode::from(3)
    );
}
#[test]
fn accounts_keep_persisted_fields_and_project_safe_runner_readiness() {
    let account = matrixpost_core::Account {
        id: "account-1".into(),
        platform: Platform::Douyin,
        display_name: "local".into(),
        status: matrixpost_core::AccountStatus::LoggedIn,
        phone: "123".into(),
        partition: "persist:local".into(),
    };
    let unavailable = accounts_with_readiness_using(vec![account.clone()], &[], |_| {
        panic!("no runner must not probe")
    });
    assert_eq!(unavailable[0]["id"], "account-1");
    assert_eq!(unavailable[0]["readiness"], "unavailable");
    let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
    let ready =
        accounts_with_readiness_using(vec![account], &[runner], |_| AccountReadiness::Ready);
    assert_eq!(ready[0]["readiness"], "ready");
    assert!(ready[0].get("cookie").is_none());
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
    assert_eq!(
        provider_registry(&parsed.provider_runner)
            .unwrap()
            .availability_report()[&Platform::Douyin],
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
    let runners = provider_runners(&parsed.provider_runner).unwrap();
    let selected = login_runner(&runners, platform.parse().unwrap()).unwrap();
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
    let request = matrixpost_core::PublishArticleRequest {
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
            job_id: "article-job".into()
        }),
        ExitCode::SUCCESS
    );
    assert_eq!(
        emit_article_dispatch_outcome(ArticleDispatchOutcome::Unavailable {
            reason: "not enabled".into()
        }),
        ExitCode::from(3)
    );
    assert_eq!(
        emit_article_dispatch_outcome(ArticleDispatchOutcome::Rejected {
            reason: "schedule unsupported".into(),
            automation_attempted: false
        }),
        ExitCode::from(4)
    );
}
#[test]
fn runner_argument_never_echoes_a_credential_like_value() {
    assert_eq!(
        provider_registry(&["dy=unix:/run/matrixpost/token.sock".into()])
            .err()
            .unwrap(),
        "provider runner endpoint must not contain credential-like data"
    );
}
