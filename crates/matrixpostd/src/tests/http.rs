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
            article_runner: None,
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
            article_runner: None,
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
        article_runner: None,
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
        article_runner: None,
    });
    let platforms = Request::get("/platforms").body(Body::empty()).unwrap();
    let (status, body) = json_response(router.clone(), platforms).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    let platform_list = body["platforms"].as_array().unwrap();
    assert_eq!(platform_list.len(), 8);
    let douyin = platform_list
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

#[tokio::test]
async fn upstream_metadata_routes_return_deterministic_compatibility_specs() {
    let router = app(AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
        article_runner: None,
    });

    let (status, platforms) = json_response(
        router.clone(),
        Request::get("/platforms").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(platforms["success"], true);
    assert_eq!(
        platforms["platforms"][0],
        serde_json::json!({
            "code": "dy",
            "name": "抖音",
            "aliases": ["douyin", "抖音"],
            "automated": true,
            "note": null,
            "hasConfig": null,
        })
    );
    assert_eq!(platforms["platforms"][7]["code"], "fqsp");
    assert_eq!(platforms["platforms"][7]["automated"], false);
    assert_eq!(
        platforms["platforms"][7]["hasConfig"],
        serde_json::Value::Null
    );
    let (repeat_status, repeated_platforms) = json_response(
        router.clone(),
        Request::get("/platforms").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(repeat_status, StatusCode::OK);
    assert_eq!(repeated_platforms, platforms);

    let (status, statements) = json_response(
        router.clone(),
        Request::get("/creative-statements")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(statements["success"], true);
    assert_eq!(statements["default"], "none");
    assert_eq!(statements["batchOptions"].as_array().unwrap().len(), 8);
    assert_eq!(
        statements["batchOptions"][6],
        serde_json::json!({
            "value": "self_shot",
            "label": "自行拍摄",
            "onlyPlatforms": ["sph"],
        })
    );
    assert_eq!(
        statements["platforms"]["sph"]["options"][6],
        serde_json::json!({
            "value": "self_shot",
            "label": "内容为自行拍摄",
        })
    );
    assert_eq!(
        statements["platforms"]["tt"]["options"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        statements["platforms"]["fqsp"],
        serde_json::json!({
            "name": "番茄视频",
            "supports": false,
            "options": [],
        })
    );
    assert_eq!(
        statements["input"]["fallback"],
        "所选声明在某平台无对应选项时自动回退为 none（无标注）"
    );

    for (platform, name, supports, values) in [
        (
            "dy",
            "抖音",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
            ][..],
        ),
        (
            "sph",
            "视频号",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
                "self_shot",
            ][..],
        ),
        (
            "blbl",
            "哔哩哔哩",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
                "self_made_no_repost",
            ][..],
        ),
        (
            "bjh",
            "百家号",
            true,
            &[
                "none",
                "ai_generated",
                "fiction",
                "marketing",
                "personal_opinion",
                "repost",
            ][..],
        ),
        (
            "tt",
            "头条",
            true,
            &["ai_generated", "fiction", "repost"][..],
        ),
        (
            "ks",
            "快手",
            true,
            &["ai_generated", "fiction", "personal_opinion", "repost"][..],
        ),
        (
            "xhs",
            "小红书",
            true,
            &["ai_generated", "fiction", "marketing"][..],
        ),
        ("fqsp", "番茄视频", false, &[][..]),
    ] {
        assert_eq!(statements["platforms"][platform]["name"], name);
        assert_eq!(statements["platforms"][platform]["supports"], supports);
        assert_eq!(
            statements["platforms"][platform]["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|option| option["value"].as_str().unwrap())
                .collect::<Vec<_>>(),
            values
        );
    }

    let (repeat_status, repeated_statements) = json_response(
        router,
        Request::get("/creative-statements")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(repeat_status, StatusCode::OK);
    assert_eq!(repeated_statements, statements);
}

#[tokio::test]
async fn publish_records_a_terminal_history_entry_before_returning_queued() {
    let (state, _) = scheduler_state(
        matrixpost_core::ProviderAvailability::Available,
        DispatchOutcome::Queued {
            job_id: "local-job".into(),
        },
    );
    let repository = Arc::clone(&state.repository);
    let router = app(state);
    let (status, body) = json_response(
        router,
        Request::post("/publish")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"platform":"dy","file":"movie.mp4","title":"Title"}"#,
            ))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["outcome"], "queued");
    let history = repository.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, matrixpost_core::PublishState::Published);
}

#[tokio::test]
async fn upstream_test_probe_and_api_success_alias_are_compatible() {
    let router = app(AppState {
        repository: Arc::new(SqliteRepository::in_memory().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
        article_runner: None,
    });

    let (status, body) = json_response(
        router.clone(),
        Request::get("/test").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "success": true, "message": "ok" })
    );

    let (status, body) = json_response(
        router.clone(),
        change_data_request(serde_json::json!({
            "fileName": "config",
            "type": "get",
            "item": { "id": "dy" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], body["ok"]);

    let (status, body) = json_response(
        router,
        Request::post("/publish")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["success"], body["ok"]);
}
