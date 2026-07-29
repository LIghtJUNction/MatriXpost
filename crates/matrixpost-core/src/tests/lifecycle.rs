    #[test]
    fn sqlite_persists_safe_article_accounts_without_session_material() {
        let repository = SqliteRepository::in_memory().unwrap();
        let account = ArticleAccount {
            id: "juejin-primary".into(),
            platform: ArticlePlatform::Juejin,
            display_name: "Primary".into(),
            status: ArticleAccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:juejin".into(),
        };
        repository.save_article_account(&account).unwrap();
        assert_eq!(repository.article_accounts().unwrap(), vec![account]);
    }

    fn business_object(id: &str, kind: &str, external_id: Option<&str>) -> BusinessObject {
        let now = Utc::now();
        BusinessObject {
            id: id.into(),
            kind: kind.into(),
            external_id: external_id.map(str::to_owned),
            display_name: "Example object".into(),
            lifecycle_status: BusinessObjectStatus::Active,
            approval_status: ApprovalStatus::Approved,
            revision: 0,
            attributes: BTreeMap::from([("source".into(), "manual".into())]),
            created_at: now,
            updated_at: now,
        }
    }

    fn approved_ledger_entry(id: &str, business_object_id: &str) -> LedgerEntry {
        LedgerEntry {
            id: id.into(),
            business_object_id: business_object_id.into(),
            direction: LedgerDirection::Expense,
            category: "service".into(),
            amount_minor: 35_000,
            currency: "CNY".into(),
            occurred_at: Utc::now(),
            approval_status: ApprovalStatus::Approved,
            counterparty: Some("Example supplier".into()),
            reference: Some("receipt-1".into()),
            description: Some("Approved service cost".into()),
            created_at: Utc::now(),
        }
    }

    fn business_relation(
        id: &str,
        source_business_object_id: &str,
        target_business_object_id: &str,
        relation_type: &str,
    ) -> BusinessRelation {
        BusinessRelation {
            id: id.into(),
            source_business_object_id: source_business_object_id.into(),
            target_business_object_id: target_business_object_id.into(),
            relation_type: relation_type.into(),
            attributes: BTreeMap::from([("source".into(), "manual".into())]),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn lifecycle_repository_round_trips_an_object_ledger_and_content_attribution() {
        let repository = SqliteRepository::in_memory().unwrap();
        let object = business_object("object-1", "asset", Some("external-1"));
        let ledger_entry = approved_ledger_entry("ledger-1", &object.id);
        let history = HistoryRecord {
            id: "history-lifecycle-1".into(),
            request: request(),
            state: PublishState::Published,
            recorded_at: Utc::now(),
            detail: None,
        };
        let attribution = ContentAttribution {
            business_object_id: object.id.clone(),
            history_id: history.id.clone(),
            created_at: Utc::now(),
        };

        repository.insert_business_object(&object).unwrap();
        repository.insert_ledger_entry(&ledger_entry).unwrap();
        repository.append_history(&history).unwrap();
        repository.insert_content_attribution(&attribution).unwrap();

        assert_eq!(
            repository.business_object(&object.id).unwrap(),
            Some(object)
        );
        assert_eq!(
            repository.ledger_entries("object-1").unwrap(),
            vec![ledger_entry]
        );
        assert_eq!(
            repository.content_attributions("object-1").unwrap(),
            vec![attribution]
        );
    }

    #[test]
    fn lifecycle_repository_rejects_duplicate_external_ids_within_the_same_kind() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("object-1", "asset", Some("same")))
            .unwrap();

        assert_eq!(
            repository.insert_business_object(&business_object("object-2", "asset", Some("same"))),
            Err(DomainError::DuplicateBusinessObjectExternalId {
                kind: "asset".into(),
                external_id: "same".into(),
            })
        );
    }

    #[test]
    fn lifecycle_repository_rejects_sensitive_attribute_keys_case_insensitively() {
        let repository = SqliteRepository::in_memory().unwrap();

        for key in [
            "token",
            "COOKIE",
            "Api_Secret",
            "sessionId",
            "authorization",
        ] {
            let mut object = business_object(&format!("object-{key}"), "asset", None);
            object.attributes = BTreeMap::from([(key.into(), "value".into())]);

            assert_eq!(
                repository.insert_business_object(&object),
                Err(DomainError::SensitiveBusinessObjectAttributeKey(key.into()))
            );
            assert_eq!(repository.business_object(&object.id).unwrap(), None);
        }
    }

    #[test]
    fn lifecycle_repository_does_not_content_scan_non_sensitive_attribute_values() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-generic-text", "campaign", None);
        object.attributes = BTreeMap::from([(
            "notes".into(),
            "Explain the token and cookie terms in the customer onboarding guide.".into(),
        )]);

        repository.insert_business_object(&object).unwrap();

        assert_eq!(
            repository.business_object(&object.id).unwrap(),
            Some(object)
        );
    }

    #[test]
    fn lifecycle_repository_accepts_non_sensitive_generic_attributes() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-safe-attributes", "campaign", None);
        object.attributes = BTreeMap::from([
            ("customer_segment".into(), "returning".into()),
            ("content_topic".into(), "summer promotion".into()),
        ]);

        repository.insert_business_object(&object).unwrap();

        assert_eq!(
            repository.business_object(&object.id).unwrap(),
            Some(object)
        );
    }

    #[test]
    fn lifecycle_repository_requires_new_objects_to_start_at_revision_zero() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-invalid-revision", "project", None);
        object.revision = 1;

        assert_eq!(
            repository.insert_business_object(&object),
            Err(DomainError::InvalidInitialBusinessObjectRevision(1))
        );
    }

    #[test]
    fn lifecycle_transition_updates_status_timestamp_and_revision() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-transition-1", "project", None);
        object.lifecycle_status = BusinessObjectStatus::Draft;
        object.approval_status = ApprovalStatus::Pending;
        repository.insert_business_object(&object).unwrap();
        let updated_at = object.updated_at + ChronoDuration::minutes(1);

        let transitioned = repository
            .transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Active,
                ApprovalStatus::Pending,
                updated_at,
            )
            .unwrap();

        assert_eq!(transitioned.lifecycle_status, BusinessObjectStatus::Active);
        assert_eq!(transitioned.approval_status, ApprovalStatus::Pending);
        assert_eq!(transitioned.revision, 1);
        assert_eq!(transitioned.updated_at, updated_at);
        assert_eq!(
            repository.business_object(&object.id).unwrap(),
            Some(transitioned)
        );
    }

    #[test]
    fn lifecycle_transition_allows_approval_resubmission() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-transition-2", "project", None);
        object.approval_status = ApprovalStatus::Pending;
        repository.insert_business_object(&object).unwrap();
        let rejected_at = object.updated_at + ChronoDuration::minutes(1);
        let rejected = repository
            .transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Active,
                ApprovalStatus::Rejected,
                rejected_at,
            )
            .unwrap();
        let resubmitted_at = rejected_at + ChronoDuration::minutes(1);

        let resubmitted = repository
            .transition_business_object(
                &object.id,
                rejected.revision,
                BusinessObjectStatus::Active,
                ApprovalStatus::Pending,
                resubmitted_at,
            )
            .unwrap();

        assert_eq!(resubmitted.approval_status, ApprovalStatus::Pending);
        assert_eq!(resubmitted.revision, 2);
        assert_eq!(resubmitted.updated_at, resubmitted_at);
    }

    #[test]
    fn lifecycle_transition_rejects_terminal_and_noop_statuses() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-transition-3", "project", None);
        object.lifecycle_status = BusinessObjectStatus::Archived;
        object.approval_status = ApprovalStatus::Approved;
        repository.insert_business_object(&object).unwrap();

        assert_eq!(
            repository.transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Active,
                ApprovalStatus::Approved,
                Utc::now(),
            ),
            Err(DomainError::InvalidBusinessObjectLifecycleTransition {
                from: BusinessObjectStatus::Archived,
                to: BusinessObjectStatus::Active,
            })
        );
        assert_eq!(
            repository.transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Archived,
                ApprovalStatus::Pending,
                Utc::now(),
            ),
            Err(DomainError::InvalidBusinessObjectApprovalTransition {
                from: ApprovalStatus::Approved,
                to: ApprovalStatus::Pending,
            })
        );
        assert_eq!(
            repository.transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Archived,
                ApprovalStatus::Approved,
                Utc::now(),
            ),
            Err(DomainError::BusinessObjectTransitionNoop(
                "object-transition-3".into()
            ))
        );
    }

    #[test]
    fn lifecycle_transition_rejects_stale_revision() {
        let repository = SqliteRepository::in_memory().unwrap();
        let mut object = business_object("object-transition-4", "project", None);
        object.approval_status = ApprovalStatus::Pending;
        repository.insert_business_object(&object).unwrap();
        repository
            .transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Active,
                ApprovalStatus::Approved,
                Utc::now(),
            )
            .unwrap();

        assert_eq!(
            repository.transition_business_object(
                &object.id,
                0,
                BusinessObjectStatus::Completed,
                ApprovalStatus::Approved,
                Utc::now(),
            ),
            Err(DomainError::StaleBusinessObjectRevision {
                id: "object-transition-4".into(),
                expected: 0,
                actual: 1,
            })
        );
    }

    #[test]
    fn lifecycle_repository_rejects_ledger_entries_for_missing_objects() {
        let repository = SqliteRepository::in_memory().unwrap();

        assert_eq!(
            repository.insert_ledger_entry(&approved_ledger_entry("ledger-1", "missing")),
            Err(DomainError::UnknownBusinessObject("missing".into()))
        );
    }

    #[test]
    fn lifecycle_repository_distinguishes_missing_objects_from_empty_child_lists() {
        let repository = SqliteRepository::in_memory().unwrap();

        assert_eq!(
            repository.ledger_entries("missing-object"),
            Err(DomainError::UnknownBusinessObject("missing-object".into()))
        );
        assert_eq!(
            repository.content_attributions("missing-object"),
            Err(DomainError::UnknownBusinessObject("missing-object".into()))
        );

        repository
            .insert_business_object(&business_object("empty-object", "asset", None))
            .unwrap();
        assert!(
            repository
                .ledger_entries("empty-object")
                .unwrap()
                .is_empty()
        );
        assert!(
            repository
                .content_attributions("empty-object")
                .unwrap()
                .is_empty()
        );
        assert!(
            repository
                .business_relations("empty-object")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn lifecycle_repository_round_trips_directed_business_relations() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("asset-1", "asset", None))
            .unwrap();
        repository
            .insert_business_object(&business_object("customer-1", "customer", None))
            .unwrap();
        let relation =
            business_relation("relation-1", "asset-1", "customer-1", "customer_interest");

        repository.insert_business_relation(&relation).unwrap();

        assert_eq!(
            repository.business_relations("asset-1").unwrap(),
            vec![relation.clone()]
        );
        assert_eq!(
            repository.business_relations("customer-1").unwrap(),
            vec![relation]
        );
    }

    #[test]
    fn lifecycle_repository_rejects_invalid_or_duplicate_business_relations() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("source", "asset", None))
            .unwrap();
        repository
            .insert_business_object(&business_object("target", "customer", None))
            .unwrap();

        let mut empty_id = business_relation("relation", "source", "target", "owner");
        empty_id.id = " ".into();
        assert_eq!(
            repository.insert_business_relation(&empty_id),
            Err(DomainError::EmptyLifecycleField("business relation id"))
        );
        let mut empty_source = business_relation("empty-source", "source", "target", "owner");
        empty_source.source_business_object_id = " ".into();
        assert_eq!(
            repository.insert_business_relation(&empty_source),
            Err(DomainError::EmptyLifecycleField(
                "business relation source business object id"
            ))
        );
        let mut empty_target = business_relation("empty-target", "source", "target", "owner");
        empty_target.target_business_object_id = " ".into();
        assert_eq!(
            repository.insert_business_relation(&empty_target),
            Err(DomainError::EmptyLifecycleField(
                "business relation target business object id"
            ))
        );
        let mut empty_type = business_relation("empty-type", "source", "target", "owner");
        empty_type.relation_type = " ".into();
        assert_eq!(
            repository.insert_business_relation(&empty_type),
            Err(DomainError::EmptyLifecycleField("business relation type"))
        );

        assert_eq!(
            repository
                .insert_business_relation(&business_relation("self", "source", "source", "owner")),
            Err(DomainError::BusinessRelationSelfReference("source".into()))
        );
        assert_eq!(
            repository.insert_business_relation(&business_relation(
                "missing-source",
                "missing",
                "target",
                "owner"
            )),
            Err(DomainError::UnknownBusinessObject("missing".into()))
        );
        assert_eq!(
            repository.insert_business_relation(&business_relation(
                "missing-target",
                "source",
                "missing",
                "owner"
            )),
            Err(DomainError::UnknownBusinessObject("missing".into()))
        );

        let relation = business_relation("relation-1", "source", "target", "owner");
        repository.insert_business_relation(&relation).unwrap();
        assert_eq!(
            repository.insert_business_relation(&relation),
            Err(DomainError::DuplicateBusinessRelationId(
                "relation-1".into()
            ))
        );
        assert_eq!(
            repository.insert_business_relation(&business_relation(
                "relation-2",
                "source",
                "target",
                "owner"
            )),
            Err(DomainError::DuplicateBusinessRelation {
                source_business_object_id: "source".into(),
                target_business_object_id: "target".into(),
                relation_type: "owner".into(),
            })
        );
    }

    #[test]
    fn lifecycle_repository_rejects_invalid_and_sensitive_relation_attributes() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("source", "asset", None))
            .unwrap();
        repository
            .insert_business_object(&business_object("target", "customer", None))
            .unwrap();

        let mut invalid_key = business_relation("invalid-key", "source", "target", "owner");
        invalid_key.attributes = BTreeMap::from([("invalid key".into(), "value".into())]);
        assert_eq!(
            repository.insert_business_relation(&invalid_key),
            Err(DomainError::InvalidBusinessRelationAttributeKey(
                "invalid key".into()
            ))
        );

        let mut sensitive_key = business_relation("sensitive-key", "source", "target", "owner");
        sensitive_key.attributes = BTreeMap::from([("TOKEN".into(), "value".into())]);
        assert_eq!(
            repository.insert_business_relation(&sensitive_key),
            Err(DomainError::SensitiveBusinessRelationAttributeKey(
                "TOKEN".into()
            ))
        );

        let mut empty_value = business_relation("empty-value", "source", "target", "owner");
        empty_value.attributes = BTreeMap::from([("notes".into(), " ".into())]);
        assert_eq!(
            repository.insert_business_relation(&empty_value),
            Err(DomainError::InvalidBusinessRelationAttributeValue(
                "notes".into()
            ))
        );

        let mut generic_value = business_relation("generic-value", "source", "target", "reference");
        generic_value.attributes = BTreeMap::from([(
            "notes".into(),
            "Explain the token and cookie terms in the customer guide.".into(),
        )]);
        assert!(repository.insert_business_relation(&generic_value).is_ok());
    }

    #[test]
    fn lifecycle_repository_rejects_relations_for_missing_objects_when_listing() {
        let repository = SqliteRepository::in_memory().unwrap();

        assert_eq!(
            repository.business_relations("missing-object"),
            Err(DomainError::UnknownBusinessObject("missing-object".into()))
        );
    }

    #[test]
    fn lifecycle_repository_rejects_attributions_for_missing_history() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("object-1", "asset", None))
            .unwrap();
        let attribution = ContentAttribution {
            business_object_id: "object-1".into(),
            history_id: "missing-history".into(),
            created_at: Utc::now(),
        };

        assert_eq!(
            repository.insert_content_attribution(&attribution),
            Err(DomainError::UnknownHistoryRecord("missing-history".into()))
        );
    }

    #[test]
    fn lifecycle_schema_rejects_direct_attributions_for_missing_history() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("object-1", "asset", None))
            .unwrap();
        let connection = repository.locked().unwrap();

        let error = connection
            .execute(
                "INSERT INTO content_attributions(business_object_id, history_id, created_at) VALUES (?1, ?2, ?3)",
                params!["object-1", "missing-history", Utc::now().to_rfc3339()],
            )
            .unwrap_err();

        assert_eq!(error.to_string(), "FOREIGN KEY constraint failed");
    }

    #[test]
    fn lifecycle_repository_rejects_attributions_for_missing_objects() {
        let repository = SqliteRepository::in_memory().unwrap();
        let history = HistoryRecord {
            id: "history-lifecycle-1".into(),
            request: request(),
            state: PublishState::Published,
            recorded_at: Utc::now(),
            detail: None,
        };
        repository.append_history(&history).unwrap();
        let attribution = ContentAttribution {
            business_object_id: "missing-object".into(),
            history_id: history.id,
            created_at: Utc::now(),
        };

        assert_eq!(
            repository.insert_content_attribution(&attribution),
            Err(DomainError::UnknownBusinessObject("missing-object".into()))
        );
    }

    #[test]
    fn lifecycle_repository_allows_one_history_record_for_multiple_objects() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("object-1", "asset", None))
            .unwrap();
        repository
            .insert_business_object(&business_object("object-2", "asset", None))
            .unwrap();
        let history = HistoryRecord {
            id: "history-lifecycle-1".into(),
            request: request(),
            state: PublishState::Published,
            recorded_at: Utc::now(),
            detail: None,
        };
        repository.append_history(&history).unwrap();
        repository
            .insert_content_attribution(&ContentAttribution {
                business_object_id: "object-1".into(),
                history_id: history.id.clone(),
                created_at: Utc::now(),
            })
            .unwrap();

        assert!(
            repository
                .insert_content_attribution(&ContentAttribution {
                    business_object_id: "object-2".into(),
                    history_id: history.id,
                    created_at: Utc::now(),
                })
                .is_ok()
        );
    }

    #[test]
    fn lifecycle_repository_rejects_duplicate_content_attribution_pairs() {
        let repository = SqliteRepository::in_memory().unwrap();
        repository
            .insert_business_object(&business_object("object-1", "asset", None))
            .unwrap();
        let history = HistoryRecord {
            id: "history-lifecycle-1".into(),
            request: request(),
            state: PublishState::Published,
            recorded_at: Utc::now(),
            detail: None,
        };
        let attribution = ContentAttribution {
            business_object_id: "object-1".into(),
            history_id: history.id.clone(),
            created_at: Utc::now(),
        };
        repository.append_history(&history).unwrap();
        repository.insert_content_attribution(&attribution).unwrap();

        assert_eq!(
            repository.insert_content_attribution(&attribution),
            Err(DomainError::DuplicateContentAttribution {
                business_object_id: "object-1".into(),
                history_id: "history-lifecycle-1".into(),
            })
        );
    }

    #[test]
    fn lifecycle_migration_adds_tables_to_a_version_four_database() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY); INSERT INTO schema_migrations(version) VALUES (4);",
            )
            .unwrap();
        SqliteRepository::migrate(&connection).unwrap();

        let version_five: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=5)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(version_five);
        let ledger_entries_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ledger_entries')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ledger_entries_exists);
        let columns = connection
            .prepare("PRAGMA table_info(business_objects)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"revision".into()));
    }

    #[test]
    fn lifecycle_migration_adds_relations_to_a_version_five_database() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY); INSERT INTO schema_migrations(version) VALUES (2); INSERT INTO schema_migrations(version) VALUES (3); INSERT INTO schema_migrations(version) VALUES (4); CREATE TABLE accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL, phone TEXT NOT NULL DEFAULT '', partition TEXT NOT NULL DEFAULT ''); CREATE TABLE article_accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL, phone TEXT NOT NULL DEFAULT '', partition TEXT NOT NULL DEFAULT ''); CREATE TABLE history (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); CREATE TABLE jobs (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, due_at TEXT, revision INTEGER NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE config (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL); CREATE TABLE job_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); CREATE TABLE business_objects (id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL, external_id TEXT, display_name TEXT NOT NULL, lifecycle_status TEXT NOT NULL, approval_status TEXT NOT NULL, revision INTEGER NOT NULL, attributes_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE ledger_entries (id TEXT PRIMARY KEY NOT NULL, business_object_id TEXT NOT NULL REFERENCES business_objects(id), direction TEXT NOT NULL, category TEXT NOT NULL, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL, occurred_at TEXT NOT NULL, approval_status TEXT NOT NULL, counterparty TEXT, reference TEXT, description TEXT, created_at TEXT NOT NULL); CREATE TABLE content_attributions (business_object_id TEXT NOT NULL REFERENCES business_objects(id), history_id TEXT NOT NULL REFERENCES history(id), created_at TEXT NOT NULL, PRIMARY KEY(business_object_id, history_id)); INSERT INTO schema_migrations(version) VALUES (5);",
            )
            .unwrap();

        SqliteRepository::migrate(&connection).unwrap();

        let version_six: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=6)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(version_six);
        let relation_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='business_relations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(relation_table_exists);
    }

    #[test]
    fn migration_adds_safe_account_routes_to_a_version_three_database() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY); INSERT INTO schema_migrations(version) VALUES (3); CREATE TABLE accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE article_accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL);").unwrap();
        SqliteRepository::migrate(&connection).unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(accounts)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"phone".into()));
        assert!(columns.contains(&"partition".into()));
    }
    #[test]
    fn compatibility_dto_preserves_remote_url_and_per_target_account() {
        let dto: UpstreamPublishDto = serde_json::from_str(r#"{"platforms":[{"platform":"dy","phone":"one"}],"file":"https://example.invalid/video.mp4","title":"T","bt2":"legacy","tags":"one, two three","creativeStatements":{"抖音":"original"}}"#).unwrap();
        let request = PublishRequest::try_from(dto).unwrap();
        assert!(matches!(request.source, MediaSource::RemoteUrl(_)));
        assert_eq!(request.bt2.as_deref(), Some("legacy"));
        assert_eq!(
            request.overrides[0]
                .account
                .as_ref()
                .and_then(|item| item.phone.as_deref()),
            Some("one")
        );
        assert_eq!(request.tags, vec!["one", "two", "three"]);
    }
    #[test]
    fn compatibility_dto_accepts_single_platform_and_sph_option_map() {
        let dto: UpstreamPublishDto = serde_json::from_str(r#"{"platform":"sph","file":"movie.mp4","title":"T","tags":["a"],"sphProductId":"product","platformOptions":{"sph":{"link":{"type":"product","value":"value"}}}}"#).unwrap();
        let request = PublishRequest::try_from(dto).unwrap();
        assert_eq!(request.targets, vec![Platform::WechatChannels]);
        assert_eq!(request.wechat_link.product_id.as_deref(), Some("product"));
        assert_eq!(request.wechat_link.link_type.as_deref(), Some("product"));
        assert_eq!(request.wechat_link.link_value.as_deref(), Some("value"));
    }
