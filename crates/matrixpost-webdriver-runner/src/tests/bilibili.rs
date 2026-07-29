use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Bilibili,
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
        value(json!("ready")),
        element("title"),
        value(json!(null)),
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
    ]
}

fn bilibili_upload_replies(
    states: impl IntoIterator<Item = &'static str>,
) -> Vec<Result<Value, String>> {
    let mut replies = vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
    ];
    replies.extend(states.into_iter().map(|state| value(json!(state))));
    replies.push(value(json!(null)));
    replies
}

fn assert_bilibili_upload_failed_before_metadata_and_action(
    publisher: &WebDriverPublisher<MockWebDriver>,
) {
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(
        paths
            .last()
            .is_some_and(|path| path.ends_with("/session/s"))
    );
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
    drop(paths);
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(!bodies.iter().any(|body| {
        body.get("text") == Some(&Value::String("Title".into()))
            || body.get("text") == Some(&Value::String("Description".into()))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(BILIBILI_STATEMENT_OPEN_SCRIPT.into()))
    }));
}

#[test]
fn bilibili_target_override_resolves_upstream_canonical_values_and_aliases() {
    for (value, expected) in [
        ("none", "内容无需标注"),
        ("无标注", "内容无需标注"),
        ("内容无需标注", "内容无需标注"),
        ("无需添加自主声明", "内容无需标注"),
        ("无需标注", "内容无需标注"),
        ("无需声明", "内容无需标注"),
        ("ai_generated", "含AI生成内容"),
        ("AI生成", "含AI生成内容"),
        ("内容由AI生成", "含AI生成内容"),
        ("内容为AI生成", "含AI生成内容"),
        ("笔记含AI合成内容", "含AI生成内容"),
        ("fiction", "含虚构演绎内容"),
        ("虚构演绎", "含虚构演绎内容"),
        ("虚构演绎，故事经历", "含虚构演绎内容"),
        ("内容为虚构剧情，仅供娱乐", "含虚构演绎内容"),
        ("marketing", "内容含营销信息"),
        ("营销推广", "内容含营销信息"),
        ("内容含营销推广信息", "内容含营销信息"),
        ("内容包含营销广告", "内容含营销信息"),
        ("personal_opinion", "个人观点，仅供参考"),
        ("个人观点", "个人观点，仅供参考"),
        ("内容为个人观点或见解", "个人观点，仅供参考"),
        ("repost", "内容为转载"),
        ("转载", "内容为转载"),
        ("素材来源于网络", "内容为转载"),
        ("self_made_no_repost", "内容为自制：未经作者允许，禁止转载"),
        ("自制禁转载", "内容为自制：未经作者允许，禁止转载"),
        ("self_shot", "自行拍摄"),
        ("自行拍摄", "自行拍摄"),
        ("内容为自行拍摄", "自行拍摄"),
        ("unknown", "内容无需标注"),
    ] {
        assert_eq!(
            bilibili_creative_statement_label(&request_with_statement(value)),
            Some(expected),
            "{value}"
        );
    }
}

#[test]
fn bilibili_target_statement_selects_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Bilibili, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(BILIBILI_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["内容含营销信息"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("marketing"))
    }));
    let list_gone = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(BILIBILI_STATEMENT_LIST_GONE_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(list_gone < submit);
}

#[test]
fn bilibili_upload_readiness_precedes_metadata_statement_and_final_action() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Bilibili, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    let ready = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(BILIBILI_UPLOAD_READY_STATE_SCRIPT.into()))
        })
        .unwrap();
    let title = bodies
        .iter()
        .position(|body| {
            body.get("value")
                == Some(&Value::String(
                    profile(Platform::Bilibili).unwrap().title[0].into(),
                ))
        })
        .unwrap();
    let statement = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(BILIBILI_STATEMENT_OPEN_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(ready < title && title < statement && statement < submit);
}

#[test]
fn bilibili_upload_pending_uses_the_complete_bounded_poll_budget_without_actions() {
    let publisher = test_publisher(MockWebDriver::new(bilibili_upload_replies(
        std::iter::repeat_n("pending", BILIBILI_UPLOAD_READY_POLL_ATTEMPTS),
    )));
    assert!(publisher.publish(Platform::Bilibili, &request()).is_err());
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert_eq!(
        bodies
            .iter()
            .filter(|body| {
                body.get("script")
                    == Some(&Value::String(BILIBILI_UPLOAD_READY_STATE_SCRIPT.into()))
            })
            .count(),
        BILIBILI_UPLOAD_READY_POLL_ATTEMPTS
    );
    drop(bodies);
    assert_bilibili_upload_failed_before_metadata_and_action(&publisher);
}

#[test]
fn bilibili_ambiguous_upload_fails_closed_before_metadata_and_action() {
    let publisher = test_publisher(MockWebDriver::new(bilibili_upload_replies(["ambiguous"])));
    assert!(publisher.publish(Platform::Bilibili, &request()).is_err());
    assert_bilibili_upload_failed_before_metadata_and_action(&publisher);
}

#[test]
fn bilibili_invalid_upload_state_fails_closed_before_metadata_and_action() {
    let publisher = test_publisher(MockWebDriver::new(bilibili_upload_replies(["invalid"])));
    assert!(publisher.publish(Platform::Bilibili, &request()).is_err());
    assert_bilibili_upload_failed_before_metadata_and_action(&publisher);
}

#[test]
fn bilibili_none_and_unknown_values_actively_select_the_none_label() {
    for value in ["none", "内容无需标注", "not-supported"] {
        let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
        publisher
            .publish(Platform::Bilibili, &request_with_statement(value))
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
                        == Some(&Value::String(BILIBILI_STATEMENT_SELECT_SCRIPT.into()))
                        && body.get("args") == Some(&json!(["内容无需标注"]))
                })
        );
    }
}

#[test]
fn bilibili_without_target_override_does_not_run_statement_scripts() {
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
        value(json!(true)),
        value(json!(true)),
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]));
    publisher.publish(Platform::Bilibili, &request()).unwrap();
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
                    Some(BILIBILI_STATEMENT_OPEN_SCRIPT)
                        | Some(BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT)
                        | Some(BILIBILI_STATEMENT_SELECT_SCRIPT)
                        | Some(BILIBILI_STATEMENT_LIST_GONE_SCRIPT)
                )
            })
    );
}

#[test]
fn bilibili_ambiguous_visible_lists_fail_closed_and_clean_up_before_submit() {
    for script in [
        BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT,
        BILIBILI_STATEMENT_SELECT_SCRIPT,
    ] {
        assert!(script.contains("matches.length===1") || script.contains("matches.length!==1"));
        assert!(script.contains("getBoundingClientRect"));
    }
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Bilibili, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn bilibili_disabled_option_fails_closed_and_cleans_up_before_submit() {
    assert!(BILIBILI_STATEMENT_SELECT_SCRIPT.contains("action?.disabled"));
    assert!(BILIBILI_STATEMENT_SELECT_SCRIPT.contains("aria-disabled"));
    let publisher = test_publisher(MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(true)),
        value(json!(false)),
        value(json!(null)),
    ]));
    assert!(
        publisher
            .publish(Platform::Bilibili, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn bilibili_persistent_visible_list_fails_closed_and_cleans_up_before_submit() {
    assert!(BILIBILI_STATEMENT_LIST_GONE_SCRIPT.contains("matches.length===0"));
    let mut replies = vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
        value(json!(true)),
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
            .publish(Platform::Bilibili, &request_with_statement("ai_generated"))
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}
