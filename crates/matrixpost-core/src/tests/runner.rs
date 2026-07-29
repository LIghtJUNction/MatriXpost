    #[test]
    fn article_request_preserves_all_fields() {
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("p".into()),
                partition: Some("q".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: Some("cover".into()),
            category: Some("rust".into()),
            tags: vec!["tag".into()],
            summary: Some("summary".into()),
            scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn runner_safe_requests_omit_all_account_routing_from_serialized_payloads() {
        let mut video = request();
        video.overrides[0].account = Some(AccountSelection {
            phone: Some("override-phone".into()),
            partition: Some("persist:override".into()),
        });
        let serialized = serde_json::to_string(&ProviderRunnerRequest {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            request: video.runner_safe(),
        })
        .unwrap();
        assert!(!serialized.contains("masked"));
        assert!(!serialized.contains("override-phone"));
        assert!(!serialized.contains("partition"));
        let article = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("article-phone".into()),
                partition: Some("persist:article".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        let serialized = serde_json::to_string(&ArticleRunnerRequest {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            request: article.runner_safe(),
        })
        .unwrap();
        assert!(!serialized.contains("article-phone"));
        assert!(!serialized.contains("partition"));
    }

    #[test]
    fn tcp_runner_provider_serializes_only_runner_safe_routing_data() {
        let mut routed = request();
        routed.account.phone = Some("top-level-phone".into());
        routed.account.partition = Some("persist:top-level".into());
        routed.overrides[0].account = Some(AccountSelection {
            phone: Some("override-phone".into()),
            partition: Some("persist:override".into()),
        });
        let provider = TcpRunnerProvider {
            platform: Platform::Douyin,
            address: "127.0.0.1:39001".parse().unwrap(),
        };
        let transport = CapturingRunnerTransport(Mutex::new(None));
        assert_eq!(
            provider.enqueue_with(&routed, &transport).unwrap(),
            DispatchOutcome::Queued {
                job_id: "safe-job".into(),
            }
        );
        let (endpoint, payload) = transport.0.lock().unwrap().take().unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:39001/v1/publish");
        assert!(!payload.contains("top-level-phone"));
        assert!(!payload.contains("override-phone"));
        assert!(!payload.contains("partition"));
        assert!(!payload.contains("phone"));
    }

    #[test]
    fn article_runner_protocol_rejects_unknown_fields_and_non_juejin_requests() {
        assert!(serde_json::from_str::<ArticleRunnerRequest>(
            r#"{"version":1,"request":{"platform":"juejin","account":{},"title":"T","content":"body","file":null,"cover":null,"category":null,"tags":[],"summary":null,"scheduled_at":null},"token":"forbidden"}"#,
        )
        .is_err());
        assert!(serde_json::from_str::<ArticleRunnerRequest>(
            r#"{"version":1,"request":{"platform":"juejin","account":{"session":"forbidden"},"title":"T","content":"body","file":null,"cover":null,"category":null,"tags":[],"summary":null,"scheduled_at":null}}"#,
        )
        .is_err());
        let request = PublishArticleRequest {
            platform: "dy".into(),
            account: Default::default(),
            title: "T".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        assert_eq!(
            request.validate(),
            Err(DomainError::UnknownPlatform("dy".into()))
        );
    }

    #[test]
    fn article_runner_response_requires_matching_version_platform_and_job() {
        let accepted = ArticleRunnerResponse::Queued {
            version: ARTICLE_RUNNER_PROTOCOL_VERSION,
            platform: ArticlePlatform::Juejin,
            job_id: "article-1".into(),
            automation_attempted: true,
        };
        assert_eq!(
            accepted.into_dispatch(ArticlePlatform::Juejin),
            Some(ArticleDispatchOutcome::Queued {
                job_id: "article-1".into(),
            })
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION + 1,
                platform: ArticlePlatform::Juejin,
                job_id: "article-1".into(),
                automation_attempted: true,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
        assert!(
            serde_json::from_str::<ArticleRunnerResponse>(
                r#"{"outcome":"queued","version":1,"platform":"juejin","job_id":"article-1"}"#,
            )
            .is_err()
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id: "article-1".into(),
                automation_attempted: false,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
        assert!(
            ArticleRunnerResponse::Queued {
                version: ARTICLE_RUNNER_PROTOCOL_VERSION,
                platform: ArticlePlatform::Juejin,
                job_id: " ".into(),
                automation_attempted: true,
            }
            .into_dispatch(ArticlePlatform::Juejin)
            .is_none()
        );
    }

    #[test]
    fn article_runner_adapter_strips_routing_and_uses_only_the_article_endpoint() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("article-phone".into()),
                partition: Some("persist:article".into()),
            },
            title: "title".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (
                200,
                r#"{"outcome":"queued","version":1,"platform":"juejin","job_id":"article-job","automation_attempted":true}"#
                    .into(),
            ),
        };
        assert_eq!(
            runner.dispatch_with(&request, &transport).unwrap(),
            ArticleDispatchOutcome::Queued {
                job_id: "article-job".into(),
            }
        );
        let (endpoint, payload) = transport.captured.lock().unwrap().take().unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:39002/v1/publish-article");
        assert!(!payload.contains("article-phone"));
        assert!(!payload.contains("persist:article"));
        assert!(!payload.contains("partition"));
    }

    #[test]
    fn article_runner_adapter_rejects_unsafe_endpoints_and_malformed_responses() {
        assert_eq!(
            ArticleRunner::parse_cli("tcp:192.0.2.1:39002"),
            Err(ArticleRunnerConfigError::TcpMustBeLoopback)
        );
        assert_eq!(
            ArticleRunner::parse_cli("tcp:127.0.0.1:39002?token=forbidden"),
            Err(ArticleRunnerConfigError::CredentialLikeEndpoint)
        );
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (200, "not json".into()),
        };
        assert!(matches!(
            runner
                .dispatch_with(
                    &PublishArticleRequest {
                        platform: "juejin".into(),
                        account: Default::default(),
                        title: "title".into(),
                        content: Some("body".into()),
                        file: None,
                        cover: None,
                        category: None,
                        tags: Vec::new(),
                        summary: None,
                        scheduled_at: None,
                    },
                    &transport,
                )
                .unwrap(),
            ArticleDispatchOutcome::Rejected { .. }
        ));
    }

    #[test]
    fn article_runner_adapter_marks_transport_failure_as_attempted() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let outcome = runner
            .dispatch_with(
                &PublishArticleRequest {
                    platform: "juejin".into(),
                    account: Default::default(),
                    title: "title".into(),
                    content: Some("body".into()),
                    file: None,
                    cover: None,
                    category: None,
                    tags: Vec::new(),
                    summary: None,
                    scheduled_at: None,
                },
                &FailingArticleRunnerTransport,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            ArticleDispatchOutcome::Rejected {
                automation_attempted: true,
                ..
            }
        ));
    }

    #[test]
    fn article_runner_adapter_rejects_schedules_without_transport_dispatch() {
        let runner = ArticleRunner::parse_cli("tcp:127.0.0.1:39002").unwrap();
        let transport = CapturingArticleRunnerTransport {
            captured: Mutex::new(None),
            response: (200, String::new()),
        };
        let outcome = runner
            .dispatch_with(
                &PublishArticleRequest {
                    platform: "juejin".into(),
                    account: Default::default(),
                    title: "title".into(),
                    content: Some("body".into()),
                    file: None,
                    cover: None,
                    category: None,
                    tags: Vec::new(),
                    summary: None,
                    scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
                },
                &transport,
            )
            .unwrap();
        assert!(matches!(outcome, ArticleDispatchOutcome::Rejected { .. }));
        assert!(transport.captured.lock().unwrap().is_none());
    }
    #[test]
    fn stager_rejects_missing_content_type_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "missing-content-type",
            staging_policy(3),
            TestTransport::response(None, Some("3"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_disallowed_content_type_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "disallowed-content-type",
            staging_policy(3),
            TestTransport::response(Some("text/plain"), Some("3"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_invalid_content_length_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "invalid-length",
            staging_policy(3),
            TestTransport::response(
                Some("video/mp4"),
                Some("not-a-number"),
                Cursor::new(b"abc".to_vec()),
            ),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_rejects_declared_too_large_content_length_without_creating_output() {
        assert_staging_error_leaves_no_file(
            "declared-too-large",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), Some("4"), Cursor::new(b"abc".to_vec())),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_removes_created_file_when_stream_exceeds_limit() {
        let filesystem = TestFilesystem::file();
        assert_staging_error_leaves_no_file(
            "stream-too-large",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abcd".to_vec())),
            &filesystem,
        );
        assert_eq!(filesystem.created_count(), 1);
    }

    #[test]
    fn stager_removes_created_file_when_reader_fails() {
        assert_staging_error_leaves_no_file(
            "read-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, FailingReader),
            &TestFilesystem::file(),
        );
    }

    #[test]
    fn stager_removes_created_file_when_writer_fails() {
        assert_staging_error_leaves_no_file(
            "write-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abc".to_vec())),
            &TestFilesystem::failing(TestOutput::FailWrite),
        );
    }

    #[test]
    fn stager_removes_created_file_when_flush_fails() {
        assert_staging_error_leaves_no_file(
            "flush-failure",
            staging_policy(3),
            TestTransport::response(Some("video/mp4"), None, Cursor::new(b"abc".to_vec())),
            &TestFilesystem::failing(TestOutput::FailFlush),
        );
    }

    #[test]
    fn stager_retries_name_collisions_without_overwriting_existing_file() {
        let directory = staging_directory("collision");
        fs::create_dir_all(&directory).unwrap();
        let existing = directory.join("taken");
        fs::write(&existing, b"preserved").unwrap();
        let policy = staging_policy(3);
        let stager = HttpRemoteMediaStager::new(directory.clone());
        let transport =
            TestTransport::response(Some("video/mp4"), Some("3"), Cursor::new(b"abc".to_vec()));
        let mut names = TestNames(VecDeque::from(["taken".into(), "fresh".into()]));
        let staged = stager
            .stage_with(
                &staging_request(&policy),
                &policy,
                &transport,
                &TestFilesystem::file(),
                &mut names,
            )
            .unwrap();
        assert_eq!(fs::read(&existing).unwrap(), b"preserved");
        assert_eq!(staged.path(), directory.join("fresh"));
        assert_eq!(fs::read(staged.path()).unwrap(), b"abc");
        staged.cleanup().unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn configured_tcp_runner_installs_the_versioned_execution_adapter() {
        let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        let registry = ProviderRegistry::from_runners([runner]).unwrap();
        assert_eq!(
            registry.availability(Platform::Douyin),
            ProviderAvailability::Available,
        );
    }

    #[test]
    fn runner_response_requires_matching_version_and_platform() {
        let accepted = ProviderRunnerResponse::Queued {
            version: PROVIDER_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
            job_id: "job".into(),
        };
        assert_eq!(
            accepted.into_dispatch(Platform::Douyin),
            Some(DispatchOutcome::Queued {
                job_id: "job".into()
            })
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION + 1,
                platform: Platform::Douyin,
                job_id: "job".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Douyin,
                job_id: "   ".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
        assert!(
            serde_json::from_str::<ProviderRunnerResponse>(
                r#"{"outcome":"queued","version":1,"platform":"dy","job_id":"job","extra":true}"#,
            )
            .is_err()
        );
        assert!(
            ProviderRunnerResponse::Queued {
                version: PROVIDER_RUNNER_PROTOCOL_VERSION,
                platform: Platform::Kuaishou,
                job_id: "job".into(),
            }
            .into_dispatch(Platform::Douyin)
            .is_none()
        );
    }

    #[test]
    fn manual_login_protocol_dtos_are_versioned_and_reject_unknown_fields() {
        let request = LoginRunnerRequest {
            version: LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"version":1,"platform":"dy"}"#
        );
        assert!(
            serde_json::from_str::<LoginRunnerRequest>(
                r#"{"version":1,"platform":"dy","cookie":"forbidden"}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<LoginRunnerResponse>(
                r#"{"outcome":"opened","version":1,"platform":"dy","manual_login_required":true,"extra":true}"#,
            )
            .is_err()
        );
    }

    #[test]
    fn account_status_protocol_is_strict_and_loopback_only() {
        let request = AccountStatusRunnerRequest {
            version: ACCOUNT_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::Douyin,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"version":1,"platform":"dy"}"#
        );
        assert!(
            serde_json::from_str::<AccountStatusRunnerRequest>(
                r#"{"version":1,"platform":"dy","cookie":"no"}"#
            )
            .is_err()
        );
        let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        let transport = CapturingManualLoginTransport {
            captured: Mutex::new(None),
            response: Ok((
                200,
                r#"{"outcome":"ready","version":1,"platform":"dy"}"#.into(),
            )),
        };
        assert_eq!(
            runner.account_readiness_with(&transport).unwrap(),
            AccountReadiness::Ready
        );
        assert_eq!(
            transport.captured.lock().unwrap().as_ref().unwrap().0,
            "http://127.0.0.1:39001/v1/account-status"
        );
        let remote = ProviderRunner {
            platform: Platform::Douyin,
            transport: ProviderRunnerTransport::Tcp {
                address: "192.0.2.1:39001".parse().unwrap(),
            },
        };
        assert_eq!(
            remote.account_readiness_with(&transport).unwrap(),
            AccountReadiness::Unavailable
        );
    }

    #[test]
    fn fanqie_review_status_protocol_is_strict_bounded_and_local_only() {
        let request = ReviewStatusRunnerRequest {
            version: REVIEW_STATUS_RUNNER_PROTOCOL_VERSION,
            platform: Platform::FanqieVideo,
            title_query: " title ".into(),
        };
        assert!(request.validate());
        assert!(
            serde_json::from_str::<ReviewStatusRunnerRequest>(
                r#"{"version":1,"platform":"fqsp","title_query":"title","cookie":"no"}"#
            )
            .is_err()
        );
        let runner = ProviderRunner::parse_cli("fqsp=tcp:127.0.0.1:39001").unwrap();
        let transport = CapturingManualLoginTransport {
            captured: Mutex::new(None),
            response: Ok((
                200,
                r#"{"outcome":"under_review","version":1,"platform":"fqsp"}"#.into(),
            )),
        };
        assert_eq!(
            runner
                .fanqie_review_status_with(" title ", &transport)
                .unwrap(),
            ReviewStatus::UnderReview
        );
        let captured = transport.captured.lock().unwrap();
        assert_eq!(
            captured.as_ref().unwrap().0,
            "http://127.0.0.1:39001/v1/review-status"
        );
        assert!(
            captured
                .as_ref()
                .unwrap()
                .1
                .contains(r#""title_query":"title""#)
        );
        let douyin = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        assert_eq!(
            douyin
                .fanqie_review_status_with("title", &transport)
                .unwrap(),
            ReviewStatus::Rejected
        );
        assert_eq!(
            runner
                .fanqie_review_status_with(
                    &"x".repeat(REVIEW_STATUS_TITLE_QUERY_MAX_BYTES + 1),
                    &transport
                )
                .unwrap(),
            ReviewStatus::Rejected
        );
    }

    #[test]
    fn manual_login_uses_only_the_validated_loopback_endpoint_and_safe_payload() {
        let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        let transport = CapturingManualLoginTransport {
            captured: Mutex::new(None),
            response: Ok((
                200,
                r#"{"outcome":"opened","version":1,"platform":"dy","manual_login_required":true}"#
                    .into(),
            )),
        };

        assert_eq!(
            runner.request_manual_login_with(&transport).unwrap(),
            ManualLoginOutcome::Opened
        );
        let (endpoint, body) = transport.captured.lock().unwrap().clone().unwrap();
        assert_eq!(endpoint, "http://127.0.0.1:39001/v1/login");
        assert_eq!(body, r#"{"version":1,"platform":"dy"}"#);
    }

    #[test]
    fn manual_login_rejects_malformed_and_mismatched_runner_responses() {
        let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        for response in [
            Ok((200, "not-json".into())),
            Ok((
                200,
                r#"{"outcome":"opened","version":1,"platform":"ks","manual_login_required":true}"#
                    .into(),
            )),
            Ok((
                200,
                r#"{"outcome":"opened","version":1,"platform":"dy","manual_login_required":false}"#
                    .into(),
            )),
            Ok((503, "service unavailable".into())),
            Err(ManualLoginTransportError::RequestFailed),
        ] {
            let transport = CapturingManualLoginTransport {
                captured: Mutex::new(None),
                response,
            };
            assert_eq!(
                runner.request_manual_login_with(&transport).unwrap(),
                ManualLoginOutcome::Rejected
            );
        }
    }

    #[test]
    fn manual_login_does_not_invoke_a_direct_nonloopback_runner() {
        let runner = ProviderRunner {
            platform: Platform::Douyin,
            transport: ProviderRunnerTransport::Tcp {
                address: "192.0.2.1:39001".parse().unwrap(),
            },
        };
        let transport = CapturingManualLoginTransport {
            captured: Mutex::new(None),
            response: Ok((200, String::new())),
        };
        assert_eq!(
            runner.request_manual_login_with(&transport).unwrap(),
            ManualLoginOutcome::Unavailable
        );
        assert!(transport.captured.lock().unwrap().is_none());
    }

    #[test]
    fn provider_runner_exposes_only_validated_loopback_tcp_addresses() {
        let tcp = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
        assert_eq!(
            tcp.loopback_tcp_address(),
            Some("127.0.0.1:39001".parse().unwrap())
        );
        let socket = ProviderRunner::parse_cli("dy=unix:/tmp/matrixpost.sock").unwrap();
        assert_eq!(socket.loopback_tcp_address(), None);
        let direct_remote = ProviderRunner {
            platform: Platform::Douyin,
            transport: ProviderRunnerTransport::Tcp {
                address: "192.0.2.1:39001".parse().unwrap(),
            },
        };
        assert_eq!(direct_remote.loopback_tcp_address(), None);
    }

    #[test]
    fn runner_configuration_rejects_nonlocal_duplicate_and_credential_like_endpoints() {
        assert_eq!(
            ProviderRunner::parse_cli("dy=tcp:192.0.2.1:39001"),
            Err(ProviderRunnerConfigError::TcpMustBeLoopback)
        );
        assert_eq!(
            ProviderRunner::parse_cli("dy=unix:/run/matrixpost/token.sock"),
            Err(ProviderRunnerConfigError::CredentialLikeEndpoint)
        );
        let runner = ProviderRunner::parse_cli("dy=unix:/run/matrixpost/dy.sock").unwrap();
        assert!(matches!(
            ProviderRegistry::from_runners([runner.clone(), runner]),
            Err(ProviderRunnerConfigError::DuplicatePlatform {
                platform: Platform::Douyin,
            })
        ));
    }
