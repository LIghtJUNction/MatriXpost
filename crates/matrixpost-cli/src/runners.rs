use std::process::ExitCode;

use matrixpost_core::{
    Account, AccountReadiness, ArticleRunner, ManualLoginOutcome, Platform, ProviderRegistry,
    ProviderRunner, ReviewStatus,
};

use crate::output::emit;

pub(crate) fn provider_registry(values: &[String]) -> Result<ProviderRegistry, String> {
    ProviderRegistry::from_runners(provider_runners(values)?).map_err(|error| error.to_string())
}
pub(crate) fn provider_runners(values: &[String]) -> Result<Vec<ProviderRunner>, String> {
    values
        .iter()
        .map(|value| ProviderRunner::parse_cli(value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}
pub(crate) fn login_runner(
    runners: &[ProviderRunner],
    platform: Platform,
) -> Option<&ProviderRunner> {
    runners.iter().find(|runner| runner.platform == platform)
}
pub(crate) fn article_runner(values: &[String]) -> Result<Option<ArticleRunner>, String> {
    match values {
        [] => Ok(None),
        [value] => ArticleRunner::parse_cli(value)
            .map(Some)
            .map_err(|error| error.to_string()),
        _ => Err("--article-runner may be supplied only once".into()),
    }
}
pub(crate) fn dispatch_manual_login(runners: &[ProviderRunner], platform: Platform) -> ExitCode {
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
            serde_json::json!({ "outcome": "opened", "platform": platform, "manual_login_required": true }),
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
pub(crate) fn dispatch_fanqie_review_status(runners: &[ProviderRunner], title: &str) -> ExitCode {
    let Some(runner) = login_runner(runners, Platform::FanqieVideo) else {
        return emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": Platform::FanqieVideo }),
            Some(
                "no local Fanqie runner is configured; no browser review-status probe was attempted",
            ),
        );
    };
    match runner.fanqie_review_status(title) {
        Ok(ReviewStatus::Published) => emit(
            0,
            serde_json::json!({ "outcome": "published", "platform": Platform::FanqieVideo }),
            Some(
                "a matching local video-list card is published-like; this does not prove remote publication acceptance",
            ),
        ),
        Ok(ReviewStatus::UnderReview) => emit(
            0,
            serde_json::json!({ "outcome": "under_review", "platform": Platform::FanqieVideo }),
            Some(
                "a matching local video-list card is under review; no remote publication success is claimed",
            ),
        ),
        Ok(ReviewStatus::NotFound) => emit(
            0,
            serde_json::json!({ "outcome": "not_found", "platform": Platform::FanqieVideo }),
            Some(
                "no matching local video-list card was found; no remote publication success is claimed",
            ),
        ),
        Ok(ReviewStatus::Unavailable) => emit(
            3,
            serde_json::json!({ "outcome": "unavailable", "platform": Platform::FanqieVideo }),
            Some(
                "the local Fanqie runner is unavailable; no browser review-status probe was completed",
            ),
        ),
        Ok(ReviewStatus::Rejected) | Err(_) => emit(
            4,
            serde_json::json!({ "outcome": "rejected", "platform": Platform::FanqieVideo }),
            Some(
                "the local Fanqie review-status probe was rejected; no remote publication success is claimed",
            ),
        ),
    }
}
pub(crate) fn accounts_with_readiness(
    accounts: Vec<Account>,
    runners: &[ProviderRunner],
) -> Vec<serde_json::Value> {
    accounts_with_readiness_using(accounts, runners, |runner| {
        runner
            .account_readiness()
            .unwrap_or(AccountReadiness::Rejected)
    })
}
pub(crate) fn accounts_with_readiness_using<F>(
    accounts: Vec<Account>,
    runners: &[ProviderRunner],
    probe: F,
) -> Vec<serde_json::Value>
where
    F: Fn(&ProviderRunner) -> AccountReadiness,
{
    accounts
        .into_iter()
        .map(|account| {
            let readiness = login_runner(runners, account.platform)
                .map(&probe)
                .unwrap_or(AccountReadiness::Unavailable);
            let mut account = serde_json::to_value(account).expect("account is serializable");
            account
                .as_object_mut()
                .expect("account serializes as an object")
                .insert(
                    "readiness".into(),
                    serde_json::to_value(readiness).expect("readiness is serializable"),
                );
            account
        })
        .collect()
}
