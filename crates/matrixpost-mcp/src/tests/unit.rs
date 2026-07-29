use super::*;

#[test]
fn scheduled_mcp_article_is_local_only_and_history_is_redacted() {
    let service = service();
    let result = service
        .publish_article_result(PublishArticleInput {
            platform: ArticlePlatformInput::Juejin,
            phone: "13800138000".into(),
            title: "Safe title".into(),
            content: Some("private body http://127.0.0.1:39002/secret".into()),
            file: Some("/private/article.md".into()),
            cover: None,
            category: None,
            tags: None,
            summary: None,
            publish_at: Some("2026-08-01 10:20:00".into()),
            show: None,
        })
        .unwrap();
    assert_eq!(result.outcome, "scheduled_locally");
    assert!(!result.remote_publish_attempted);

    let due = matrixpost_core::LocalSchedule::parse("2026-08-01 10:20:00").unwrap();
    let claimed = matrixpost_core::ArticlePublicationQueue::claim_due_articles(
        service.repository.as_ref(),
        &due,
        chrono::Utc::now(),
        1,
    )
    .unwrap();
    matrixpost_core::ArticlePublicationQueue::complete_article_with_history(
        service.repository.as_ref(),
        &claimed[0].id,
        claimed[0].revision,
        matrixpost_core::PublishState::Unavailable,
        chrono::Utc::now(),
        Some("http://127.0.0.1:39002 private body"),
    )
    .unwrap();
    let history = service.list_article_history_result().unwrap();
    assert_eq!(history.len(), 1);
    let serialized = serde_json::to_string(&history).unwrap();
    for forbidden in [
        "private body",
        "/private/article.md",
        "13800138000",
        "127.0.0.1:39002",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn video_request_maps_upstream_arguments_without_provider_side_effects() {
    let request = video_request(PublishVideoInput {
        platform: VideoPlatform::Sph,
        file: "https://example.invalid/video.mp4".into(),
        title: "Title".into(),
        phone: "13800138000".into(),
        bt2: Some("Short".into()),
        tags: Some("one,two three".into()),
        address: Some("Somewhere".into()),
        publish_at: Some("2026-08-01 10:20".into()),
        show: Some(true),
        draft: Some(true),
        creative_statement: Some("original".into()),
        sph_product_id: Some("product-1".into()),
        sph_link: None,
    })
    .unwrap();
    assert_eq!(request.targets, vec![Platform::WechatChannels]);
    assert_eq!(request.wechat_link.product_id.as_deref(), Some("product-1"));
    assert_eq!(request.wechat_link.link_type.as_deref(), Some("product"));
    assert_eq!(request.wechat_link.link_value.as_deref(), Some("product-1"));
    assert_eq!(request.scheduled_at.unwrap().0, "2026-08-01 10:20:00");
    assert_eq!(request.tags, vec!["one", "two", "three"]);
}

#[test]
fn list_accounts_reads_persisted_juejin_account_metadata() {
    let service = service();
    service
        .repository
        .save_article_account(&ArticleAccount {
            id: "juejin-primary".into(),
            platform: ArticlePlatform::Juejin,
            display_name: "Primary".into(),
            status: ArticleAccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:juejin-primary".into(),
        })
        .unwrap();
    let result = service
        .list_accounts_result(ListAccountsInput {
            platform: Some(AccountsPlatform::Juejin),
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!([{
            "phone": "13800138000",
            "platform": "juejin",
            "partition": "persist:juejin-primary"
        }])
    );
}

#[test]
fn video_platform_schema_excludes_non_upstream_targets() {
    assert_eq!(video_platform(VideoPlatform::Sph), Platform::WechatChannels);
    assert!(
        serde_json::from_value::<PublishVideoInput>(serde_json::json!({
            "platform": "xhs", "file": "/tmp/video.mp4", "title": "T", "phone": "p"
        }))
        .is_err()
    );
}

#[test]
fn article_tags_accept_the_upstream_string_and_normalize_to_core_tags() {
    let input = serde_json::from_value::<PublishArticleInput>(serde_json::json!({
        "platform": "juejin",
        "phone": "13800138000",
        "title": "Title",
        "content": "Body",
        "tags": "one,two three"
    }))
    .unwrap();
    let request = article_request(input).unwrap();
    assert_eq!(request.tags, vec!["one", "two", "three"]);
}

#[test]
fn history_platform_schema_rejects_fqsp_and_filters_through_the_core_query() {
    assert!(
        serde_json::from_value::<ListHistoryInput>(serde_json::json!({
            "platform": "fqsp"
        }))
        .is_err()
    );
    let history = service()
        .list_history_result(ListHistoryInput {
            days: None,
            platform: None,
            status: None,
            all: Some(true),
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(history).unwrap(),
        serde_json::json!([])
    );

    let service = service();
    let request = video_request(PublishVideoInput {
        platform: VideoPlatform::Dy,
        file: "/tmp/video.mp4".into(),
        title: "Title".into(),
        phone: "13800138000".into(),
        bt2: None,
        tags: None,
        address: None,
        publish_at: None,
        show: None,
        draft: Some(true),
        creative_statement: None,
        sph_product_id: None,
        sph_link: None,
    })
    .unwrap();
    let mut other_platform = request.clone();
    other_platform.targets = vec![Platform::Bilibili];
    let records = vec![
        HistoryRecord {
            id: "scheduled-dy".into(),
            request: request.clone(),
            state: PublishState::Queued,
            recorded_at: Utc::now(),
            detail: None,
        },
        HistoryRecord {
            id: "draft-dy".into(),
            request: request.clone(),
            state: PublishState::Draft,
            recorded_at: Utc::now(),
            detail: None,
        },
        HistoryRecord {
            id: "published-dy".into(),
            request,
            state: PublishState::Published,
            recorded_at: Utc::now(),
            detail: None,
        },
        HistoryRecord {
            id: "scheduled-blbl".into(),
            request: other_platform,
            state: PublishState::Queued,
            recorded_at: Utc::now(),
            detail: None,
        },
    ];
    for record in &records {
        service.repository.append_history(record).unwrap();
    }
    let input = ListHistoryInput {
        days: None,
        platform: Some(HistoryPlatform::Dy),
        status: Some(HistoryStatusInput::Scheduled),
        all: Some(true),
    };
    let expected = HistoryFilter::from_query(
        input.days,
        true,
        Some(Platform::Douyin),
        Some(HistoryStatus::Scheduled),
        Utc::now(),
    )
    .unwrap()
    .filter(records);
    let actual = service.list_history_result(input).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        actual
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        ["scheduled-dy"]
    );
}

#[test]
fn article_schedule_accepts_time_only_and_full_seconds_forms() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
    assert_eq!(
        parse_article_schedule("10:20", date).unwrap().0,
        "2026-08-01 10:20:00"
    );
    assert_eq!(
        parse_article_schedule("2026-08-02 10:20:30", date)
            .unwrap()
            .0,
        "2026-08-02 10:20:30"
    );
}

#[test]
fn sph_link_schema_rejects_missing_or_arbitrary_link_details() {
    assert!(serde_json::from_value::<SphLinkInput>(serde_json::json!({})).is_err());
    assert!(
        serde_json::from_value::<SphLinkInput>(serde_json::json!({
            "type": "article", "value": "value"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SphLinkInput>(serde_json::json!({
            "type": "none", "value": "unexpected"
        }))
        .is_err()
    );
}

#[test]
fn sph_product_link_requires_value_and_product_id_takes_precedence() {
    let missing_value = effective_sph_link(
        None,
        Some(SphLinkInput::Product {
            value: String::new(),
        }),
    )
    .unwrap_err();
    assert_eq!(
        missing_value,
        "sphLink.value must not be empty when sphLink.type is product"
    );
    let effective =
        effective_sph_link(Some("product-id".into()), Some(SphLinkInput::None {})).unwrap();
    assert_eq!(effective.product_id.as_deref(), Some("product-id"));
    assert_eq!(effective.link_type.as_deref(), Some("product"));
    assert_eq!(effective.link_value.as_deref(), Some("product-id"));
}

#[test]
fn article_request_rejects_missing_content_and_file() {
    let error = article_request(PublishArticleInput {
        platform: ArticlePlatformInput::Juejin,
        phone: "13800138000".into(),
        title: "Title".into(),
        content: None,
        file: None,
        cover: None,
        category: None,
        tags: None,
        summary: None,
        publish_at: None,
        show: None,
    })
    .unwrap_err();
    assert_eq!(error, "article content or file is required");
}

#[test]
fn immediate_video_without_a_runner_reports_unavailable_without_persisting() {
    let result = service()
        .publish_video_result(video_input(None, None))
        .unwrap();
    assert_eq!(result.outcome, "unavailable");
    assert!(!result.provider_available);
    assert!(!result.remote_publish_attempted);
    assert!(!result.persisted);
    assert!(result.job.is_none());
    assert_eq!(
        serde_json::to_value(result).unwrap()["providers"]["dy"],
        "unavailable"
    );
}

#[test]
fn immediate_video_dispatches_through_the_configured_provider_registry() {
    let (service, calls) = service_with_queued_provider();
    let result = service
        .publish_video_result(video_input(None, None))
        .unwrap();
    assert_eq!(result.outcome, "queued");
    assert!(result.provider_available);
    assert!(result.remote_publish_attempted);
    assert!(!result.persisted);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn scheduled_and_draft_videos_persist_without_dispatching() {
    let (service, calls) = service_with_queued_provider();
    let scheduled = service
        .publish_video_result(video_input(None, Some("2026-08-01 10:20")))
        .unwrap();
    let draft = service
        .publish_video_result(video_input(Some(true), None))
        .unwrap();
    assert_eq!(scheduled.outcome, "scheduled_locally");
    assert!(!scheduled.remote_publish_attempted);
    assert!(scheduled.persisted);
    assert_eq!(draft.outcome, "draft_locally");
    assert!(!draft.remote_publish_attempted);
    assert!(draft.persisted);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn video_dispatch_projection_is_reason_free_and_truthful() {
    let queued = video_dispatch_result(ProviderDispatchReport {
        outcomes: BTreeMap::from([(
            Platform::Douyin,
            DispatchOutcome::Queued {
                job_id: "local-job".into(),
            },
        )]),
    });
    assert_eq!(queued.outcome, "queued");
    assert!(queued.provider_available);
    assert!(queued.remote_publish_attempted);
    assert!(!queued.persisted);

    let mixed = video_dispatch_result(ProviderDispatchReport {
        outcomes: BTreeMap::from([
            (
                Platform::Douyin,
                DispatchOutcome::Rejected {
                    reason: "http://127.0.0.1:39001 private failure".into(),
                },
            ),
            (
                Platform::Bilibili,
                DispatchOutcome::Unavailable {
                    reason: "tcp:127.0.0.1:39002 private failure".into(),
                },
            ),
        ]),
    });
    let serialized = serde_json::to_string(&mixed).unwrap();
    assert_eq!(mixed.outcome, "rejected");
    assert!(mixed.remote_publish_attempted);
    assert!(!serialized.contains("127.0.0.1"));
    assert!(!serialized.contains("private failure"));
}

#[test]
fn video_dispatch_projection_marks_only_all_unavailable_as_not_attempted() {
    let result = video_dispatch_result(ProviderDispatchReport {
        outcomes: BTreeMap::from([(
            Platform::Douyin,
            DispatchOutcome::Unavailable {
                reason: "runner unavailable".into(),
            },
        )]),
    });
    assert_eq!(result.outcome, "unavailable");
    assert!(!result.provider_available);
    assert!(!result.remote_publish_attempted);
}

#[test]
fn state_path_flag_overrides_environment_path() {
    let path = state_path(
        ["--state-path".to_owned(), "flag.db".to_owned()],
        Some(OsStr::new("environment.db")),
    )
    .unwrap();
    assert_eq!(path, PathBuf::from("flag.db"));
}

#[test]
fn mcp_arguments_accept_state_path_repeatable_provider_runners_and_one_article_runner() {
    let config = mcp_config(
        [
            "--state-path".to_owned(),
            "state.db".to_owned(),
            "--provider-runner=dy=tcp:127.0.0.1:39001".to_owned(),
            "--provider-runner".to_owned(),
            "blbl=tcp:127.0.0.1:39003".to_owned(),
            "--article-runner=tcp:127.0.0.1:39002".to_owned(),
        ],
        None,
    )
    .unwrap();
    assert_eq!(config.state_path, PathBuf::from("state.db"));
    assert_eq!(
        config.article_runner.unwrap().address.to_string(),
        "127.0.0.1:39002"
    );
    assert!(matches!(
        config.provider_registry.availability(Platform::Douyin),
        matrixpost_core::ProviderAvailability::Available
    ));
    assert!(mcp_config(["--provider-runner=tcp:127.0.0.1:39001".into()], None).is_err());
    assert!(mcp_config(["--provider-runner=dy=tcp:192.0.2.1:39001".into()], None).is_err());
    assert!(mcp_config(["--provider-runner=dy=unix:/tmp/runner.sock".into()], None).is_err());
    assert!(
        mcp_config(
            [r"--provider-runner=dy=pipe:\\.\pipe\matrixpost-dy".into()],
            None
        )
        .is_err()
    );
    assert!(mcp_config(["--article-runner=tcp:192.0.2.1:39002".into()], None).is_err());
    assert!(
        mcp_config(
            [
                "--provider-runner=dy=tcp:127.0.0.1:39001".into(),
                "--provider-runner=dy=tcp:127.0.0.1:39002".into(),
            ],
            None
        )
        .is_err()
    );
    assert!(
        mcp_config(
            [
                "--article-runner=tcp:127.0.0.1:39002".into(),
                "--article-runner=tcp:127.0.0.1:39003".into(),
            ],
            None,
        )
        .is_err()
    );
}

#[test]
fn article_service_reports_default_unavailable_and_queued_runner_truthfully() {
    let unavailable = service()
        .publish_article_result(PublishArticleInput {
            platform: ArticlePlatformInput::Juejin,
            phone: "13800138000".into(),
            title: "Title".into(),
            content: Some("Body".into()),
            file: None,
            cover: None,
            category: None,
            tags: None,
            summary: None,
            publish_at: None,
            show: None,
        })
        .unwrap();
    assert_eq!(unavailable.outcome, "unavailable");
    assert!(!unavailable.provider_available);
    assert!(!unavailable.remote_publish_attempted);

    let queued = article_dispatch_result(ArticleDispatchOutcome::Queued {
        job_id: "mock-article-job".into(),
    });
    assert_eq!(queued.outcome, "queued");
    assert!(queued.provider_available);
    assert!(queued.remote_publish_attempted);

    let preflight_rejection = article_dispatch_result(ArticleDispatchOutcome::Rejected {
        reason: "unsupported schedule".into(),
        automation_attempted: false,
    });
    assert_eq!(preflight_rejection.outcome, "rejected");
    assert!(!preflight_rejection.provider_available);
    assert!(!preflight_rejection.remote_publish_attempted);

    let attempted_rejection = article_dispatch_result(ArticleDispatchOutcome::Rejected {
        reason: "mock automation failure".into(),
        automation_attempted: true,
    });
    assert_eq!(attempted_rejection.outcome, "rejected");
    assert!(!attempted_rejection.provider_available);
    assert!(attempted_rejection.remote_publish_attempted);
}

#[test]
fn fanqie_review_tool_is_reason_free_and_unavailable_without_runner() {
    let result = service().review_fanqie_status_result(ReviewFanqieStatusInput {
        title: "Title".into(),
    });
    assert_eq!(result.outcome, "unavailable");
    assert_eq!(result.platform, "fqsp");
    assert!(!result.message.contains("Title"));
}

#[test]
fn stderr_logging_is_opt_in() {
    assert!(!logging_enabled(None));
    assert!(logging_enabled(Some(OsStr::new("1"))));
}

#[test]
fn macro_generated_router_preserves_upstream_tools_and_exposes_closed_lifecycle_schemas() {
    let router = MatrixpostMcp::tool_router();
    let tools = router.list_all();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "add_business_relation",
            "add_content_attribution",
            "append_ledger_entry",
            "create_business_object",
            "get_business_object",
            "list_accounts",
            "list_article_history",
            "list_business_objects",
            "list_business_relations",
            "list_content_attributions",
            "list_history",
            "list_ledger_entries",
            "publish_article",
            "publish_video",
            "review_fanqie_status",
            "transition_business_object",
        ]
    );
    let publish_video = router.get("publish_video").unwrap();
    let schema = serde_json::to_value(&publish_video.input_schema).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["$defs"]["VideoPlatform"]["enum"],
        serde_json::json!(["dy", "ks", "blbl", "bjh", "tt", "sph"])
    );
    assert!(schema["$defs"]["SphLinkInput"]["oneOf"].is_array());
    assert!(
        schema["$defs"]["SphLinkInput"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .all(|variant| variant["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|field| field == "type")))
    );
    assert!(
        schema["$defs"]["SphLinkInput"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|variant| variant["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|field| field == "value")))
    );
    let publish_article = router.get("publish_article").unwrap();
    let article_schema = serde_json::to_value(&publish_article.input_schema).unwrap();
    assert_eq!(article_schema["additionalProperties"], false);
    assert_eq!(
        article_schema["properties"]["tags"]["type"],
        serde_json::json!(["string", "null"])
    );
    let list_accounts = router.get("list_accounts").unwrap();
    let accounts_schema = serde_json::to_value(&list_accounts.input_schema).unwrap();
    assert_eq!(
        accounts_schema["$defs"]["AccountsPlatform"]["enum"],
        serde_json::json!([
            "dy", "ks", "blbl", "bjh", "tt", "sph", "xhs", "juejin", "fqsp"
        ])
    );
    let list_history = router.get("list_history").unwrap();
    let history_schema = serde_json::to_value(&list_history.input_schema).unwrap();
    assert_eq!(
        history_schema["$defs"]["HistoryPlatform"]["enum"],
        serde_json::json!(["dy", "ks", "blbl", "bjh", "tt", "sph", "xhs"])
    );
    assert_eq!(
        history_schema["$defs"]["HistoryStatusInput"]["enum"],
        serde_json::json!(["success", "failed", "publishing", "scheduled"])
    );
    let create_object = router.get("create_business_object").unwrap();
    let create_schema = serde_json::to_value(&create_object.input_schema).unwrap();
    assert_eq!(create_schema["additionalProperties"], false);
    assert_eq!(
        create_schema["properties"]["displayName"]["type"],
        serde_json::json!("string")
    );
    let list_objects = router.get("list_business_objects").unwrap();
    let list_schema = serde_json::to_value(&list_objects.input_schema).unwrap();
    assert_eq!(list_schema["additionalProperties"], false);
    for name in [
        "get_business_object",
        "create_business_object",
        "list_ledger_entries",
        "append_ledger_entry",
        "list_content_attributions",
        "add_content_attribution",
        "list_business_relations",
        "add_business_relation",
        "transition_business_object",
    ] {
        let schema = serde_json::to_value(&router.get(name).unwrap().input_schema).unwrap();
        assert_eq!(schema["additionalProperties"], false, "{name}");
    }
}
