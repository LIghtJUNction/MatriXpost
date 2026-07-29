use super::support::*;
use crate::{profiles::*, webdriver::*};
use matrixpost_core::*;
use serde_json::{Value, json};

fn request_with_tags(tags: &[&str], statement: Option<&str>) -> PublishRequest {
    let mut request = request();
    request.overrides.push(PlatformOverride {
        platform: Platform::Bilibili,
        title: None,
        short_title: None,
        tags: Some(tags.iter().map(|tag| (*tag).into()).collect()),
        creative_statement: statement.map(Into::into),
        account: None,
        wechat_link: None,
    });
    request
}

fn completed_tag_replies(tags: usize, statement: bool) -> Vec<Result<Value, String>> {
    let mut replies = vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
    ];
    replies.extend((0..tags).flat_map(|_| [value(json!(true)), value(json!(true))]));
    if statement {
        replies.extend((0..4).map(|_| value(json!(true))));
    }
    replies.extend([
        Err("not visible".into()),
        Err("not visible".into()),
        element("submit"),
        value(json!(null)),
        element("success"),
        value(json!(true)),
        value(json!(null)),
    ]);
    replies
}

fn failed_tag_replies(committed: bool) -> Vec<Result<Value, String>> {
    let mut replies = vec![
        value(json!({"sessionId":"s"})),
        value(json!(null)),
        element("file"),
        value(json!(null)),
        value(json!("ready")),
        element("title"),
        value(json!(null)),
    ];
    if committed {
        replies.push(value(json!(true)));
        replies.extend((0..DOUYIN_STATEMENT_POLL_ATTEMPTS).map(|_| value(json!(false))));
    } else {
        replies.push(value(json!(false)));
    }
    replies.push(value(json!(null)));
    replies
}

#[test]
fn bilibili_submits_each_override_tag_before_statement_and_final_action() {
    let publisher = test_publisher(MockWebDriver::new(completed_tag_replies(2, true)));
    publisher
        .publish(
            Platform::Bilibili,
            &request_with_tags(&["override-one", "override-two"], Some("marketing")),
        )
        .unwrap();
    let bodies = publisher.transport.bodies.lock().unwrap();
    let submitted = bodies
        .iter()
        .filter(|body| body.get("script") == Some(&json!(BILIBILI_TAG_SUBMIT_SCRIPT)))
        .map(|body| body["args"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        submitted,
        vec![json!(["override-one"]), json!(["override-two"])]
    );
    let committed = bodies
        .iter()
        .rposition(|body| body.get("script") == Some(&json!(BILIBILI_TAG_COMMITTED_SCRIPT)))
        .unwrap();
    let statement = bodies
        .iter()
        .position(|body| body.get("script") == Some(&json!(BILIBILI_STATEMENT_OPEN_SCRIPT)))
        .unwrap();
    let submit = bodies
        .iter()
        .position(|body| body.get("value") == Some(&json!("button[type='submit']")))
        .unwrap();
    assert!(committed < statement && statement < submit);
    assert!(!bodies.iter().any(|body| {
        body.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("#tag") || text.contains("#override"))
    }));
}

#[test]
fn bilibili_false_tag_submit_or_commit_fails_closed_before_final_action() {
    for committed in [false, true] {
        let publisher = test_publisher(MockWebDriver::new(failed_tag_replies(committed)));
        assert!(
            publisher
                .publish(Platform::Bilibili, &request_with_tags(&["tag"], None))
                .is_err()
        );
        let paths = publisher.transport.paths.lock().unwrap();
        assert!(
            paths
                .last()
                .is_some_and(|path| path.ends_with("/session/s"))
        );
        assert!(!paths.iter().any(|path| path.ends_with("/click")));
    }
}
