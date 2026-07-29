use super::support::*;
use crate::scheduler::{
    SCHEDULED_LOCAL_RUNNER_COMPLETE, SCHEDULED_LOCAL_RUNNER_INCOMPLETE,
    SCHEDULED_LOCAL_RUNNER_UNAVAILABLE, run_scheduler_tick_at,
};

#[test]
fn scheduler_tick_publishes_locally_once_and_strips_schedule() {
    let (state, observed) = scheduler_state(
        matrixpost_core::ProviderAvailability::Available,
        DispatchOutcome::Queued {
            job_id: "local-workflow".into(),
        },
    );
    let now = chrono::Utc::now();
    let job = state.repository.enqueue(&scheduled_request(), now).unwrap();
    let due = LocalSchedule::parse("2026-07-29 09:00:00").unwrap();

    run_scheduler_tick_at(&state, &due, now, 1).unwrap();
    let complete = state.repository.job(&job.id).unwrap().unwrap();
    assert_eq!(complete.state, PublishState::Published);
    assert_eq!(complete.revision, 2);
    let observed = observed.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].scheduled_at, None);
    drop(observed);

    let history = state.repository.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, PublishState::Published);
    let detail = history[0].detail.as_deref().unwrap();
    assert_eq!(detail, SCHEDULED_LOCAL_RUNNER_COMPLETE);
    assert!(!detail.contains("movie.mp4"));
    assert!(!detail.contains("127.0.0.1"));

    run_scheduler_tick_at(&state, &due, now, 1).unwrap();
    assert_eq!(state.repository.history().unwrap().len(), 1);
}

#[test]
fn scheduler_requeues_only_the_failed_claim_and_completes_later_jobs() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut providers = ProviderRegistry::new();
    providers
        .register(Box::new(SchedulerProvider {
            platform: Platform::Douyin,
            availability: matrixpost_core::ProviderAvailability::Available,
            outcome: DispatchOutcome::Queued {
                job_id: "local-workflow".into(),
            },
            observed: Arc::clone(&observed),
        }))
        .unwrap();
    let state = AppState {
        repository: Arc::new(FailFirstCompletionRepository::new()),
        providers: Arc::new(providers),
    };
    let now = chrono::Utc::now();
    let due = LocalSchedule::parse("2026-07-29 09:00:00").unwrap();
    let first = state.repository.enqueue(&scheduled_request(), now).unwrap();
    let second = state.repository.enqueue(&scheduled_request(), now).unwrap();

    run_scheduler_tick_at(&state, &due, now, 2).unwrap();
    let first_after_failure = state.repository.job(&first.id).unwrap().unwrap();
    assert_eq!(first_after_failure.state, PublishState::Queued);
    assert_eq!(first_after_failure.revision, 2);
    assert_eq!(
        state.repository.job(&second.id).unwrap().unwrap().state,
        PublishState::Published
    );
    assert_eq!(state.repository.history().unwrap().len(), 1);

    run_scheduler_tick_at(&state, &due, now, 2).unwrap();
    assert_eq!(
        state.repository.job(&first.id).unwrap().unwrap().state,
        PublishState::Published
    );
    assert_eq!(state.repository.history().unwrap().len(), 2);
    // The runner received the first request twice: recovery occurs after
    // a possible local acceptance, so semantics are intentionally
    // at-least-once rather than a false exactly-once claim.
    assert_eq!(observed.lock().unwrap().len(), 3);
}

#[test]
fn scheduler_tick_marks_unavailable_and_rejected_outcomes_safely() {
    let now = chrono::Utc::now();
    let due = LocalSchedule::parse("2026-07-29 09:00:00").unwrap();
    let (unavailable_state, unavailable_observed) = scheduler_state(
        matrixpost_core::ProviderAvailability::Unavailable {
            reason: "not logged in".into(),
        },
        DispatchOutcome::Queued {
            job_id: "must-not-run".into(),
        },
    );
    let unavailable_job = unavailable_state
        .repository
        .enqueue(&scheduled_request(), now)
        .unwrap();
    run_scheduler_tick_at(&unavailable_state, &due, now, 1).unwrap();
    assert_eq!(
        unavailable_state
            .repository
            .job(&unavailable_job.id)
            .unwrap()
            .unwrap()
            .state,
        PublishState::Unavailable
    );
    assert!(unavailable_observed.lock().unwrap().is_empty());
    assert_eq!(
        unavailable_state.repository.history().unwrap()[0].detail,
        Some(SCHEDULED_LOCAL_RUNNER_UNAVAILABLE.into())
    );

    let (rejected_state, _) = scheduler_state(
        matrixpost_core::ProviderAvailability::Available,
        DispatchOutcome::Rejected {
            reason: "tcp:127.0.0.1:39001 rejected movie.mp4".into(),
        },
    );
    let rejected_job = rejected_state
        .repository
        .enqueue(&scheduled_request(), now)
        .unwrap();
    run_scheduler_tick_at(&rejected_state, &due, now, 1).unwrap();
    assert_eq!(
        rejected_state
            .repository
            .job(&rejected_job.id)
            .unwrap()
            .unwrap()
            .state,
        PublishState::Failed
    );
    let detail = rejected_state.repository.history().unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert_eq!(detail, SCHEDULED_LOCAL_RUNNER_INCOMPLETE);
    assert!(!detail.contains("127.0.0.1"));
    assert!(!detail.contains("movie.mp4"));
}
