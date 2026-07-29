    #[test]
    fn provider_registry_rejects_duplicate_platforms_deterministically() {
        let mut registry = ProviderRegistry::new();
        let (first, _) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "first".into(),
            },
            None,
        );
        let (second, _) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "second".into(),
            },
            None,
        );
        registry.register(first).unwrap();
        assert_eq!(
            registry.register(second),
            Err(ProviderRegistrationError::Duplicate {
                platform: Platform::Douyin,
            })
        );
        assert_eq!(
            registry.dispatch(Platform::Douyin, &request()).unwrap(),
            DispatchOutcome::Queued {
                job_id: "first".into(),
            }
        );
    }

    #[test]
    fn provider_registry_requires_the_dispatch_target_to_be_requested() {
        assert_eq!(
            ProviderRegistry::new().dispatch(Platform::Bilibili, &request()),
            Err(DomainError::ProviderPlatformNotTarget {
                platform: Platform::Bilibili,
            })
        );
    }

    #[test]
    fn provider_registry_makes_missing_and_declared_unavailable_explicit() {
        let mut registry = ProviderRegistry::new();
        let (provider, calls) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Unavailable {
                reason: "browser login required".into(),
            },
            DispatchOutcome::Queued {
                job_id: "must-not-run".into(),
            },
            None,
        );
        registry.register(provider).unwrap();

        assert_eq!(
            registry.dispatch(Platform::Douyin, &request()).unwrap(),
            DispatchOutcome::Unavailable {
                reason: "browser login required".into(),
            }
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            registry.availability(Platform::Bilibili),
            ProviderAvailability::Unavailable {
                reason: "no provider registered for blbl".into(),
            }
        );
    }

    #[test]
    fn provider_registry_aggregates_every_target_without_stopping_on_rejection() {
        let mut request = request();
        request.targets = vec![Platform::Bilibili, Platform::Douyin];
        request.overrides.clear();
        let mut registry = ProviderRegistry::new();
        let (provider, calls) = test_provider(
            Platform::Douyin,
            ProviderAvailability::Available,
            DispatchOutcome::Queued {
                job_id: "must-not-run".into(),
            },
            Some("adapter failed"),
        );
        registry.register(provider).unwrap();

        let report = registry.dispatch_all(&request).unwrap();
        assert_eq!(
            report.outcomes,
            BTreeMap::from([
                (
                    Platform::Douyin,
                    DispatchOutcome::Rejected {
                        reason: "remote media error: adapter failed".into(),
                    },
                ),
                (
                    Platform::Bilibili,
                    DispatchOutcome::Unavailable {
                        reason: "no provider registered for blbl".into(),
                    },
                ),
            ])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parses_canonical_and_alias_platforms() {
        assert_eq!(Platform::from_str("dy"), Ok(Platform::Douyin));
        assert_eq!(Platform::from_str("抖音"), Ok(Platform::Douyin));
        assert_eq!(
            serde_json::to_string(&Platform::WechatChannels).unwrap(),
            "\"sph\""
        );
        assert_eq!(Platform::ALL.len(), 8);
    }
    #[test]
    fn validation_models_all_publish_fields() {
        assert!(request().validate().is_ok());
        let mut invalid = request();
        invalid.overrides.push(invalid.overrides[0].clone());
        assert_eq!(invalid.validate(), Err(DomainError::DuplicateOverrides));
    }
    #[test]
    fn local_schedule_is_exact() {
        assert!(LocalSchedule::parse("2026-01-02 03:04:05").is_ok());
        assert!(LocalSchedule::parse("2026-01-02T03:04:05Z").is_err());
    }
    #[test]
    fn sqlite_persists_and_transitions_deterministically() {
        let repository = SqliteRepository::in_memory().unwrap();
        let now = Utc::now();
        let job = repository.enqueue(&request(), now).unwrap();
        let dispatched = repository
            .advance(&job.id, 0, PublishState::Dispatching, now)
            .unwrap();
        assert_eq!(dispatched.revision, 1);
        assert!(matches!(
            repository.advance(&job.id, 0, PublishState::Published, now),
            Err(DomainError::StaleJobRevision { .. })
        ));
        assert_eq!(repository.job(&job.id).unwrap(), Some(dispatched));
    }

    #[test]
    fn claim_due_is_atomic_bounded_and_excludes_ineligible_jobs() {
        let repository = SqliteRepository::in_memory().unwrap();
        let updated_at = Utc::now();
        let due = LocalSchedule::parse("2026-01-02 03:04:05").unwrap();

        let due_job = repository.enqueue(&request(), updated_at).unwrap();

        let mut future_request = request();
        future_request.scheduled_at = Some(LocalSchedule::parse("2026-01-02 03:04:06").unwrap());
        let future_job = repository.enqueue(&future_request, updated_at).unwrap();

        let mut unscheduled_request = request();
        unscheduled_request.scheduled_at = None;
        let unscheduled_job = repository
            .enqueue(&unscheduled_request, updated_at)
            .unwrap();

        let mut draft_request = request();
        draft_request.draft = true;
        let draft_job = repository.enqueue(&draft_request, updated_at).unwrap();

        let claimed = repository
            .claim_due(
                &due,
                updated_at,
                <SqliteRepository as PublicationQueue>::MAX_CLAIM_BATCH + 1,
            )
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, due_job.id);
        assert_eq!(claimed[0].state, PublishState::Dispatching);
        assert_eq!(claimed[0].revision, 1);
        assert_eq!(claimed[0].due_at, Some(due.clone()));
        assert!(
            repository
                .claim_due(
                    &due,
                    updated_at,
                    <SqliteRepository as PublicationQueue>::MAX_CLAIM_BATCH,
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            repository.job(&future_job.id).unwrap().unwrap().state,
            PublishState::Queued
        );
        assert_eq!(
            repository.job(&unscheduled_job.id).unwrap().unwrap().state,
            PublishState::Queued
        );
        assert_eq!(
            repository.job(&draft_job.id).unwrap().unwrap().state,
            PublishState::Draft
        );
    }

    #[test]
    fn completing_a_claimed_job_records_one_matching_history_atomically() {
        let repository = SqliteRepository::in_memory().unwrap();
        let now = Utc::now();
        let job = repository.enqueue(&request(), now).unwrap();
        let due = job.due_at.clone().unwrap();
        let claimed = repository.claim_due(&due, now, 1).unwrap().pop().unwrap();
        // A pre-existing legacy ID in the scheduler namespace must not make a
        // terminal transition collide. The repository allocates and retries a
        // durable private sequence inside the same transaction.
        repository
            .append_history(&HistoryRecord {
                id: "scheduled-history-1".into(),
                request: request(),
                state: PublishState::Queued,
                recorded_at: now,
                detail: None,
            })
            .unwrap();
        let (completed, history) = repository
            .complete_job_with_history(
                &claimed.id,
                claimed.revision,
                PublishState::Published,
                now,
                Some("local runner workflow completed"),
            )
            .unwrap();
        assert_eq!(completed.state, PublishState::Published);
        assert_eq!(history.id, "scheduled-history-2");
        assert_eq!(history.request, claimed.request.runner_safe());
        assert_eq!(history.state, PublishState::Published);
        assert_eq!(repository.history().unwrap().len(), 2);
        assert!(matches!(
            repository.complete_job_with_history(
                &claimed.id,
                claimed.revision,
                PublishState::Published,
                now,
                None,
            ),
            Err(DomainError::InvalidStateTransition { .. })
        ));
        assert_eq!(repository.history().unwrap().len(), 2);
    }

    #[test]
    fn requeue_claim_is_revision_guarded_and_due_again() {
        let repository = SqliteRepository::in_memory().unwrap();
        let now = Utc::now();
        let job = repository.enqueue(&request(), now).unwrap();
        let due = job.due_at.clone().unwrap();
        let claimed = repository.claim_due(&due, now, 1).unwrap().pop().unwrap();

        assert!(matches!(
            repository.requeue_claim(&claimed.id, claimed.revision - 1, now),
            Err(DomainError::StaleJobRevision { .. })
        ));
        let requeued = repository
            .requeue_claim(&claimed.id, claimed.revision, now)
            .unwrap();
        assert_eq!(requeued.state, PublishState::Queued);
        assert_eq!(requeued.revision, claimed.revision + 1);
        let retry = repository.claim_due(&due, now, 1).unwrap().pop().unwrap();
        assert_eq!(retry.id, claimed.id);
        assert_eq!(retry.state, PublishState::Dispatching);
        assert_eq!(retry.revision, requeued.revision + 1);
    }

    #[test]
    fn competing_sqlite_connections_claim_a_due_job_once() {
        let path = env::temp_dir().join(format!(
            "matrixpost-claim-{}-{}.sqlite",
            std::process::id(),
            STAGING_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let first = Arc::new(SqliteRepository::open(&path).unwrap());
        let second = Arc::new(SqliteRepository::open(&path).unwrap());
        let now = Utc::now();
        let job = first.enqueue(&request(), now).unwrap();
        let due = job.due_at.clone().unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let contenders = [Arc::clone(&first), Arc::clone(&second)].map(|repository| {
            let barrier = Arc::clone(&barrier);
            let due = due.clone();
            thread::spawn(move || {
                barrier.wait();
                repository.claim_due(&due, now, 1)
            })
        });
        barrier.wait();
        let claims = contenders
            .into_iter()
            .map(|contender| contender.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(claims.iter().map(Vec::len).sum::<usize>(), 1);
        assert_eq!(
            first.job(&job.id).unwrap().unwrap().state,
            PublishState::Dispatching
        );
        drop(first);
        drop(second);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn sqlite_persists_safe_accounts_and_history() {
        let repository = SqliteRepository::in_memory().unwrap();
        let account = Account {
            id: "dy-primary".into(),
            platform: Platform::Douyin,
            display_name: "Primary".into(),
            status: AccountStatus::LoggedIn,
            phone: "13800138000".into(),
            partition: "persist:dy".into(),
        };
        repository.save_account(&account).unwrap();
        assert_eq!(repository.accounts().unwrap(), vec![account]);
        let record = HistoryRecord {
            id: "history-1".into(),
            request: request(),
            state: PublishState::Queued,
            recorded_at: Utc::now(),
            detail: None,
        };
        repository.append_history(&record).unwrap();
        assert_eq!(repository.history().unwrap(), vec![record]);
    }
    #[test]
    fn history_filter_includes_its_cutoff_and_intersects_platform_and_status() {
        let now = Utc::now();
        let cutoff = now - ChronoDuration::days(7);
        let filter = HistoryFilter::from_query(
            Some(7),
            false,
            Some(Platform::Douyin),
            Some(HistoryStatus::Scheduled),
            now,
        )
        .unwrap();
        let mut matching = request();
        matching.targets = vec![Platform::Douyin];
        let mut wrong_platform = matching.clone();
        wrong_platform.targets = vec![Platform::Bilibili];
        let history = vec![
            HistoryRecord {
                id: "inclusive".into(),
                request: matching.clone(),
                state: PublishState::Queued,
                recorded_at: cutoff,
                detail: None,
            },
            HistoryRecord {
                id: "draft-is-not-scheduled".into(),
                request: matching.clone(),
                state: PublishState::Draft,
                recorded_at: now,
                detail: None,
            },
            HistoryRecord {
                id: "wrong-platform".into(),
                request: wrong_platform,
                state: PublishState::Queued,
                recorded_at: now,
                detail: None,
            },
            HistoryRecord {
                id: "wrong-state".into(),
                request: matching,
                state: PublishState::Published,
                recorded_at: now,
                detail: None,
            },
        ];
        assert_eq!(
            filter
                .filter(history)
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec!["inclusive"]
        );
    }

    #[test]
    fn article_queue_persists_safe_due_claim_and_terminal_history() {
        let repository = SqliteRepository::in_memory().unwrap();
        let now = Utc::now();
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("13800000000".into()),
                partition: Some("persist:private".into()),
            },
            title: "Scheduled article".into(),
            content: Some("private body https://127.0.0.1:39002/secret".into()),
            file: Some("/private/article.md".into()),
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: Some(LocalSchedule::parse("2026-07-30 09:00:00").unwrap()),
        };
        let job = repository.enqueue_article(&request, now).unwrap();
        assert!(job.request.account.is_empty());

        let due = LocalSchedule::parse("2026-07-30 09:00:00").unwrap();
        let claimed = repository.claim_due_articles(&due, now, 1).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].state, PublishState::Dispatching);

        let (_, history) = repository
            .complete_article_with_history(
                &job.id,
                claimed[0].revision,
                PublishState::Unavailable,
                now,
                Some("article runner was unavailable"),
            )
            .unwrap();
        assert_eq!(history.platform, ArticlePlatform::Juejin);
        assert_eq!(history.title, "Scheduled article");
        let serialized = serde_json::to_string(&history).unwrap();
        for forbidden in [
            "private body",
            "/private/article.md",
            "13800000000",
            "127.0.0.1:39002",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        assert_eq!(repository.article_history().unwrap(), vec![history]);
    }

    #[test]
    fn article_queue_rejects_unscheduled_requests_without_persisting() {
        let repository = SqliteRepository::in_memory().unwrap();
        let request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection::default(),
            title: "Immediate article".into(),
            content: Some("body".into()),
            file: None,
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: None,
        };
        assert!(repository.enqueue_article(&request, Utc::now()).is_err());
        assert!(repository.article_history().unwrap().is_empty());
    }

    #[test]
    fn migration_from_v8_redacts_article_history_without_stranding_queued_jobs() {
        let repository = SqliteRepository::in_memory().unwrap();
        let due = LocalSchedule::parse("2026-07-30 09:00:00").unwrap();
        let recorded_at = "2026-07-30T08:00:00+00:00";
        let safe_queued_request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection::default(),
            title: "Queued article".into(),
            content: Some("queued article body".into()),
            file: None,
            cover: None,
            category: None,
            tags: vec!["migration".into()],
            summary: None,
            scheduled_at: Some(due.clone()),
        };
        let legacy_history_request = PublishArticleRequest {
            platform: "juejin".into(),
            account: AccountSelection {
                phone: Some("13800138000".into()),
                partition: Some("persist:private".into()),
            },
            title: "Legacy article".into(),
            content: Some("private body http://127.0.0.1:39002/secret".into()),
            file: Some("/private/article.md".into()),
            cover: None,
            category: None,
            tags: Vec::new(),
            summary: None,
            scheduled_at: Some(due.clone()),
        };
        {
            let connection = repository.locked().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE article_history; DROP TABLE article_jobs; DROP TABLE article_job_sequence; DROP TABLE article_history_sequence; DELETE FROM schema_migrations WHERE version=9; CREATE TABLE article_jobs (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, due_at TEXT NOT NULL, revision INTEGER NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE article_history (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); CREATE TABLE article_job_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); CREATE TABLE article_history_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT);",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO article_jobs(id, request_json, state, due_at, revision, updated_at) VALUES (?1, ?2, 'queued', ?3, 0, ?4)",
                    params![
                        "article-job-legacy",
                        serde_json::to_string(&safe_queued_request).unwrap(),
                        due.0,
                        recorded_at,
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO article_history(id, request_json, state, recorded_at, detail) VALUES (?1, ?2, 'published', ?3, ?4)",
                    params![
                        "article-history-legacy",
                        serde_json::to_string(&legacy_history_request).unwrap(),
                        recorded_at,
                        "legacy runner detail",
                    ],
                )
                .unwrap();
            SqliteRepository::migrate(&connection).unwrap();

            let schema: String = connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name='article_history'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!schema.contains("request_json"));
            SqliteRepository::migrate(&connection).unwrap();
            let repeated_schema: String = connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name='article_history'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(repeated_schema, schema);
            let migration_count: u64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM schema_migrations WHERE version=9",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(migration_count, 1);
            let columns = connection
                .prepare("PRAGMA table_info(article_history)")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(!columns.iter().any(|column| column == "request_json"));
        }

        let claimed = repository
            .claim_due_articles(&due, Utc::now(), 1)
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, "article-job-legacy");
        assert_eq!(claimed[0].state, PublishState::Dispatching);
        assert!(!claimed[0].request.has_account_routing());

        let history = repository.article_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "article-history-legacy");
        assert_eq!(history[0].platform, ArticlePlatform::Juejin);
        assert_eq!(history[0].title, "Legacy article");
        assert_eq!(history[0].state, PublishState::Published);
        assert_eq!(history[0].recorded_at.to_rfc3339(), recorded_at);
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
