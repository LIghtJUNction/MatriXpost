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

fn ordinary_replies(state: &str, action: &str) -> Vec<Result<Value, String>> {
    vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        Err("not visible".into()),
        Err("not visible".into()),
        value(json!(state)),
        value(json!(action)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]
}

fn statement_replies() -> Vec<Result<Value, String>> {
    let mut replies = ordinary_replies("ready", "clicked");
    replies.splice(
        6..6,
        [
            value(json!(true)),
            value(json!(true)),
            value(json!(true)),
            value(json!(true)),
        ],
    );
    replies
}

fn terminal_replies(state: &str) -> Vec<Result<Value, String>> {
    let mut replies = ordinary_replies(state, "unexpected");
    replies.truncate(9);
    replies.push(value(json!(null)));
    replies
}

fn race_replies() -> Vec<Result<Value, String>> {
    let mut replies = ordinary_replies("ready", "disabled");
    replies.truncate(10);
    replies.push(value(json!(null)));
    replies
}

fn assert_closed_without_generic_click(publisher: WebDriverPublisher<MockWebDriver>) {
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

fn statement_failure_replies(
    actions: impl IntoIterator<Item = Result<Value, String>>,
) -> Vec<Result<Value, String>> {
    let mut replies = ordinary_replies("unexpected", "unexpected");
    replies.truncate(6);
    replies.extend(actions);
    replies.push(value(json!(null)));
    replies
}

fn assert_statement_failure(replies: Vec<Result<Value, String>>) {
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(Platform::Kuaishou, &request_with_statement("ai_generated"))
            .is_err()
    );
    assert_closed_without_generic_click(publisher);
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
fn kuaishou_writes_the_effective_title_and_tags_once_to_the_upstream_editor() {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Kuaishou,
        title: Some("Effective title".into()),
        short_title: None,
        tags: Some(vec!["one".into(), "two".into()]),
        creative_statement: None,
        account: None,
        wechat_link: None,
    });
    let publisher = test_publisher(MockWebDriver::new(ordinary_replies("ready", "clicked")));
    publisher.publish(Platform::Kuaishou, &request).unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    let editor_lookups = bodies
        .iter()
        .filter(|body| body.get("value") == Some(&Value::String("#work-description-edit".into())))
        .count();
    assert_eq!(editor_lookups, 1);
    let text_inputs = bodies
        .iter()
        .filter_map(|body| body.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(text_inputs, vec!["movie.mp4", "Effective title #one #two"]);
}

#[test]
fn kuaishou_statement_finishes_before_the_preview_action_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(statement_replies()));
    publisher
        .publish(Platform::Kuaishou, &request_with_statement("AI生成"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("AI生成"))
    }));
    let statement = bodies
        .iter()
        .position(|body| body.get("args") == Some(&json!(["内容为AI生成"])))
        .unwrap();
    let action = bodies
        .iter()
        .rposition(|body| body.get("args") == Some(&json!([])))
        .unwrap();
    assert!(statement < action);
}

#[test]
fn kuaishou_none_unknown_and_missing_target_override_skip_statement_actions() {
    for request in [
        request(),
        request_with_statement("none"),
        request_with_statement("not-supported"),
    ] {
        let publisher = test_publisher(MockWebDriver::new(ordinary_replies("ready", "clicked")));
        publisher.publish(Platform::Kuaishou, &request).unwrap();
        assert!(
            !publisher
                .transport
                .bodies
                .lock()
                .unwrap()
                .iter()
                .any(|body| { body.get("args") == Some(&json!(["内容为AI生成"])) })
        );
    }
}

#[test]
fn kuaishou_missing_statement_selector_fails_closed() {
    assert_statement_failure(statement_failure_replies([value(json!(false))]));
}

#[test]
fn kuaishou_ambiguous_statement_selector_fails_closed() {
    let mut actions = vec![value(json!(true))];
    actions.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    assert_statement_failure(statement_failure_replies(actions));
}

#[test]
fn kuaishou_disabled_statement_option_fails_closed() {
    assert_statement_failure(statement_failure_replies([
        value(json!(true)),
        value(json!(true)),
        value(json!(false)),
    ]));
}

#[test]
fn kuaishou_unverified_statement_application_times_out_and_fails_closed() {
    let mut actions = vec![value(json!(true)), value(json!(true)), value(json!(true))];
    actions.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    assert_statement_failure(statement_failure_replies(actions));
}

#[test]
fn kuaishou_preview_pending_then_ready_runs_only_the_specialized_action() {
    let mut replies = ordinary_replies("ready", "clicked");
    replies.insert(8, value(json!("pending")));
    let publisher = test_publisher(MockWebDriver::new(replies));
    publisher.publish(Platform::Kuaishou, &request()).unwrap();
    {
        let bodies = publisher.transport.bodies.lock().unwrap();
        assert_eq!(
            bodies
                .iter()
                .filter(|body| body.get("args") == Some(&json!([])))
                .count(),
            3,
            "a pending probe, readiness probe, and revalidated action are required"
        );
    }
    assert_closed_without_generic_click(publisher);
}

#[test]
fn kuaishou_permanently_pending_preview_times_out_without_any_click() {
    let mut replies = terminal_replies("pending");
    replies.truncate(8);
    replies.extend((0..KUAISHOU_ACTION_POLL_ATTEMPTS).map(|_| value(json!("pending"))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(publisher.publish(Platform::Kuaishou, &request()).is_err());
    assert_closed_without_generic_click(publisher);
}

#[test]
fn kuaishou_missing_ambiguous_or_disabled_preview_actions_fail_closed() {
    for state in ["missing", "ambiguous", "disabled"] {
        let publisher = test_publisher(MockWebDriver::new(terminal_replies(state)));
        assert!(
            publisher.publish(Platform::Kuaishou, &request()).is_err(),
            "{state}"
        );
        assert_closed_without_generic_click(publisher);
    }
}

#[test]
fn kuaishou_action_race_fails_closed_without_a_generic_fallback() {
    let publisher = test_publisher(MockWebDriver::new(race_replies()));
    assert!(publisher.publish(Platform::Kuaishou, &request()).is_err());
    assert_closed_without_generic_click(publisher);
}

#[test]
fn kuaishou_draft_is_rejected_before_session_creation() {
    let mut draft = request();
    draft.draft = true;
    let publisher = test_publisher(MockWebDriver::new(Vec::new()));
    assert!(publisher.publish(Platform::Kuaishou, &draft).is_err());
    assert!(publisher.transport.paths.lock().unwrap().is_empty());
}
