use super::support::*;
use crate::api::{dispatch_response, parse_publish};

#[test]
fn malformed_http_json_is_a_bad_request() {
    assert_eq!(
        parse_publish(Bytes::from_static(b"{"))
            .unwrap_err()
            .status(),
        StatusCode::BAD_REQUEST
    );
}
#[test]
fn valid_http_request_is_parsed_but_not_queued() {
    let body = Bytes::from_static(br#"{"platform":"dy","file":"movie.mp4","title":"Title"}"#);
    let request = parse_publish(body).expect("valid request must be accepted");
    assert_eq!(request.targets, vec![Platform::Douyin]);
}
#[tokio::test]
async fn all_queued_report_is_accepted_but_mixed_report_fails_closed() {
    let request = parse_publish(Bytes::from_static(
        br#"{"platform":"dy","file":"movie.mp4","title":"Title"}"#,
    ))
    .unwrap();
    let queued = ProviderDispatchReport {
        outcomes: [(
            Platform::Douyin,
            DispatchOutcome::Queued {
                job_id: "job".into(),
            },
        )]
        .into_iter()
        .collect(),
    };
    assert_eq!(
        dispatch_response(request.clone(), queued).status(),
        StatusCode::ACCEPTED
    );
    let mixed = ProviderDispatchReport {
        outcomes: [
            (
                Platform::Douyin,
                DispatchOutcome::Queued {
                    job_id: "job".into(),
                },
            ),
            (
                Platform::Kuaishou,
                DispatchOutcome::Rejected {
                    reason: "runner failed".into(),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    assert_eq!(
        dispatch_response(request, mixed).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
#[test]
fn platform_contract_is_canonical() {
    assert_eq!(
        Platform::ALL
            .iter()
            .map(|platform| platform.metadata().code)
            .collect::<Vec<_>>(),
        vec!["dy", "sph", "blbl", "bjh", "tt", "ks", "xhs", "fqsp"]
    );
}
#[tokio::test]
async fn change_data_add_get_update_get_delete_get_and_config_roundtrip() {
    for file_name in [
        "account",
        "pushData",
        "config",
        "creative-statements",
        "platform-options",
    ] {
        let state = AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        };
        let router = app(state);
        let item = serde_json::json!({ "id": "dy", "platform": "dy", "value": "one" });
        let (status, body) = json_response(
            router.clone(),
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "add",
                "item": item.clone(),
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "ok");
        assert_eq!(body["data"]["key"], format!("{file_name}:dy"));
        assert_eq!(body["data"]["updated"], true);

        let (status, body) = json_response(
            router.clone(),
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "get",
                "item": { "id": "dy" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["value"], item.to_string());

        let updated = serde_json::json!({ "id": "dy", "platform": "dy", "value": "two" });
        let (status, body) = json_response(
            router.clone(),
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "update",
                "item": updated.clone(),
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["updated"], true);

        let (status, body) = json_response(
            router.clone(),
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "get",
                "item": { "id": "dy" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["value"], updated.to_string());

        let (status, body) = json_response(
            router.clone(),
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "delete",
                "item": { "id": "dy" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["deleted"], true);

        let (status, body) = json_response(
            router,
            change_data_request(serde_json::json!({
                "fileName": file_name,
                "type": "get",
                "item": { "id": "dy" },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["value"], serde_json::Value::Null);
    }
}
#[tokio::test]
async fn change_data_rejects_nested_secret_fields() {
    for forbidden in [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
    ] {
        let router = app(AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        });
        let (status, body) = json_response(
            router,
            change_data_request(serde_json::json!({
                "fileName": "account",
                "type": "add",
                "item": { "id": "dy", "nested": [{ (forbidden): "x" }] },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["ok"], false);
        assert_eq!(body["outcome"], "rejected");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("allowed non-secret")
        );
    }
}
#[tokio::test]
async fn change_data_rejects_structured_item_without_id() {
    let router = app(AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
    });
    let (status, body) = json_response(
        router,
        change_data_request(serde_json::json!({
            "fileName": "account",
            "type": "add",
            "item": { "platform": "dy" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["outcome"], "rejected");
}
#[test]
fn daemon_config_toml_state_path_and_cli_override_precedence() {
    let path = std::env::temp_dir().join(format!("matrixpostd-test-{}.toml", std::process::id()));
    std::fs::write(&path, "bind='127.0.0.1:9000'\nstate_path='/tmp/a.db'").unwrap();
    let config = DaemonConfig::load(Args {
        config: Some(path.clone()),
        bind: None,
        state_path: None,
    })
    .unwrap();
    assert_eq!(config.bind, "127.0.0.1:9000".parse().unwrap());
    assert_eq!(config.state_path, PathBuf::from("/tmp/a.db"));
    let loaded = DaemonConfig::load(Args {
        config: Some(path.clone()),
        bind: Some("127.0.0.1:9001".parse().unwrap()),
        state_path: Some(PathBuf::from("/tmp/b.db")),
    })
    .unwrap();
    assert_eq!(loaded.bind, "127.0.0.1:9001".parse().unwrap());
    assert_eq!(loaded.state_path, PathBuf::from("/tmp/b.db"));
    let _ = std::fs::remove_file(path);
}
#[tokio::test]
async fn router_platform_metadata_publish_503_and_invalid_dto_400() {
    let router = app(AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
    });
    let platforms = Request::get("/platforms").body(Body::empty()).unwrap();
    let (status, body) = json_response(router.clone(), platforms).await;
    assert_eq!(status, StatusCode::OK);
    let douyin = body
        .as_array()
        .unwrap()
        .iter()
        .find(|platform| platform["code"] == "dy")
        .expect("Douyin metadata must be present");
    assert_eq!(douyin["name"], "抖音");
    assert!(
        douyin["aliases"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("douyin"))
    );
    let providers = Request::get("/providers").body(Body::empty()).unwrap();
    let (status, body) = json_response(router.clone(), providers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["dy"]["outcome"], "unavailable");
    assert_eq!(body.as_object().unwrap().len(), Platform::ALL.len());
    let publish = Request::post("/publish")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"platforms":[{"platform":"sph","phone":"p"}],"file":"movie.mp4","title":"T","tags":"a,b","creativeStatements":{"视频号":"ok"},"sphProductId":"x","platformOptions":{"sph":{"link":{"type":"product","value":"x"}}}}"#,
        ))
        .unwrap();
    let (status, body) = json_response(router.clone(), publish).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["ok"], false);
    assert_eq!(body["outcome"], "unavailable");
    assert_eq!(body["data"]["accepted"], false);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("no provider implementation")
    );
    let invalid = Request::post("/publish")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = json_response(router, invalid).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);
    assert_eq!(body["outcome"], "rejected");
    assert_eq!(body["data"], serde_json::Value::Null);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("invalid JSON publish request")
    );
}
