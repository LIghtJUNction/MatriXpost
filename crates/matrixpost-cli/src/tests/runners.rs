use std::{cell::Cell, process::ExitCode};

use clap::Parser;
use matrixpost_core::{AccountReadiness, ArticleDispatchOutcome, Platform, ProviderRunner};

use crate::{
    app::dispatch_article,
    args::{Cli, Command},
    output::emit_article_dispatch_outcome,
    runners::{
        accounts_with_query_readiness_using, article_runner, dispatch_fanqie_review_status,
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
fn account(id: &str, platform: Platform, phone: &str) -> matrixpost_core::Account {
    matrixpost_core::Account {
        id: id.into(),
        platform,
        display_name: "local".into(),
        status: matrixpost_core::AccountStatus::LoggedIn,
        phone: phone.into(),
        partition: "persist:local".into(),
    }
}
fn account_args(arguments: &[&str]) -> crate::args::AccountsArgs {
    let parsed = Cli::try_parse_from(arguments).unwrap();
    let Command::Accounts(args) = parsed.command else {
        panic!("expected accounts")
    };
    args
}
#[test]
fn accounts_keep_persisted_fields_and_project_safe_runner_readiness() {
    let account = account("account-1", Platform::Douyin, "123");
    let args = account_args(&["matrixpost", "accounts"]);
    let unavailable =
        accounts_with_query_readiness_using(vec![account.clone()], &[], &args, |_| {
            panic!("no runner must not probe")
        });
    assert_eq!(unavailable[0]["id"], "account-1");
    assert_eq!(unavailable[0]["readiness"], "unavailable");
    let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
    let ready = accounts_with_query_readiness_using(vec![account], &[runner], &args, |_| {
        AccountReadiness::Ready
    });
    assert_eq!(ready[0]["readiness"], "ready");
    assert!(ready[0].get("cookie").is_none());
}
#[test]
fn account_metadata_filter_precedes_readiness_probe() {
    let args = account_args(&[
        "matrixpost",
        "accounts",
        "--platform",
        "dy",
        "--phone",
        "keep",
    ]);
    let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
    let probes = Cell::new(0);
    let accounts = accounts_with_query_readiness_using(
        vec![
            account("keep", Platform::Douyin, "keep"),
            account("wrong-phone", Platform::Douyin, "skip"),
            account("wrong-platform", Platform::Kuaishou, "keep"),
        ],
        &[runner],
        &args,
        |_| {
            probes.set(probes.get() + 1);
            AccountReadiness::Ready
        },
    );
    assert_eq!(probes.get(), 1);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["id"], "keep");
}
#[test]
fn account_readiness_filters_only_retain_their_explicit_state() {
    let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
    let logged_in = account_args(&["matrixpost", "accounts", "--logged-in"]);
    let ready = accounts_with_query_readiness_using(
        vec![account("ready", Platform::Douyin, "1")],
        std::slice::from_ref(&runner),
        &logged_in,
        |_| AccountReadiness::Ready,
    );
    assert_eq!(ready.len(), 1);
    let rejected = accounts_with_query_readiness_using(
        vec![account("rejected", Platform::Douyin, "1")],
        &[runner],
        &logged_in,
        |_| AccountReadiness::Rejected,
    );
    let unavailable = accounts_with_query_readiness_using(
        vec![account("unavailable", Platform::Kuaishou, "1")],
        &[],
        &logged_in,
        |_| panic!("missing runner must not probe"),
    );
    assert!(rejected.is_empty());
    assert!(unavailable.is_empty());
    let logged_out = account_args(&["matrixpost", "accounts", "--logged-out"]);
    let not_ready = accounts_with_query_readiness_using(
        vec![account("not-ready", Platform::Douyin, "1")],
        &[ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap()],
        &logged_out,
        |_| AccountReadiness::NotReady,
    );
    assert_eq!(not_ready.len(), 1);
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
    let Command::Login {
        platform,
        terminal_qr,
    } = parsed.command
    else {
        panic!("expected login command")
    };
    assert!(!terminal_qr);
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
