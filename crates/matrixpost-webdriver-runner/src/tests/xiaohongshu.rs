use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_statement(value: &str) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Xiaohongshu,
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
        value(json!("description")),
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
fn xiaohongshu_target_override_resolves_only_upstream_supported_values() {
    for (value, expected) in [
        ("ai_generated", Some("笔记含AI合成内容")),
        ("AI生成", Some("笔记含AI合成内容")),
        ("内容由AI生成", Some("笔记含AI合成内容")),
        ("fiction", Some("虚构演绎，仅供娱乐")),
        ("演绎情节，仅供娱乐", Some("虚构演绎，仅供娱乐")),
        ("marketing", Some("内容包含营销广告")),
        ("内容含营销推广信息", Some("内容包含营销广告")),
        ("none", None),
        ("无标注", None),
        ("personal_opinion", None),
        ("repost", None),
        ("unknown", None),
    ] {
        assert_eq!(
            xiaohongshu_creative_statement_label(&request_with_statement(value)),
            expected,
            "{value}"
        );
    }

    let mut another_platform = request();
    another_platform.overrides.push(PlatformOverride {
        platform: Platform::Douyin,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some("ai_generated".into()),
        account: None,
        wechat_link: None,
    });
    assert_eq!(
        xiaohongshu_creative_statement_label(&another_platform),
        None
    );
}

#[test]
fn xiaohongshu_selects_before_submit_without_description_leak() {
    let publisher = test_publisher(MockWebDriver::new(completed_statement_replies()));
    publisher
        .publish(Platform::Xiaohongshu, &request_with_statement("marketing"))
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script") == Some(&Value::String(XIAOHONGSHU_STATEMENT_SELECT_SCRIPT.into()))
            && body.get("args") == Some(&json!(["内容包含营销广告"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("marketing"))
    }));
    let applied = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(XIAOHONGSHU_STATEMENT_APPLIED_SCRIPT.into()))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(applied < submit);
}

#[test]
fn xiaohongshu_replacement_placeholder_proof_allows_submit() {
    // This completed response sequence models the upstream UI variant that
    // replaces the declaration prompt with the selected label instead of
    // rendering a description node. The runner treats the boolean proof as
    // authoritative and still requires it before the submit request.
    let mut replies = completed_statement_replies();
    replies[11] = value(json!("placeholder"));
    let publisher = test_publisher(MockWebDriver::new(replies));
    publisher
        .publish(Platform::Xiaohongshu, &request_with_statement("fiction"))
        .unwrap();
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("/element/submit/click"))
    );
}

#[test]
fn xiaohongshu_prompt_or_open_proof_fails_closed_before_submit() {
    for proof in ["prompt", "open"] {
        let mut replies = completed_statement_replies();
        replies.truncate(12);
        replies[11] = value(json!(proof));
        replies.extend((1..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(proof))));
        replies.push(value(json!(null)));
        let publisher = test_publisher(MockWebDriver::new(replies));
        assert!(
            publisher
                .publish(Platform::Xiaohongshu, &request_with_statement("fiction"))
                .is_err()
        );
        let paths = publisher.transport.paths.lock().unwrap();
        assert!(paths.last().unwrap().ends_with("/session/s"));
        assert!(!paths.iter().any(|path| path.ends_with("/click")));
    }
}

#[test]
fn xiaohongshu_missing_none_unknown_or_other_platform_override_skips_statement_actions() {
    let mut other_platform = request();
    other_platform.overrides.push(PlatformOverride {
        platform: Platform::Kuaishou,
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
        request_with_statement("not-supported"),
        other_platform,
    ] {
        let publisher = test_publisher(MockWebDriver::new(completed_default_replies()));
        publisher.publish(Platform::Xiaohongshu, &request).unwrap();
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
                        Some(XIAOHONGSHU_STATEMENT_OPEN_SCRIPT)
                            | Some(XIAOHONGSHU_STATEMENT_LIST_VISIBLE_SCRIPT)
                            | Some(XIAOHONGSHU_STATEMENT_SELECT_SCRIPT)
                            | Some(XIAOHONGSHU_STATEMENT_APPLIED_SCRIPT)
                    )
                })
        );
    }
}

#[test]
fn xiaohongshu_missing_or_ambiguous_selector_or_option_fails_closed_before_submit() {
    for replies in [
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
            value(json!(null)),
        ],
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
            value(json!(false)),
            value(json!(null)),
        ],
    ] {
        let publisher = test_publisher(MockWebDriver::new(replies));
        assert!(
            publisher
                .publish(
                    Platform::Xiaohongshu,
                    &request_with_statement("ai_generated")
                )
                .is_err()
        );
        let paths = publisher.transport.paths.lock().unwrap();
        assert!(paths.last().unwrap().ends_with("/session/s"));
        assert!(!paths.iter().any(|path| path.ends_with("/click")));
    }
}

#[test]
fn xiaohongshu_disabled_option_fails_closed_before_submit() {
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
            .publish(
                Platform::Xiaohongshu,
                &request_with_statement("ai_generated")
            )
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn xiaohongshu_unverified_selection_fails_closed_before_submit() {
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
    replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!("pending"))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    assert!(
        publisher
            .publish(
                Platform::Xiaohongshu,
                &request_with_statement("ai_generated")
            )
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}
