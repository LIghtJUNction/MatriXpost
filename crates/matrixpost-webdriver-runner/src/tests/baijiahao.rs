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
fn baijiahao_statement_uses_canonical_label_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Baijiahao, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(BAIJIAHAO_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["内容含营销信息"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("marketing"))
    }));
    let statement_gone = bodies
        .iter()
        .position(|body| {
            body.get("script")
                == Some(&Value::String(
                    BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT.into(),
                ))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(statement_gone < submit);
}

#[test]
fn baijiahao_none_and_unsupported_values_actively_select_the_none_label() {
    for value in ["none", "无标注", "not-supported"] {
        let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
        publisher
            .publish(Platform::Baijiahao, &request_with_statement(value))
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
                        == Some(&Value::String(BAIJIAHAO_STATEMENT_SELECT_SCRIPT.into()))
                        && body.get("args") == Some(&json!(["无需声明"]))
                })
        );
    }
}

#[test]
fn baijiahao_without_target_override_does_not_run_statement_scripts() {
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
    publisher.publish(Platform::Baijiahao, &request()).unwrap();
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
                    Some(BAIJIAHAO_STATEMENT_OPEN_SCRIPT)
                        | Some(BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT)
                        | Some(BAIJIAHAO_STATEMENT_SELECT_SCRIPT)
                        | Some(BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT)
                        | Some(BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT)
                )
            })
    );
}

#[test]
fn baijiahao_ambiguous_visible_dialogs_fail_closed_and_clean_up_before_submit() {
    for script in [
        BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT,
        BAIJIAHAO_STATEMENT_SELECT_SCRIPT,
        BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT,
    ] {
        assert!(script.contains("matches.length!==1"));
        assert!(script.contains("getBoundingClientRect"));
    }
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
        // Two matching visible dialogs are represented by the false result:
        // the runner must reject the ambiguity rather than select either.
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Baijiahao, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn baijiahao_disabled_confirmation_fails_closed_and_cleans_up_before_submit() {
    assert!(BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT.contains("buttons.length!==1"));
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
            .publish(Platform::Baijiahao, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}
