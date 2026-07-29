use super::support::*;
use crate::scheduler::{
    SCHEDULED_LOCAL_RUNNER_COMPLETE, SCHEDULED_LOCAL_RUNNER_INCOMPLETE,
    SCHEDULED_LOCAL_RUNNER_UNAVAILABLE, run_scheduler_tick_at,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

const MAX_TEST_HTTP_REQUEST_BYTES: usize = 64 * 1024;

fn read_complete_http_request(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "test article runner received an incomplete HTTP request",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
        if request.len() > MAX_TEST_HTTP_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test article runner request exceeded its bounded fixture limit",
            ));
        }
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = std::str::from_utf8(&request[..header_end]).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test article runner received non-UTF-8 HTTP headers",
        )
    })?;
    let content_length = headers
        .lines()
        .find_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test article runner received an invalid content length",
            )
        })?
        .unwrap_or(0);
    let expected = header_end.checked_add(content_length).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test article runner content length overflowed",
        )
    })?;
    if expected > MAX_TEST_HTTP_REQUEST_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test article runner request exceeded its bounded fixture limit",
        ));
    }
    while request.len() < expected {
        let remaining = expected - request.len();
        let read_len = remaining.min(chunk.len());
        let read = stream.read(&mut chunk[..read_len])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "test article runner received an incomplete HTTP body",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

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
        article_runner: None,
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

#[test]
fn scheduler_processes_article_jobs_within_the_shared_batch_limit() {
    use matrixpost_core::{ArticlePublicationQueue, PublishArticleRequest};

    let state = AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
        article_runner: None,
    };
    let now = chrono::Utc::now();
    let due = LocalSchedule::parse("2026-07-29 09:00:00").unwrap();
    let video = state.repository.enqueue(&scheduled_request(), now).unwrap();
    let article = state
        .repository
        .enqueue_article(
            &PublishArticleRequest {
                platform: "juejin".into(),
                account: Default::default(),
                title: "Scheduled article".into(),
                content: Some("body".into()),
                file: None,
                cover: None,
                category: None,
                tags: Vec::new(),
                summary: None,
                scheduled_at: Some(due.clone()),
            },
            now,
        )
        .unwrap();

    run_scheduler_tick_at(&state, &due, now, 1).unwrap();
    assert_eq!(
        state.repository.job(&video.id).unwrap().unwrap().state,
        PublishState::Unavailable
    );
    assert!(state.repository.article_history().unwrap().is_empty());

    run_scheduler_tick_at(&state, &due, now, 1).unwrap();
    assert_eq!(state.repository.article_history().unwrap().len(), 1);
    assert_eq!(
        state.repository.article_history().unwrap()[0].state,
        PublishState::Unavailable
    );
    let history = &state.repository.article_history().unwrap()[0];
    assert_eq!(
        history.detail.as_deref(),
        Some("scheduled local article runner unavailable")
    );
    assert_eq!(history.title, "Scheduled article");
    let serialized = serde_json::to_string(history).unwrap();
    assert!(!serialized.contains("body"));
    assert!(!article.id.is_empty());
}

#[test]
fn scheduler_records_queued_and_rejected_article_runner_outcomes_without_leaking_requests() {
    for (response, expected) in [
        (
            r#"{"outcome":"queued","version":1,"platform":"juejin","job_id":"local-job","automation_attempted":true}"#,
            PublishState::Published,
        ),
        (
            r#"{"outcome":"rejected","version":1,"platform":"juejin","reason":"http://127.0.0.1:39002 private body","automation_attempted":true}"#,
            PublishState::Failed,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = response.to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_complete_http_request(&mut stream).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });
        let state = AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            article_runner: Some(matrixpost_core::ArticleRunner { address }),
        };
        let now = chrono::Utc::now();
        let due = LocalSchedule::parse("2026-07-29 09:00:00").unwrap();
        let job = state
            .repository
            .enqueue_article(
                &matrixpost_core::PublishArticleRequest {
                    platform: "juejin".into(),
                    account: matrixpost_core::AccountSelection {
                        phone: Some("13800138000".into()),
                        partition: Some("persist:private".into()),
                    },
                    title: "Safe title".into(),
                    content: Some("private body http://127.0.0.1:39002/secret".into()),
                    file: Some("/private/article.md".into()),
                    cover: None,
                    category: None,
                    tags: Vec::new(),
                    summary: None,
                    scheduled_at: Some(due.clone()),
                },
                now,
            )
            .unwrap();
        run_scheduler_tick_at(&state, &due, now, 1).unwrap();
        server.join().unwrap();
        assert_eq!(
            state
                .repository
                .claim_due_articles(&due, now, 1)
                .unwrap()
                .len(),
            0
        );
        let history = state.repository.article_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, expected);
        let serialized = serde_json::to_string(&history).unwrap();
        for forbidden in [
            "private body",
            "/private/article.md",
            "13800138000",
            "127.0.0.1:39002",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        assert!(!job.id.is_empty());
    }
}
