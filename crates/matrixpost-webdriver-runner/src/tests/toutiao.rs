use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Toutiao,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some(value.into()),
        account: None,
        wechat_link: None,
    });
    request
}

fn completed_statement_replies() -> Vec<Result<Value, String>> {
    vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
        value(json!("clicked")),
        value(json!(true)),
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]
}

fn completed_default_replies() -> Vec<Result<Value, String>> {
    vec![
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
    ]
}

#[test]
fn toutiao_target_override_resolves_only_upstream_supported_values() {
    for (value, expected) in [
        ("ai_generated", Some("AI生成")),
        ("内容为AI生成", Some("AI生成")),
        ("fiction", Some("虚构演绎，故事经历")),
        ("虚构演绎，仅供娱乐", Some("虚构演绎，故事经历")),
        ("repost", Some("取自站外")),
        ("素材来源于网络", Some("取自站外")),
        ("none", None),
        ("marketing", None),
        ("personal_opinion", None),
        ("unknown", None),
    ] {
        assert_eq!(
            toutiao_creative_statement_label(&request_with_statement(value)),
            expected,
            "{value}"
        );
    }
}

#[test]
fn toutiao_target_statement_selects_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Toutiao, &request_with_statement("ai_generated"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(TOUTIAO_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["AI生成"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("ai_generated"))
    }));
    let checked = bodies
        .iter()
        .rposition(|body| {
            body.get("script") == Some(&Value::String(TOUTIAO_STATEMENT_SELECTED_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(checked < submit);
}

#[test]
fn toutiao_selected_statement_is_idempotent() {
    let mut replies = completed_statement_replies();
    replies[8] = value(json!(true));
    let _ = replies.remove(9);
    let _ = replies.remove(9);
    let publisher = test_publisher(MockWebDriver::new(replies));
    publisher
        .publish(Platform::Toutiao, &request_with_statement("fiction"))
        .unwrap();
    assert!(
        !publisher
            .transport
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| {
                body.get("script") == Some(&Value::String(TOUTIAO_STATEMENT_SELECT_SCRIPT.into()))
            })
    );
}

#[test]
fn toutiao_missing_none_unknown_or_other_platform_override_skips_statement_actions() {
    let mut other_platform = request();
    other_platform.overrides.push(PlatformOverride {
        platform: Platform::Douyin,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some("ai_generated".into()),
        account: None,
        wechat_link: None,
    });
    for request in [
        request(),
        request_with_statement("none"),
        request_with_statement("marketing"),
        other_platform,
    ] {
        let publisher = test_publisher(MockWebDriver::new(completed_default_replies()));
        publisher.publish(Platform::Toutiao, &request).unwrap();
        assert!(
            !publisher
                .transport
                .bodies
                .lock()
                .unwrap()
                .iter()
                .any(|body| {
                    matches!(
                        body.get("script").and_then(Value::as_str),
                        Some(TOUTIAO_STATEMENT_SELECTED_SCRIPT)
                            | Some(TOUTIAO_STATEMENT_SELECT_SCRIPT)
                    )
                })
        );
    }
}

fn assert_fails_closed(replies: Vec<Result<Value, String>>) {
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(Platform::Toutiao, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

fn assert_statement_state_fails_closed(state: &str) {
    assert_fails_closed(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
        value(json!(state)),
        value(json!(null)),
    ]);
}

#[test]
fn toutiao_missing_statement_fails_closed_before_submit() {
    assert_statement_state_fails_closed("missing");
}

#[test]
fn toutiao_ambiguous_statement_fails_closed_before_submit() {
    assert_statement_state_fails_closed("ambiguous");
}

#[test]
fn toutiao_disabled_statement_fails_closed_before_submit() {
    assert_statement_state_fails_closed("disabled");
}

#[test]
fn toutiao_unverified_statement_application_fails_closed_before_submit() {
    let mut replies = vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
        value(json!("clicked")),
    ];
    replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.push(value(json!(null)));
    assert_fails_closed(replies);
}
