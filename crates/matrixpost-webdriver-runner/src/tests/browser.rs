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
use serde_json::{Value, json};
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
fn wechat_publish_without_product_link_keeps_the_standard_protocol() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
    publisher
        .publish(Platform::WechatChannels, &request)
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(!bodies.iter().any(|body| {
        body.get("script")
            .and_then(Value::as_str)
            .is_some_and(|script| script.contains("wujie-app"))
    }));
}

#[test]
fn wechat_link_type_none_disables_product_attachment_before_webdriver() {
    let publisher = test_publisher(MockWebDriver::new(Vec::new()));
    let mut request = request();
    request.wechat_link.link_type = Some("NoNe".into());
    request.wechat_link.link_value = Some("ignored-by-disabled-link".into());
    assert_eq!(
        WebDriverPublisher::<MockWebDriver>::wechat_product_id(&request).unwrap(),
        None
    );
    assert!(publisher.transport.paths.lock().unwrap().is_empty());
}

#[test]
fn wechat_product_link_runs_shadow_root_protocol_and_closes_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
    request.wechat_link.product_id = Some("product-1".into());
    request.wechat_link.link_type = Some("none".into());
    assert_eq!(
        publisher
            .publish(Platform::WechatChannels, &request)
            .unwrap(),
        "webdriver-sph-1"
    );
    let bodies = publisher.transport.bodies.lock().unwrap();
    let scripts = bodies
        .iter()
        .filter_map(|body| body.get("script").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let product_scripts = scripts
        .iter()
        .filter(|script| script.contains("wujie-app"))
        .collect::<Vec<_>>();
    assert_eq!(product_scripts.len(), 9);
    assert!(
        product_scripts
            .iter()
            .all(|script| script.contains("shadowRoot"))
    );
    for script in [
        WECHAT_PRODUCT_SEARCH_SCRIPT,
        WECHAT_PRODUCT_EXACT_ROW_SCRIPT,
        WECHAT_PRODUCT_SELECT_EXACT_SCRIPT,
    ] {
        assert!(bodies.iter().any(|body| {
            body.get("script") == Some(&Value::String(script.into()))
                && body.get("args") == Some(&json!(["product-1"]))
        }));
    }
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
fn non_wechat_product_metadata_never_runs_wechat_shadow_scripts() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.wechat_link.product_id = Some("product-1".into());
    publisher.publish(Platform::Douyin, &request).unwrap();
    assert!(
        !publisher
            .transport
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| {
                body.get("script")
                    .and_then(Value::as_str)
                    .is_some_and(|script| script.contains("wujie-app"))
            })
    );
}

#[test]
fn wechat_product_deadline_is_fixed_and_finite() {
    assert_eq!(WECHAT_PRODUCT_POLL_ATTEMPTS, 30);
    assert_eq!(
        WECHAT_PRODUCT_POLL_INTERVAL,
        std::time::Duration::from_millis(200)
    );
}

#[test]
fn malformed_wechat_product_link_fails_before_creating_session() {
    for link in [
        matrixpost_core::WechatLink {
            link_type: Some("url".into()),
            link_value: Some("https://example.invalid".into()),
            ..Default::default()
        },
        matrixpost_core::WechatLink {
            link_type: Some("product".into()),
            link_value: Some("   ".into()),
            ..Default::default()
        },
    ] {
        let publisher = test_publisher(MockWebDriver::new(Vec::new()));
        let mut request = request();
        request.wechat_link = link;
        assert!(
            publisher
                .publish(Platform::WechatChannels, &request)
                .is_err()
        );
        assert!(publisher.transport.paths.lock().unwrap().is_empty());
    }
}

#[test]
fn wechat_product_failure_still_closes_the_temporary_session() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(true)),
        value(json!(false)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.wechat_link.product_id = Some("product-1".into());
    assert!(
        publisher
            .publish(Platform::WechatChannels, &request)
            .is_err()
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
