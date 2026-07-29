use std::process::ExitCode;

use matrixpost_core::{ArticleDispatchOutcome, Platform, ProviderDispatchReport};
use serde::Serialize;

#[derive(Serialize)]
struct Output<'a, T: Serialize> {
    ok: bool,
    code: u8,
    result: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

pub(crate) fn emit<T: Serialize>(code: u8, result: T, message: Option<&str>) -> ExitCode {
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
pub(crate) fn emit_dispatch(report: ProviderDispatchReport) -> ExitCode {
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

pub(crate) fn emit_article_dispatch_outcome(outcome: ArticleDispatchOutcome) -> ExitCode {
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
