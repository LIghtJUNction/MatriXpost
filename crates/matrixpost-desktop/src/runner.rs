use std::collections::BTreeSet;

use chrono::Utc;
use matrixpost_core::{
    AccountReadiness, DispatchOutcome, DomainError, Platform, ProviderRegistry, ProviderRunner,
    ProviderRunnerTransport, PublishRequest, REVIEW_STATUS_TITLE_QUERY_MAX_BYTES, Repository,
    ReviewStatus, SqliteRepository,
};

use crate::{DesktopError, LocalRunnerDispatchOutcome, LocalRunnerDispatchReport};

pub(crate) fn lifecycle_error(error: DomainError) -> DesktopError {
    match error {
        DomainError::UnknownBusinessObject(_) | DomainError::UnknownHistoryRecord(_) => {
            DesktopError::NotFound("the requested lifecycle record does not exist".into())
        }
        _ => DesktopError::InvalidRequest("lifecycle request could not be completed".into()),
    }
}

pub(crate) fn local_probe_runner(
    declaration: Option<&str>,
    platform: Platform,
) -> Result<Option<ProviderRunner>, DesktopError> {
    let Some(declaration) = declaration.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let runner = ProviderRunner::parse_cli(declaration).map_err(|_| {
        DesktopError::InvalidRequest(
            "runner must use the matching PLATFORM=tcp:127.0.0.1:PORT declaration".into(),
        )
    })?;
    if runner.platform != platform
        || !matches!(runner.transport, ProviderRunnerTransport::Tcp { .. })
    {
        return Err(DesktopError::InvalidRequest(
            "runner must use the matching PLATFORM=tcp:127.0.0.1:PORT declaration".into(),
        ));
    }
    Ok(Some(runner))
}

pub(crate) const fn account_readiness_label(readiness: AccountReadiness) -> &'static str {
    match readiness {
        AccountReadiness::Ready => "ready",
        AccountReadiness::NotReady => "not_ready",
        AccountReadiness::Unavailable => "unavailable",
        AccountReadiness::Rejected => "rejected",
    }
}

pub(crate) const fn review_status_label(status: ReviewStatus) -> &'static str {
    match status {
        ReviewStatus::Published => "published",
        ReviewStatus::UnderReview => "under_review",
        ReviewStatus::Rejected => "rejected",
        ReviewStatus::NotFound => "not_found",
        ReviewStatus::Unavailable => "unavailable",
    }
}

pub(crate) fn valid_review_title_query(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    !normalized.is_empty() && normalized.len() <= REVIEW_STATUS_TITLE_QUERY_MAX_BYTES
}

pub(crate) fn local_runner_registry(
    declarations: &[String],
    targets: &[Platform],
) -> Result<ProviderRegistry, DesktopError> {
    let runners = declarations
        .iter()
        .map(|runner| ProviderRunner::parse_cli(runner.trim()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            DesktopError::InvalidRequest(
                "each runner must use PLATFORM=tcp:127.0.0.1:PORT and be local".into(),
            )
        })?;
    let runner_platforms = runners
        .iter()
        .map(|runner| runner.platform)
        .collect::<Vec<_>>();
    let selected_platforms = targets.iter().copied().collect::<BTreeSet<_>>();
    let declared_platforms = runner_platforms.iter().copied().collect::<BTreeSet<_>>();

    if runner_platforms.len() != declared_platforms.len() {
        return Err(DesktopError::InvalidRequest(
            "declare each selected platform at most once".into(),
        ));
    }
    if selected_platforms.len() != targets.len()
        || declared_platforms != selected_platforms
        || runners.len() != targets.len()
    {
        return Err(DesktopError::InvalidRequest(
            "declare exactly one local runner for every selected platform".into(),
        ));
    }
    if runners
        .iter()
        .any(|runner| !matches!(&runner.transport, ProviderRunnerTransport::Tcp { .. }))
    {
        return Err(DesktopError::InvalidRequest(
            "each runner must use PLATFORM=tcp:127.0.0.1:PORT and be local".into(),
        ));
    }
    ProviderRegistry::from_runners(runners).map_err(|_| {
        DesktopError::InvalidRequest(
            "runner declarations must be local and unique per platform".into(),
        )
    })
}

pub(crate) fn local_runner_dispatch_outcome(
    platform: Platform,
    outcome: DispatchOutcome,
) -> LocalRunnerDispatchOutcome {
    let (state, reason) = match outcome {
        DispatchOutcome::Queued { .. } => (
            "runner_accepted",
            "the local runner accepted the request; remote platform processing is not confirmed",
        ),
        DispatchOutcome::Unavailable { .. } => (
            "runner_unavailable",
            "the declared local runner is unavailable for this platform",
        ),
        DispatchOutcome::Rejected { .. } => (
            "runner_rejected",
            "the local runner did not accept this request",
        ),
    };
    LocalRunnerDispatchOutcome {
        platform: platform.as_str(),
        state,
        reason: reason.into(),
    }
}

pub(crate) fn local_runner_dispatch_report(
    repository: &SqliteRepository,
    registry: &ProviderRegistry,
    request: &PublishRequest,
) -> Result<LocalRunnerDispatchReport, DesktopError> {
    let report = registry.dispatch_all(request).map_err(|_| {
        DesktopError::InvalidRequest("local runner dispatch request is invalid".into())
    })?;
    repository
        .record_provider_dispatch_history(request, &report, Utc::now())
        .map_err(|_| {
            DesktopError::Storage("local dispatch result could not be persisted".into())
        })?;
    Ok(LocalRunnerDispatchReport {
        outcomes: report
            .outcomes
            .into_iter()
            .map(|(platform, outcome)| local_runner_dispatch_outcome(platform, outcome))
            .collect(),
        remote_publish_confirmed: false,
    })
}
