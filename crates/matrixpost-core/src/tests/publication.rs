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
