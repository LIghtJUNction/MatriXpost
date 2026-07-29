use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Baijiahao,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some(value.into()),
        account: None,
        wechat_link: None,
    });
    request
}

fn field_replies() -> Vec<Result<Value, String>> {
    vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
    ]
}

fn finish(
    mut replies: Vec<Result<Value, String>>,
    state: &str,
    action: Option<&str>,
) -> Vec<Result<Value, String>> {
    replies.extend([
        Err("not visible".into()),
        Err("not visible".into()),
        value(json!(state)),
    ]);
    if let Some(action) = action {
        replies.extend([value(json!(action)), element("success"), value(json!(true))]);
    }
    replies.push(value(json!(null)));
    replies
}

fn completed_replies(action: &str) -> Vec<Result<Value, String>> {
    finish(field_replies(), "ready", Some(action))
}

fn statement_replies(action: &str) -> Vec<Result<Value, String>> {
    let mut replies = field_replies();
    replies.extend((0..5).map(|_| value(json!(true))));
    finish(replies, "ready", Some(action))
}

fn action_called(bodies: &[Value], script: &str, action: &str) -> bool {
    bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(script.into()))
            && body.get("args") == Some(&json!([action]))
    })
}

fn assert_failed_without_generic_click(publisher: &WebDriverPublisher<MockWebDriver>) {
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(
        paths
            .last()
            .is_some_and(|path| path.ends_with("/session/s"))
    );
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn baijiahao_target_override_resolves_upstream_canonical_values_and_aliases() {
    for (value, expected) in [
        ("none", "无需声明"),
        ("无标注", "无需声明"),
        ("ai_generated", "含AI生成内容"),
        ("内容为AI生成", "含AI生成内容"),
        ("fiction", "含虚构演绎内容"),
        ("虚构演绎，故事经历", "含虚构演绎内容"),
        ("marketing", "内容含营销信息"),
        ("内容含营销推广信息", "内容含营销信息"),
        ("personal_opinion", "个人观点，仅供参考"),
        ("内容为个人观点或见解", "个人观点，仅供参考"),
        ("repost", "内容为转载"),
        ("素材来源于网络", "内容为转载"),
        ("self_shot", "无需声明"),
        ("unknown", "无需声明"),
    ] {
        assert_eq!(
            baijiahao_creative_statement_label(&request_with_statement(value)),
            Some(expected),
            "{value}"
        );
    }
}

#[test]
fn baijiahao_publish_uses_only_the_ready_publish_action() {
    let publisher = test_publisher(MockWebDriver::new(completed_replies("clicked")));
    publisher.publish(Platform::Baijiahao, &request()).unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(action_called(
        &bodies,
        BAIJIAHAO_ACTION_STATE_SCRIPT,
        "publish"
    ));
    assert!(action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
    assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "draft"));
}

#[test]
fn baijiahao_draft_uses_only_the_ready_draft_action() {
    let mut request = request();
    request.draft = true;
    let publisher = test_publisher(MockWebDriver::new(completed_replies("clicked")));
    publisher.publish(Platform::Baijiahao, &request).unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(action_called(
        &bodies,
        BAIJIAHAO_ACTION_STATE_SCRIPT,
        "draft"
    ));
    assert!(action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "draft"));
    assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
}

#[test]
fn baijiahao_upload_or_readiness_that_stays_pending_times_out_without_action() {
    let mut replies = field_replies();
    replies.extend([Err("not visible".into()), Err("not visible".into())]);
    replies.extend((0..BAIJIAHAO_ACTION_POLL_ATTEMPTS).map(|_| value(json!("pending"))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(publisher.publish(Platform::Baijiahao, &request()).is_err());
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert_eq!(
        bodies
            .iter()
            .filter(|body| {
                body.get("script") == Some(&Value::String(BAIJIAHAO_ACTION_STATE_SCRIPT.into()))
                    && body.get("args") == Some(&json!(["publish"]))
            })
            .count(),
        BAIJIAHAO_ACTION_POLL_ATTEMPTS
    );
    assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
    drop(bodies);
    assert_failed_without_generic_click(&publisher);
}

#[test]
fn baijiahao_without_target_statement_override_skips_declaration_actions() {
    let publisher = test_publisher(MockWebDriver::new(completed_replies("clicked")));
    publisher.publish(Platform::Baijiahao, &request()).unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(!bodies.iter().any(|body| {
        matches!(
            body.get("script").and_then(Value::as_str),
            Some(BAIJIAHAO_STATEMENT_OPEN_SCRIPT)
                | Some(BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT)
                | Some(BAIJIAHAO_STATEMENT_SELECT_SCRIPT)
                | Some(BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT)
                | Some(BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT)
        )
    }));
}

#[test]
fn baijiahao_ambiguous_or_disabled_declaration_fails_closed_before_action() {
    let ambiguous = test_publisher(MockWebDriver::new({
        let mut replies = field_replies();
        replies.extend([value(json!(true)), value(json!(false)), value(json!(null))]);
        replies
    }));
    assert!(
        ambiguous
            .publish(Platform::Baijiahao, &request_with_statement("ai_generated"))
            .is_err()
    );
    let bodies = ambiguous.transport.bodies.lock().unwrap();
    assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
    drop(bodies);
    assert_failed_without_generic_click(&ambiguous);

    let disabled = test_publisher(MockWebDriver::new({
        let mut replies = field_replies();
        replies.extend([
            value(json!(true)),
            value(json!(true)),
            value(json!(true)),
            value(json!(false)),
            value(json!(null)),
        ]);
        replies
    }));
    assert!(
        disabled
            .publish(Platform::Baijiahao, &request_with_statement("ai_generated"))
            .is_err()
    );
    let bodies = disabled.transport.bodies.lock().unwrap();
    assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
    drop(bodies);
    assert_failed_without_generic_click(&disabled);
}

#[test]
fn baijiahao_missing_or_ambiguous_root_or_button_fails_closed() {
    for state in ["missing", "ambiguous"] {
        let publisher = test_publisher(MockWebDriver::new(finish(field_replies(), state, None)));
        assert!(publisher.publish(Platform::Baijiahao, &request()).is_err());
        let bodies = publisher.transport.bodies.lock().unwrap();
        assert!(!action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
        drop(bodies);
        assert_failed_without_generic_click(&publisher);
    }
}

#[test]
fn baijiahao_disabled_or_changed_ready_action_fails_closed() {
    let disabled = test_publisher(MockWebDriver::new(finish(
        field_replies(),
        "disabled",
        None,
    )));
    assert!(disabled.publish(Platform::Baijiahao, &request()).is_err());
    assert_failed_without_generic_click(&disabled);

    let changed = test_publisher(MockWebDriver::new(finish(
        field_replies(),
        "ready",
        Some("disabled"),
    )));
    assert!(changed.publish(Platform::Baijiahao, &request()).is_err());
    let bodies = changed.transport.bodies.lock().unwrap();
    assert!(action_called(&bodies, BAIJIAHAO_ACTION_SCRIPT, "publish"));
    drop(bodies);
    assert_failed_without_generic_click(&changed);
}

#[test]
fn baijiahao_declaration_completes_before_action_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(statement_replies("clicked")));
    publisher
        .publish(Platform::Baijiahao, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    let declaration = bodies
        .iter()
        .position(|body| {
            body.get("script")
                == Some(&Value::String(
                    BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT.into(),
                ))
        })
        .unwrap();
    let action = bodies
        .iter()
        .position(|body| body.get("script") == Some(&Value::String(BAIJIAHAO_ACTION_SCRIPT.into())))
        .unwrap();
    assert!(declaration < action);
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("marketing"))
    }));
}
