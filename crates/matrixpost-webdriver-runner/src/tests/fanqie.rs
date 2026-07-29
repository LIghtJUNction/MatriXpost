use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn completed_replies() -> Vec<Result<Value, String>> {
    vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(true)),
        value(json!("opened")),
        value(json!(true)),
        value(json!("clicked")),
        value(json!(true)),
        value(json!("ready")),
        value(json!("clicked")),
        value(json!("success")),
        value(json!(null)),
    ]
}

#[test]
fn fanqie_specialty_path_publishes_in_upstream_order_and_returns_local_job() {
    let publisher = test_publisher(MockWebDriver::new(completed_replies()));
    assert_eq!(
        publisher
            .publish(Platform::FanqieVideo, &request())
            .unwrap(),
        "webdriver-fqsp-1"
    );
    let bodies = publisher.transport.bodies.lock().unwrap();
    let upload_ready = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(FANQIE_UPLOAD_READY_SCRIPT.into()))
        })
        .unwrap();
    let one_click = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(FANQIE_ONE_CLICK_PUBLISH_SCRIPT.into()))
        })
        .unwrap();
    let result = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(FANQIE_PUBLISH_RESULT_SCRIPT.into()))
        })
        .unwrap();
    assert!(upload_ready < one_click && one_click < result);
    assert!(!bodies.iter().any(|body| {
        matches!(
            body.get("value").and_then(Value::as_str),
            Some("button[type='submit']") | Some("button[data-action='draft']")
        )
    }));
}

#[test]
fn fanqie_unready_upload_fails_closed_with_session_cleanup() {
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::FanqieVideo, &request())
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn fanqie_missing_or_ambiguous_channel_panel_fails_closed() {
    for state in ["missing", "ambiguous"] {
        let publisher = test_publisher(MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            value(json!(true)),
            value(json!(state)),
            value(json!(null)),
        ]));
        assert!(
            publisher
                .publish(Platform::FanqieVideo, &request())
                .is_err()
        );
        let paths = publisher.transport.paths.lock().unwrap();
        assert!(paths.last().unwrap().ends_with("/session/s"));
        assert!(!paths.iter().any(|path| path.ends_with("/click")));
    }
}

#[test]
fn fanqie_disabled_channel_switch_fails_closed() {
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(true)),
        value(json!("open")),
        value(json!(true)),
        value(json!("disabled")),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::FanqieVideo, &request())
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn fanqie_channel_mismatch_or_result_timeout_fails_closed_after_local_action() {
    let mut mismatch = completed_replies();
    mismatch.truncate(13);
    mismatch[12] = value(json!(false));
    mismatch.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(mismatch));
    assert!(
        publisher
            .publish(Platform::FanqieVideo, &request())
            .is_err()
    );
    assert!(
        !publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.ends_with("/click"))
    );

    let mut timeout = completed_replies();
    timeout.truncate(15);
    timeout.extend((0..FANQIE_PUBLISH_POLL_ATTEMPTS).map(|_| value(json!("pending"))));
    timeout.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(timeout));
    assert!(
        publisher
            .publish(Platform::FanqieVideo, &request())
            .is_err()
    );
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(FANQIE_ONE_CLICK_PUBLISH_SCRIPT.into()))
    }));
    assert!(!bodies.iter().any(|body| {
        matches!(
            body.get("value").and_then(Value::as_str),
            Some("button[type='submit']") | Some("button[data-action='draft']")
        )
    }));
}

#[test]
fn fanqie_delayed_enabled_one_click_action_is_polled_then_clicked_once() {
    let mut replies = completed_replies();
    replies.insert(13, value(json!("pending")));
    replies.insert(14, value(json!("disabled")));
    let publisher = test_publisher(MockWebDriver::new(replies));
    publisher
        .publish(Platform::FanqieVideo, &request())
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert_eq!(
        bodies
            .iter()
            .filter(|body| {
                body.get("script")
                    == Some(&Value::String(FANQIE_ONE_CLICK_PUBLISH_READY_SCRIPT.into()))
            })
            .count(),
        3
    );
    assert_eq!(
        bodies
            .iter()
            .filter(|body| {
                body.get("script") == Some(&Value::String(FANQIE_ONE_CLICK_PUBLISH_SCRIPT.into()))
            })
            .count(),
        1
    );
}

#[test]
fn fanqie_draft_is_rejected_before_session_creation() {
    let mut draft = request();
    draft.draft = true;
    let publisher = test_publisher(MockWebDriver::new(Vec::new()));
    assert!(publisher.publish(Platform::FanqieVideo, &draft).is_err());
    assert!(publisher.transport.paths.lock().unwrap().is_empty());
}

#[test]
fn fanqie_invalid_result_is_rejected_without_reflecting_page_data() {
    let page_data = "https://attached.invalid/private?title=secret";
    let mut replies = completed_replies();
    replies[15] = value(json!(page_data));
    let publisher = test_publisher(MockWebDriver::new(replies));
    let error = publisher
        .publish(Platform::FanqieVideo, &request())
        .unwrap_err();
    assert_eq!(error, "Fanqie publish action returned an invalid state");
    assert!(
        !publisher
            .transport
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| body.to_string().contains(page_data))
    );
}
