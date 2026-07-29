use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Douyin,
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

#[test]
fn douyin_target_override_resolves_upstream_canonical_values_and_aliases() {
    for (value, expected) in [
        ("none", "无需添加自主声明"),
        ("内容无需标注", "无需添加自主声明"),
        ("ai_generated", "内容由AI生成"),
        ("内容为AI生成", "内容由AI生成"),
        ("fiction", "虚构演绎，仅供娱乐"),
        ("虚构演绎，故事经历", "虚构演绎，仅供娱乐"),
        ("marketing", "内容含营销推广信息"),
        ("内容含营销信息", "内容含营销推广信息"),
        ("personal_opinion", "内容为个人观点或见解"),
        ("个人观点，仅供参考", "内容为个人观点或见解"),
        ("repost", "内容为转载信息"),
        ("素材来源于网络", "内容为转载信息"),
        ("self_shot", "无需添加自主声明"),
        ("self_made_no_repost", "无需添加自主声明"),
    ] {
        assert_eq!(
            douyin_autonomous_statement_label(&request_with_statement(value)),
            Some(expected),
            "{value}"
        );
    }
}

#[test]
fn douyin_target_statement_uses_canonical_label_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Douyin, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(DOUYIN_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["内容含营销推广信息"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("marketing"))
    }));
    let statement_gone = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(statement_gone < submit);
}

#[test]
fn douyin_none_and_unsupported_target_values_actively_select_the_none_label() {
    for value in ["none", "无标注", "not-supported"] {
        let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
        publisher
            .publish(Platform::Douyin, &request_with_statement(value))
            .unwrap();
        assert!(
            publisher
                .transport
                .bodies
                .lock()
                .unwrap()
                .iter()
                .any(|body| {
                    body.get("script")
                        == Some(&Value::String(DOUYIN_STATEMENT_SELECT_SCRIPT.into()))
                        && body.get("args") == Some(&json!(["无需添加自主声明"]))
                })
        );
    }
}

#[test]
fn douyin_without_target_override_does_not_run_statement_scripts() {
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
    publisher.publish(Platform::Douyin, &request()).unwrap();
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
                    Some(DOUYIN_STATEMENT_OPEN_SCRIPT)
                        | Some(DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT)
                        | Some(DOUYIN_STATEMENT_SELECT_SCRIPT)
                        | Some(DOUYIN_STATEMENT_CONFIRM_SCRIPT)
                        | Some(DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT)
                )
            })
    );
}

#[test]
fn douyin_absent_selector_fails_closed_and_cleans_up_before_submit() {
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
            .publish(Platform::Douyin, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn douyin_disabled_confirmation_fails_closed_and_cleans_up_before_submit() {
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
        value(json!(true)),
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Douyin, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn douyin_two_visible_matching_dialogs_with_one_enabled_confirmation_fail_closed() {
    assert!(DOUYIN_STATEMENT_CONFIRM_SCRIPT.contains("if(matches.length!==1)return false"));
    assert!(DOUYIN_STATEMENT_CONFIRM_SCRIPT.contains("if(buttons.length!==1)return false"));
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
        value(json!(true)),
        // The mock models a page with two visible matching modals: one has a
        // disabled confirm button and one is enabled. The script must reject
        // the ambiguity instead of selecting the enabled modal.
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Douyin, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn douyin_persistent_visible_dialog_fails_closed_and_cleans_up_before_submit() {
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
        value(json!(true)),
    ];
    replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(Platform::Douyin, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn douyin_hidden_unrelated_modal_does_not_mask_a_persistent_visible_declaration() {
    assert!(
        DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT.contains("querySelectorAll('.semi-modal-body')")
    );
    for script in [
        DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT,
        DOUYIN_STATEMENT_SELECT_SCRIPT,
        DOUYIN_STATEMENT_CONFIRM_SCRIPT,
    ] {
        assert!(script.contains("matches.length"));
        assert!(script.contains("getBoundingClientRect"));
    }
    assert!(DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT.contains("matches.length===0"));
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
        value(json!(true)),
    ];
    replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(Platform::Douyin, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}
