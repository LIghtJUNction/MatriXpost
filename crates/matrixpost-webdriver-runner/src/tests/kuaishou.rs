use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Kuaishou,
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
fn kuaishou_target_override_resolves_only_upstream_supported_values() {
    for (value, expected) in [
        ("ai_generated", Some("内容为AI生成")),
        ("AI生成", Some("内容为AI生成")),
        ("含AI生成内容", Some("内容为AI生成")),
        ("内容由AI生成", Some("内容为AI生成")),
        ("fiction", Some("演绎情节，仅供娱乐")),
        ("虚构演绎", Some("演绎情节，仅供娱乐")),
        ("虚构演绎，故事经历", Some("演绎情节，仅供娱乐")),
        ("personal_opinion", Some("个人观点，仅供参考")),
        ("内容为个人观点或见解", Some("个人观点，仅供参考")),
        ("repost", Some("素材来源于网络")),
        ("内容为转载", Some("素材来源于网络")),
        ("取自站外", Some("素材来源于网络")),
        ("none", None),
        ("无标注", None),
        ("marketing", None),
        ("unknown", None),
    ] {
        assert_eq!(
            kuaishou_creative_statement_label(&request_with_statement(value)),
            expected,
            "{value}"
        );
    }
}

#[test]
fn kuaishou_target_statement_selects_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Kuaishou, &request_with_statement("AI生成"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(KUAISHOU_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["内容为AI生成"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("AI生成"))
    }));
    let applied = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(KUAISHOU_STATEMENT_APPLIED_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(applied < submit);
}

#[test]
fn kuaishou_none_unknown_and_missing_target_override_skip_statement_actions() {
    for request in [
        request(),
        request_with_statement("none"),
        request_with_statement("not-supported"),
    ] {
        let publisher = test_publisher(MockWebDriver::new(completed_default_replies()));
        publisher.publish(Platform::Kuaishou, &request).unwrap();
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
                        Some(KUAISHOU_STATEMENT_OPEN_SCRIPT)
                            | Some(KUAISHOU_STATEMENT_LIST_VISIBLE_SCRIPT)
                            | Some(KUAISHOU_STATEMENT_SELECT_SCRIPT)
                            | Some(KUAISHOU_STATEMENT_APPLIED_SCRIPT)
                    )
                })
        );
    }
}

#[test]
fn kuaishou_missing_or_ambiguous_statement_selector_fails_closed_before_submit() {
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
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Kuaishou, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn kuaishou_disabled_statement_option_fails_closed_before_submit() {
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
        value(json!(true)),
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Kuaishou, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn kuaishou_unverified_statement_application_fails_closed_before_submit() {
    let mut replies = vec![
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
    ];
    replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(Platform::Kuaishou, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}
