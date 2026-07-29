use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

#[test]
fn wechat_publish_without_metadata_skips_unavailable_original_declaration() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
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
    let scripts = bodies
        .iter()
        .filter_map(|body| body.get("script").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(scripts, vec![WECHAT_ORIGINAL_ENTRY_SCRIPT, VISIBLE_SCRIPT]);
    assert!(!bodies.iter().any(|body| {
        body.get("value")
            == Some(&Value::String(
                profile(Platform::WechatChannels)
                    .unwrap()
                    .short_title
                    .unwrap()[0]
                    .into(),
            ))
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
        value(json!(true)),
        value(json!(true)),
        value(json!(false)),
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
    request.overrides.push(PlatformOverride {
        platform: Platform::WechatChannels,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some("marketing".into()),
        account: None,
        wechat_link: None,
    });
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
    for script in [
        WECHAT_PRODUCT_TYPE_READY_SCRIPT,
        WECHAT_PRODUCT_OPEN_CHOOSER_SCRIPT,
        WECHAT_PRODUCT_DIALOG_VISIBLE_SCRIPT,
        WECHAT_PRODUCT_SEARCH_SCRIPT,
        WECHAT_PRODUCT_EXACT_ROW_SCRIPT,
        WECHAT_PRODUCT_SELECT_EXACT_SCRIPT,
        WECHAT_PRODUCT_ADD_READY_SCRIPT,
        WECHAT_PRODUCT_ADD_SCRIPT,
        WECHAT_PRODUCT_ATTACHED_SCRIPT,
    ] {
        assert!(script.contains("shadowRoot"));
        assert!(scripts.contains(&script));
    }
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
    let last_product = scripts
        .iter()
        .position(|script| *script == WECHAT_PRODUCT_ATTACHED_SCRIPT)
        .unwrap();
    let creative_open = scripts
        .iter()
        .position(|script| *script == WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT)
        .unwrap();
    assert!(last_product < creative_open);
    assert!(scripts.contains(&WECHAT_ORIGINAL_ENTRY_SCRIPT));
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
fn wechat_short_title_uses_its_explicit_profile_selector() {
    let mock = MockWebDriver::new(vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        element("title"),
        value(json!(null)),
        element("short-title"),
        value(json!(null)),
        element("description"),
        value(json!(null)),
        value(json!(false)),
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
    request.short_title = Some("Short title".into());
    publisher
        .publish(Platform::WechatChannels, &request)
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("value")
            == Some(&Value::String(
                profile(Platform::WechatChannels)
                    .unwrap()
                    .short_title
                    .unwrap()[0]
                    .into(),
            ))
    }));
    assert!(
        bodies
            .iter()
            .any(|body| body.get("text") == Some(&Value::String("Short title".into())))
    );
}

#[test]
fn wechat_creative_statement_uses_label_without_leaking_raw_value_to_description() {
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
        value(json!(false)),
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
    request.overrides.push(PlatformOverride {
        platform: Platform::WechatChannels,
        title: None,
        short_title: None,
        tags: None,
        creative_statement: Some("AI生成".into()),
        account: None,
        wechat_link: None,
    });
    publisher
        .publish(Platform::WechatChannels, &request)
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    assert!(bodies.iter().any(|body| {
        body.get("script")
            == Some(&Value::String(
                WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT.into(),
            ))
            && body.get("args") == Some(&json!(["含AI生成内容"]))
    }));
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("AI生成"))
    }));
}

#[test]
fn wechat_none_or_unknown_creative_statement_skips_metadata_actions() {
    for statement in ["无需标注", "not-a-statement"] {
        let mock = MockWebDriver::new(vec![
            value(json!({"sessionId":"s"})),
            value(json!(null)),
            element("file"),
            value(json!(null)),
            element("title"),
            value(json!(null)),
            element("description"),
            value(json!(null)),
            value(json!(false)),
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
        request.overrides.push(PlatformOverride {
            platform: Platform::WechatChannels,
            title: None,
            short_title: None,
            tags: None,
            creative_statement: Some(statement.into()),
            account: None,
            wechat_link: None,
        });
        publisher
            .publish(Platform::WechatChannels, &request)
            .unwrap();
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
                        Some(WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT)
                            | Some(WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT)
                    )
                })
        );
    }
}

#[test]
fn wechat_original_protocol_then_declaration_confirms_before_submit() {
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
    let entry = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(WECHAT_ORIGINAL_ENTRY_SCRIPT.into()))
        })
        .unwrap();
    let confirm = bodies
        .iter()
        .position(|body| {
            body.get("script") == Some(&Value::String(WECHAT_ORIGINAL_CONFIRM_SCRIPT.into()))
        })
        .unwrap();
    let protocol_confirm = bodies
        .iter()
        .position(|body| {
            body.get("script")
                == Some(&Value::String(
                    WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT.into(),
                ))
        })
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&Value::String("button[type='submit']".into())))
        .unwrap();
    assert!(entry < protocol_confirm && protocol_confirm < confirm && confirm < submit);
}

#[test]
fn wechat_original_protocol_without_declaration_dialog_continues_to_publish() {
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
        value(json!(true)),
    ];
    replies.extend((0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.extend([
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    let publisher = test_publisher(MockWebDriver::new(replies));
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
    assert_eq!(
        publisher
            .publish(Platform::WechatChannels, &request)
            .unwrap(),
        "webdriver-sph-1"
    );
    assert!(
        !publisher
            .transport
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| {
                body.get("script") == Some(&Value::String(WECHAT_ORIGINAL_CONFIRM_SCRIPT.into()))
            })
    );
}

#[test]
fn wechat_original_protocol_failure_rejects_before_submit_and_closes_session() {
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
        Err("protocol confirmation action failed".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
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
    assert!(
        !publisher
            .transport
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.ends_with("/click"))
    );
}

#[test]
fn wechat_persistent_visible_declaration_dialog_rejects_before_submit_and_closes_session() {
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
        value(json!(false)),
        value(json!(true)),
        value(json!(true)),
    ];
    replies.extend((0..WECHAT_SHADOW_ACTION_POLL_ATTEMPTS).map(|_| value(json!(false))));
    replies.push(value(json!(null)));
    let publisher = test_publisher(MockWebDriver::new(replies));
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
    assert!(
        publisher
            .publish(Platform::WechatChannels, &request)
            .is_err()
    );
    let paths = publisher.transport.paths.lock().unwrap();
    assert!(paths.last().unwrap().ends_with("/session/s"));
    assert!(!paths.iter().any(|path| path.ends_with("/click")));
}

#[test]
fn wechat_visible_declaration_confirmation_failure_closes_the_temporary_session() {
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
        value(json!(false)),
        value(json!(true)),
        Err("declaration confirmation action failed".into()),
        value(json!(null)),
    ]);
    let publisher = test_publisher(mock);
    let mut request = request();
    request.targets = vec![Platform::WechatChannels];
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
        value(json!("ready")),
        value(json!("ready")),
        value(json!("clicked")),
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
fn wechat_shadow_action_deadline_is_fixed_and_finite() {
    assert_eq!(WECHAT_SHADOW_ACTION_POLL_ATTEMPTS, 30);
    assert_eq!(
        WECHAT_SHADOW_ACTION_POLL_INTERVAL,
        std::time::Duration::from_millis(200)
    );
    assert!(WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT.contains("button.disabled"));
    assert!(WECHAT_ORIGINAL_CONFIRM_SCRIPT.contains("button.disabled"));
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
