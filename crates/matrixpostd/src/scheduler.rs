use std::time::Duration;

use matrixpost_core::{
    DispatchOutcome, DomainError, LocalSchedule, PublicationQueue, PublishState, Repository,
};

use crate::state::AppState;

pub(crate) const SCHEDULED_LOCAL_RUNNER_COMPLETE: &str =
    "scheduled local runner workflow completed; remote platform processing is not confirmed";
pub(crate) const SCHEDULED_LOCAL_RUNNER_UNAVAILABLE: &str =
    "scheduled local runner was unavailable; no remote platform publication was attempted";
pub(crate) const SCHEDULED_LOCAL_RUNNER_INCOMPLETE: &str =
    "scheduled local runner workflow was incomplete; remote platform processing is not confirmed";

/// Runs one deterministic durable-scheduler pass.
///
/// The queue claim happens before any runner is contacted. Each claimed job is
/// therefore owned by this pass and retains its optimistic revision for the
/// terminal state transition. Runner requests deliberately clear the original
/// local schedule: the daemon, rather than a provider, owns due-time timing.
/// No branch here opens a browser or calls a platform directly; dispatch is
/// limited to the already configured local provider registry. A claimed task
/// is an at-least-once local workflow: if terminal persistence fails, its
/// exact non-terminal claim is requeued for retry, and no path claims remote
/// platform success.
pub(crate) fn run_scheduler_tick_at<R>(
    state: &AppState<R>,
    due_through: &LocalSchedule,
    updated_at: chrono::DateTime<chrono::Utc>,
    batch_size: usize,
) -> Result<(), DomainError>
where
    R: Repository + PublicationQueue,
{
    let claimed = state
        .repository
        .claim_due(due_through, updated_at, batch_size)?;
    for job in claimed {
        let mut request = job.request.clone();
        request.scheduled_at = None;
        let (terminal_state, detail) = match state.providers.dispatch_all(&request) {
            Ok(report)
                if report
                    .outcomes
                    .values()
                    .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. })) =>
            {
                (PublishState::Published, SCHEDULED_LOCAL_RUNNER_COMPLETE)
            }
            Ok(report)
                if report
                    .outcomes
                    .values()
                    .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. })) =>
            {
                (
                    PublishState::Unavailable,
                    SCHEDULED_LOCAL_RUNNER_UNAVAILABLE,
                )
            }
            Ok(_) | Err(_) => (PublishState::Failed, SCHEDULED_LOCAL_RUNNER_INCOMPLETE),
        };
        if state
            .repository
            .complete_job_with_history(
                &job.id,
                job.revision,
                terminal_state,
                updated_at,
                Some(detail),
            )
            .is_err()
        {
            // The runner may already have accepted this request. Requeue only
            // the exact non-terminal claim instead of inventing a terminal
            // record, so retry semantics remain explicitly at-least-once.
            if state
                .repository
                .requeue_claim(&job.id, job.revision, updated_at)
                .is_err()
            {
                eprintln!("matrixpostd scheduler could not recover a claimed job");
            }
        }
    }
    Ok(())
}

fn current_local_schedule() -> Result<LocalSchedule, DomainError> {
    LocalSchedule::parse(
        &chrono::Local::now()
            .naive_local()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}

/// Runs the periodic scheduler independently from request handling.
pub(crate) async fn scheduler_loop(state: AppState, interval: Duration, batch_size: usize) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let tick_state = state.clone();
        let result = tokio::task::spawn_blocking(move || {
            let due_through = current_local_schedule()?;
            run_scheduler_tick_at(&tick_state, &due_through, chrono::Utc::now(), batch_size)
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            // Errors can contain storage/runner text. Keep the daemon log
            // intentionally non-sensitive and let the next pass retry only
            // jobs that remain queued.
            eprintln!("matrixpostd scheduler tick failed");
        }
    }
}
