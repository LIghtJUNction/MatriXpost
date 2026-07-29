use super::{
    protocol::{
        Accepted, CountingArticleExecutor, FailingArticleExecutor, LocalValidationArticleExecutor,
    },
    support::*,
};
use crate::{profiles::*, service::*, webdriver::*};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use matrixpost_core::*;
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tower::ServiceExt;

#[test]
fn article_executor_writes_codemirror_verifies_optional_summary_and_closes_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"article-session"})),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("editor"),
        value(json!(true)),
        element("summary"),
        value(json!(null)),
        Err("not present".into()),
        Err("not present".into()),
        element("publish-panel"),
        value(json!(null)),
        element("confirm"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut article = article_request();
    article.summary = Some("A concise summary".into());
    assert_eq!(
        publisher.publish_article(&article).unwrap(),
        "webdriver-juejin-1"
    );
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(CODEMIRROR_WRITE_SCRIPT.into()))
            && body
                .get("args")
                .and_then(Value::as_array)
                .is_some_and(|args| args.get(1) == Some(&Value::String("# Article body".into())))
    }));
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .is_some_and(|path| path.ends_with("/session/article-session"))
    );
}

#[test]
fn article_executor_rejects_unverified_codemirror_write_and_closes_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"article-session"})),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("editor"),
        value(json!(false)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let error = publisher.publish_article(&article_request()).unwrap_err();
    assert!(error.automation_attempted);
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .is_some_and(|path| path.ends_with("/session/article-session"))
    );
}

#[test]
fn article_input_validation_bounds_inline_and_local_files() {
    let mut inline = article_request();
    inline.title = "x".repeat(MAX_ARTICLE_TITLE_BYTES + 1);
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&inline).is_err());
    let mut inline = article_request();
    inline.content = Some("x".repeat(MAX_ARTICLE_BODY_BYTES + 1));
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&inline).is_err());
    let unsupported = temporary_article_path("html");
    fs::write(&unsupported, "body").unwrap();
    let mut file_request = article_request();
    file_request.content = None;
    file_request.file = Some(unsupported.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err());
    fs::remove_file(unsupported).unwrap();
    let empty = temporary_article_path("md");
    fs::write(&empty, "").unwrap();
    file_request.file = Some(empty.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err());
    fs::remove_file(empty).unwrap();
    let oversized = temporary_article_path("txt");
    fs::write(&oversized, vec![b'x'; MAX_ARTICLE_BODY_BYTES + 1]).unwrap();
    file_request.file = Some(oversized.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err());
    fs::remove_file(oversized).unwrap();
    let invalid_utf8 = temporary_article_path("md");
    fs::write(&invalid_utf8, [0xff]).unwrap();
    file_request.file = Some(invalid_utf8.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).is_err());
    fs::remove_file(invalid_utf8).unwrap();
    let valid = temporary_article_path("md");
    fs::write(&valid, "# valid body").unwrap();
    file_request.file = Some(valid.clone());
    assert_eq!(
        WebDriverPublisher::<MockWebDriver>::validate_article_request(&file_request).unwrap(),
        "# valid body"
    );
    fs::remove_file(valid).unwrap();
}

#[test]
fn article_executor_marks_local_validation_failure_as_not_attempted() {
    let publisher = test_publisher(MockWebDriver::new(Vec::new()));
    let mut request = article_request();
    request.title = "x".repeat(MAX_ARTICLE_TITLE_BYTES + 1);
    let error = publisher.publish_article(&request).unwrap_err();
    assert!(!error.automation_attempted);
    assert!(publisher.transport.paths.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn article_input_validation_rejects_symlink_and_non_regular_files() {
    use std::os::unix::fs::symlink;

    let target = temporary_article_path("md");
    let link = temporary_article_path("md");
    fs::write(&target, "body").unwrap();
    symlink(&target, &link).unwrap();
    let mut request = article_request();
    request.content = None;
    request.file = Some(link.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
    fs::remove_file(link).unwrap();
    fs::remove_file(target).unwrap();
    let directory = temporary_article_path("md");
    fs::create_dir(&directory).unwrap();
    request.file = Some(directory.clone());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
    fs::remove_dir(directory).unwrap();
}

#[test]
fn article_input_validation_rejects_nonlocal_or_unbounded_cover() {
    let mut request = article_request();
    request.cover = Some("https://example.invalid/cover.png".into());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
    let unsupported = temporary_article_path("gif");
    fs::write(&unsupported, "cover").unwrap();
    request.cover = Some(unsupported.to_string_lossy().into_owned());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
    fs::remove_file(unsupported).unwrap();
    let oversized = temporary_article_path("png");
    fs::write(&oversized, vec![b'x'; MAX_ARTICLE_COVER_BYTES as usize + 1]).unwrap();
    request.cover = Some(oversized.to_string_lossy().into_owned());
    assert!(WebDriverPublisher::<MockWebDriver>::validate_article_request(&request).is_err());
    fs::remove_file(oversized).unwrap();
}

#[tokio::test]
async fn article_protocol_is_unavailable_without_explicit_opt_in_even_with_video_attach() {
    let router = app(Arc::new(runner_service(Some(Arc::new(Accepted)), None)));
    let request = ArticleRunnerRequest {
        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
        request: article_request(),
    };
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(matches!(
        serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
        ArticleRunnerResponse::Unavailable { .. }
    ));
    let mut invalid = serde_json::to_value(request).unwrap();
    invalid["version"] = json!(99);
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut routed = article_request();
    routed.account.partition = Some("persist:forbidden".into());
    let response = router
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArticleRunnerRequest {
                        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                        request: routed,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn article_protocol_queues_executor_response_and_rejects_unknown_payload_fields() {
    let router = app(Arc::new(runner_service(None, Some(Arc::new(Accepted)))));
    let request = ArticleRunnerRequest {
        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
        request: article_request(),
    };
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
        ArticleRunnerResponse::Queued {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            job_id: "article-job-1".into(),
            automation_attempted: true,
        }
    );
    let mut malformed = serde_json::to_value(request).unwrap();
    malformed["profile"] = json!("forbidden");
    let response = router
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(malformed.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn article_protocol_rejects_scheduled_requests_before_starting_an_executor() {
    let executor = Arc::new(CountingArticleExecutor(AtomicU64::new(0)));
    let router = app(Arc::new(runner_service(None, Some(executor.clone()))));
    let mut request = article_request();
    request.scheduled_at =
        Some(matrixpost_core::LocalSchedule::parse("2026-01-02 03:04:05").unwrap());
    let response = router
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArticleRunnerRequest {
                        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                        request,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(executor.0.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn article_protocol_marks_executor_failure_as_an_attempted_automation() {
    let router = app(Arc::new(runner_service(
        None,
        Some(Arc::new(FailingArticleExecutor)),
    )));
    let response = router
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArticleRunnerRequest {
                        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                        request: article_request(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
        ArticleRunnerResponse::Rejected {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            reason: "mock automation failure".into(),
            automation_attempted: true,
        }
    );
}

#[tokio::test]
async fn article_protocol_marks_pre_session_validation_failure_as_not_attempted() {
    let router = app(Arc::new(runner_service(
        None,
        Some(Arc::new(LocalValidationArticleExecutor)),
    )));
    let response = router
        .oneshot(
            Request::post("/v1/publish-article")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArticleRunnerRequest {
                        version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                        request: article_request(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(matches!(
        serde_json::from_slice::<ArticleRunnerResponse>(&body).unwrap(),
        ArticleRunnerResponse::Rejected {
            automation_attempted: false,
            ..
        }
    ));
}
