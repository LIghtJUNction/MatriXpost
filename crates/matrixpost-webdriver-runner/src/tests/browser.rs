use super::{
    protocol::{Accepted, AcceptedLogin, FailingLogin},
    support::*,
};
use crate::{config::*, profiles::*, service::*, webdriver::*};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use matrixpost_core::*;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn manual_login_protocol_rejects_invalid_versions_and_unknown_fields() {
    let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
        AcceptedLogin,
    )))));
    let invalid_version = json!({
        "version": LOGIN_RUNNER_PROTOCOL_VERSION + 1,
        "platform": "dy"
    });
    let response = router
        .clone()
        .oneshot(
            Request::post("/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(invalid_version.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let unknown = json!({
        "version": LOGIN_RUNNER_PROTOCOL_VERSION,
        "platform": "dy",
        "cookie": "forbidden"
    });
    let response = router
        .oneshot(
            Request::post("/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(unknown.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn manual_login_protocol_is_unavailable_without_explicit_executor() {
    let router = app(Arc::new(runner_service_with_login(None)));
    let response = router
        .oneshot(
            Request::post("/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRunnerRequest {
                        version: LOGIN_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(matches!(
        serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap(),
        LoginRunnerResponse::Unavailable { .. }
    ));
}

#[tokio::test]
async fn manual_login_protocol_opens_only_the_manual_login_page() {
    let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
        AcceptedLogin,
    )))));
    let response = router
        .oneshot(
            Request::post("/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRunnerRequest {
                        version: LOGIN_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap(),
        LoginRunnerResponse::Opened {
            version: LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            manual_login_required: true,
        }
    );
}

#[tokio::test]
async fn manual_login_protocol_rejects_executor_failures_without_exposing_them() {
    let router = app(Arc::new(runner_service_with_login(Some(Arc::new(
        FailingLogin,
    )))));
    let response = router
        .oneshot(
            Request::post("/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&LoginRunnerRequest {
                        version: LOGIN_RUNNER_PROTOCOL_VERSION,
                        platform: Platform::Douyin,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response = serde_json::from_slice::<LoginRunnerResponse>(&body).unwrap();
    assert!(matches!(response, LoginRunnerResponse::Rejected { .. }));
    assert!(
        !serde_json::to_string(&response)
            .unwrap()
            .contains("raw webdriver failure")
    );
}

#[tokio::test]
async fn health_reports_configured_but_unreachable_browser_as_detached() {
    let status = health_status(runner_service_with_probe(
        Some(Arc::new(Accepted)),
        Some(debugger_address()),
        false,
    ))
    .await;

    assert_eq!(status["browser_debugger_configured"], true);
    assert_eq!(status["attached_browser"], false);
}

#[tokio::test]
async fn health_reports_ready_configured_browser_as_attached() {
    let status = health_status(runner_service_with_probe(
        Some(Arc::new(Accepted)),
        Some(debugger_address()),
        true,
    ))
    .await;

    assert_eq!(status["browser_debugger_configured"], true);
    assert_eq!(status["attached_browser"], true);
}

#[tokio::test]
async fn health_without_browser_debugger_address_is_detached() {
    let status = health_status(runner_service_with_probe(
        Some(Arc::new(Accepted)),
        None,
        true,
    ))
    .await;

    assert_eq!(status["browser_debugger_configured"], false);
    assert_eq!(status["attached_browser"], false);
}

#[test]
fn devtools_version_probe_requires_chrome_protocol_evidence() {
    assert!(valid_chrome_devtools_version(&json!({
        "Browser": "Chrome/150.0.0.0",
        "Protocol-Version": "1.3",
        "webSocketDebuggerUrl": "ws://127.0.0.1:9222/devtools/browser/test"
    })));
    assert!(!valid_chrome_devtools_version(&json!({
        "Browser": "Chrome/150.0.0.0",
        "Protocol-Version": "1.3"
    })));
}

#[test]
fn profiles_cover_the_exact_upstream_platform_set_with_ordered_fallbacks() {
    assert_eq!(PROFILES.len(), Platform::ALL.len());
    assert_eq!(PROFILE_FIXTURES.len(), Platform::ALL.len());
    for platform in Platform::ALL {
        let profile = profile(platform).unwrap();
        assert!(profile.upload_url.starts_with("https://"));
        assert!(
            profile.file.len() >= 2
                && profile.title.len() >= 2
                && profile.description.len() >= 2
                && profile.submit.len() >= 2
                && profile.draft.len() >= 2
                && profile.success.len() >= 2
        );
        let fixture = PROFILE_FIXTURES
            .iter()
            .find(|fixture| fixture.platform == platform)
            .unwrap();
        assert_eq!(profile.upload_url, fixture.upload_url);
        assert!(profile.success.contains(&fixture.success_selector));
    }
}
#[test]
fn webdriver_protocol_runs_each_phase_and_closes_the_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!("ready")),
        value(json!("ready")),
        value(json!("clicked")),
        Err("not visible before action".into()),
        Err("not visible before action".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert_eq!(
        publisher.publish(Platform::Douyin, &request()).unwrap(),
        "webdriver-dy-1"
    );
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/s")
    );
    assert_eq!(
        publisher.transport.bodies.lock().unwrap()[0],
        json!({"capabilities":{"alwaysMatch":{"goog:chromeOptions":{"debuggerAddress":"127.0.0.1:9222"}}}})
    );
}
#[test]
fn missing_selector_fails_closed_and_still_closes_the_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        Err("not found".into()),
        Err("not found".into()),
        Err("not found".into()),
        Err("not found".into()),
        Err("not found".into()),
        Err("not found".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert!(publisher.publish(Platform::Douyin, &request()).is_err());
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/s")
    );
}

#[test]
fn webdriver_endpoint_rejects_remote_credentials_and_profile_paths() {
    for value in [
        "https://127.0.0.1:9515",
        "http://192.0.2.1:9515",
        "http://user:pass@127.0.0.1:9515",
        "http://127.0.0.1:9515/profile",
    ] {
        assert!(local_webdriver_endpoint(value).is_err(), "{value}");
    }
    assert!(local_webdriver_endpoint("http://127.0.0.1:9515/wd/hub").is_ok());
    assert!(local_webdriver_endpoint("http://[::1]:9515/wd/hub").is_ok());
}
#[test]
fn success_timeout_is_rejected_and_session_is_closed() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        Err("not visible before action".into()),
        Err("not visible before action".into()),
        element("submit"),
        value(json!(null)),
        Err("not ready".into()),
        Err("not ready".into()),
        Err("not ready".into()),
        Err("not ready".into()),
        Err("not ready".into()),
        Err("not ready".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert!(publisher.publish(Platform::Douyin, &request()).is_err());
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/s")
    );
}
#[test]
fn hidden_success_marker_never_acknowledges_and_cleanup_runs() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        element("pre-hidden"),
        value(json!(false)),
        Err("not found".into()),
        element("submit"),
        value(json!(null)),
        element("post-hidden"),
        value(json!(false)),
        Err("not found".into()),
        element("post-hidden"),
        value(json!(false)),
        Err("not found".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert!(publisher.publish(Platform::Douyin, &request()).is_err());
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/s")
    );
}
#[test]
fn preexisting_visible_success_marker_rejects_before_click_and_cleans_up() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        element("already-successful"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert!(publisher.publish(Platform::Douyin, &request()).is_err());
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
    assert!(paths.last().unwrap().ends_with("/session/s"));
}
#[test]
fn debugger_address_must_be_loopback() {
    let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
    assert!(
        build_executor(Some(endpoint.clone()), Some(debugger_address()))
            .unwrap()
            .is_some()
    );
    assert!(build_executor(Some(endpoint), Some("192.0.2.1:9222".parse().unwrap())).is_err());
}
#[test]
fn article_executor_requires_explicit_startup_opt_in() {
    let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
    assert!(
        build_article_executor(Some(endpoint.clone()), Some(debugger_address()), false)
            .unwrap()
            .is_none()
    );
    assert!(
        build_article_executor(Some(endpoint), Some(debugger_address()), true)
            .unwrap()
            .is_some()
    );
}
#[test]
fn login_executor_requires_explicit_startup_opt_in() {
    let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
    assert!(
        build_login_executor(Some(endpoint.clone()), Some(debugger_address()), false)
            .unwrap()
            .is_none()
    );
    assert!(
        build_login_executor(Some(endpoint), Some(debugger_address()), true)
            .unwrap()
            .is_some()
    );
}
#[test]
fn account_status_probe_requires_explicit_startup_opt_in() {
    let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
    assert!(
        build_account_status_executor(Some(endpoint.clone()), Some(debugger_address()), false)
            .unwrap()
            .is_none()
    );
    assert!(
        build_account_status_executor(Some(endpoint), Some(debugger_address()), true)
            .unwrap()
            .is_some()
    );
}

#[test]
fn review_status_probe_requires_explicit_startup_opt_in() {
    let endpoint = local_webdriver_endpoint("http://127.0.0.1:9515").unwrap();
    assert!(
        build_review_status_executor(Some(endpoint.clone()), Some(debugger_address()), false)
            .unwrap()
            .is_none()
    );
    assert!(
        build_review_status_executor(Some(endpoint), Some(debugger_address()), true)
            .unwrap()
            .is_some()
    );
}
#[test]
fn manual_login_navigation_closes_the_temporary_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"login-session"})),
        value(json!(null)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    publisher.open_manual_login(Platform::Douyin).unwrap();
    let paths = publisher.transport.paths.lock().unwrap();
    assert_eq!(paths[1], "/session/login-session/url");
    assert!(paths.last().unwrap().ends_with("/session/login-session"));
}
#[test]
fn manual_login_navigation_failure_still_closes_the_temporary_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"login-session"})),
        Err("navigation failed".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    assert!(publisher.open_manual_login(Platform::Douyin).is_err());
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .is_some_and(|path| path.ends_with("/session/login-session"))
    );
}

#[test]
fn account_readiness_probe_closes_sessions_for_ready_not_ready_and_failure() {
    let ready = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"ready"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
    ]));
    assert!(ready.account_readiness(Platform::Douyin).unwrap());
    assert!(
        ready
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/ready")
    );

    let not_ready = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"not-ready"})),
        value(json!(null)),
        Err("missing".into()),
        Err("missing".into()),
        value(json!(null)),
    ]));
    assert!(!not_ready.account_readiness(Platform::Douyin).unwrap());
    assert!(
        not_ready
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/not-ready")
    );

    let failure = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"failure"})),
        Err("navigation failed".into()),
        value(json!(null)),
    ]));
    assert!(failure.account_readiness(Platform::Douyin).is_err());
    assert!(
        failure
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/failure")
    );
}

#[test]
fn fanqie_review_status_uses_only_fixed_classifier_and_closes_session() {
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"review"})),
        value(json!(null)),
        value(json!("under_review")),
        value(json!(null)),
    ]));
    assert_eq!(
        publisher.review_status("  作品 标题  ").unwrap(),
        ReviewStatus::UnderReview
    );
    let bodies = publisher.transport.bodies.lock().unwrap();
    let execute = bodies
        .iter()
        .find(|body| body.get("script").is_some())
        .unwrap();
    assert_eq!(execute["args"][0], "作品标题");
    assert_eq!(execute["script"], FANQIE_REVIEW_STATUS_SCRIPT);
    assert!(
        publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .ends_with("/session/review")
    );
}

#[test]
fn fanqie_review_status_classifier_is_plain_executable_javascript() {
    assert!(!FANQIE_REVIEW_STATUS_SCRIPT.contains("pub(crate)"));
    for construct in [
        "const n=",
        "const q=",
        "for(const card of document.querySelectorAll",
        "window.scrollBy",
        "return null;",
    ] {
        assert!(
            FANQIE_REVIEW_STATUS_SCRIPT.contains(construct),
            "{construct}"
        );
    }
}

#[test]
fn terminal_qr_capture_uses_only_element_screenshot_and_closes_on_request() {
    let publisher = Arc::new(test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"terminal-qr"})),
        value(json!(null)),
        value(json!([{ELEMENT_KEY:"qr"}])),
        value(json!("iVBORw0KGgo=")),
        value(json!(null)),
    ])));
    let mut attempt = Arc::clone(&publisher)
        .start_terminal_qr_login(Platform::Douyin)
        .unwrap();
    assert_eq!(attempt.platform(), Platform::Douyin);
    attempt.close().unwrap();
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("/element/qr/screenshot"))
    );
    assert!(!paths.iter().any(|path| path.contains("/cookie")));
    assert!(
        !paths
            .iter()
            .any(|path| path == "/session/terminal-qr/screenshot")
    );
    assert!(paths.last().unwrap().ends_with("/session/terminal-qr"));
    assert_eq!(publisher.transport.methods.lock().unwrap()[3], "GET");
}
