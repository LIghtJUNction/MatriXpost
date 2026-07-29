use std::{collections::BTreeMap, sync::Arc};

use chrono::{Duration, TimeZone, Utc};
use matrixpost_core::{
    AccountSelection, DispatchOutcome, DomainError, HistoryRecord, LocalSchedule, MediaSource,
    Platform, ProviderAvailability, ProviderRegistry, PublishProvider, PublishRequest,
    PublishState, Repository, SqliteRepository,
};
use serde::Deserialize;
use serde::de::value::{
    BoolDeserializer, Error as ValueError, MapDeserializer, StringDeserializer,
};

use super::{
    AccountReadinessInput, AddLifecycleBusinessRelationInput, AddLifecycleContentAttributionInput,
    AppendLifecycleLedgerEntryInput, CreateLifecycleObjectInput, DesktopService,
    DispatchToLocalRunnerInput, FanqieReviewStatusInput, HistoryQueryInput,
    LifecycleApprovalStatusInput, LifecycleLedgerDirectionInput, LifecycleObjectIdInput,
    LifecycleStatusInput, SaveAccountInput, SaveArticleAccountInput, SaveDraftInput,
    TransitionLifecycleObjectInput,
};

fn service() -> DesktopService {
    DesktopService::new(Arc::new(
        SqliteRepository::in_memory().expect("in-memory state"),
    ))
}

struct UnavailableLocalRunner;

impl PublishProvider for UnavailableLocalRunner {
    fn platform(&self) -> Platform {
        Platform::Douyin
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Unavailable {
            reason: "private runner endpoint must not be exposed".into(),
        }
    }

    fn enqueue(&self, _: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        unreachable!("unavailable providers must not receive a dispatch")
    }
}

fn direct_runner_request() -> PublishRequest {
    PublishRequest {
        source: MediaSource::LocalFile("/media/example.mp4".into()),
        title: "One-shot local request".into(),
        short_title: None,
        tags: Vec::new(),
        address: None,
        draft: false,
        bt2: None,
        scheduled_at: None,
        task_name: None,
        account: Default::default(),
        wechat_link: Default::default(),
        overrides: Vec::new(),
        targets: vec![Platform::Douyin],
    }
}

fn history_input(
    days: Option<u16>,
    all: bool,
    platform: Option<&str>,
    status: Option<&str>,
) -> HistoryQueryInput {
    HistoryQueryInput {
        days,
        all,
        platform: platform.map(str::to_owned),
        status: status.map(str::to_owned),
    }
}

fn local_runner_input(
    provider_runners: Vec<&str>,
    scheduled_at: Option<&str>,
) -> DispatchToLocalRunnerInput {
    DispatchToLocalRunnerInput {
        title: "One-shot local request".into(),
        media_path: "/media/example.mp4".into(),
        targets: vec!["dy".into()],
        scheduled_at: scheduled_at.map(str::to_owned),
        provider_runners: provider_runners.into_iter().map(str::to_owned).collect(),
        confirmed: true,
    }
}

#[test]
fn account_readiness_without_a_runner_is_unavailable_and_safe() {
    let report = service()
        .account_readiness(AccountReadinessInput {
            platform: "dy".into(),
            provider_runner: None,
            confirmed: true,
        })
        .expect("no declaration is a safe unavailable result");

    assert_eq!(report.state, "unavailable");
    assert!(!format!("{report:?}").contains("127.0.0.1"));
}

#[test]
fn probes_require_confirmation_and_matching_loopback_runner() {
    let unconfirmed = service()
        .account_readiness(AccountReadinessInput {
            platform: "dy".into(),
            provider_runner: Some("dy=tcp:127.0.0.1:39001".into()),
            confirmed: false,
        })
        .expect_err("confirmation must precede runner use");
    assert_eq!(
        unconfirmed.to_string(),
        "invalid local draft: explicit account readiness confirmation is required"
    );

    let mismatch = service()
        .fanqie_review_status(FanqieReviewStatusInput {
            title_query: "safe title".into(),
            provider_runner: Some("dy=tcp:127.0.0.1:39001".into()),
            confirmed: true,
        })
        .expect_err("Fanqie probe must use a matching runner");
    assert_eq!(
        mismatch.to_string(),
        "invalid local draft: runner must use the matching PLATFORM=tcp:127.0.0.1:PORT declaration"
    );
}

#[test]
fn fanqie_review_without_a_runner_is_unavailable_and_does_not_echo_title() {
    let report = service()
        .fanqie_review_status(FanqieReviewStatusInput {
            title_query: "private test title".into(),
            provider_runner: None,
            confirmed: true,
        })
        .expect("no declaration is a safe unavailable result");

    assert_eq!(report.state, "unavailable");
    assert!(!format!("{report:?}").contains("private test title"));
}

#[test]
fn fanqie_review_input_rejects_unknown_fields_and_invalid_title() {
    let input = [("unexpected", "not accepted")]
        .into_iter()
        .map(|(key, value)| {
            (
                StringDeserializer::<ValueError>::new(key.to_owned()),
                StringDeserializer::<ValueError>::new(value.to_owned()),
            )
        });
    let error = FanqieReviewStatusInput::deserialize(MapDeserializer::new(input))
        .expect_err("unknown IPC input must fail");
    assert!(error.to_string().contains("unknown field `unexpected`"));

    let error = service()
        .fanqie_review_status(FanqieReviewStatusInput {
            title_query: "   \n\t".into(),
            provider_runner: None,
            confirmed: true,
        })
        .expect_err("blank title must fail before any runner request");
    assert_eq!(
        error.to_string(),
        "invalid local draft: review title query must be non-empty and within the local limit"
    );
}

fn history_record(
    id: &str,
    title: &str,
    platform: matrixpost_core::Platform,
    state: PublishState,
    recorded_at: chrono::DateTime<Utc>,
    draft: bool,
    scheduled: bool,
) -> HistoryRecord {
    HistoryRecord {
        id: id.into(),
        request: PublishRequest {
            source: MediaSource::LocalFile("/private/video.mp4".into()),
            title: title.into(),
            short_title: None,
            tags: Vec::new(),
            address: None,
            draft,
            bt2: None,
            scheduled_at: scheduled.then(|| LocalSchedule("2030-01-02 03:04:05".into())),
            task_name: None,
            account: AccountSelection {
                phone: Some("private-route".into()),
                partition: Some("persist:private".into()),
            },
            wechat_link: Default::default(),
            overrides: Vec::new(),
            targets: vec![platform],
        },
        state,
        recorded_at,
        detail: Some("private detail".into()),
    }
}

#[test]
fn snapshot_is_credential_free_and_reports_unavailable_providers() {
    let snapshot = service().snapshot().expect("snapshot");

    assert_eq!(snapshot.platforms.len(), 8);
    assert!(snapshot.accounts.is_empty());
    assert!(snapshot.article_accounts.is_empty());
    assert_eq!(snapshot.history_count, 0);
    assert!(!snapshot.provider_automation_available);
}

#[test]
fn saving_a_draft_forces_draft_state_without_remote_dispatch() {
    let service = service();
    let saved = service
        .save_local_draft(SaveDraftInput {
            title: "Local planning only".into(),
            media_path: "/media/example.mp4".into(),
            targets: vec!["dy".into()],
            scheduled_at: None,
        })
        .expect("local draft");

    assert_eq!(saved.state, "draft");
    assert!(!saved.remote_publish_attempted);
    let job = service
        .repository
        .job(&saved.id)
        .expect("job lookup")
        .expect("saved job");
    assert_eq!(job.state, PublishState::Draft);
}

#[test]
fn local_runner_dispatch_rejects_non_loopback_declarations_before_transport() {
    let error = service()
        .dispatch_to_local_runner(local_runner_input(vec!["dy=tcp:192.0.2.1:39001"], None))
        .expect_err("non-loopback runner must be rejected before dispatch");

    assert_eq!(
        error.to_string(),
        "invalid local draft: each runner must use PLATFORM=tcp:127.0.0.1:PORT and be local"
    );
}

#[test]
fn local_runner_dispatch_requires_confirmation_before_runner_parsing() {
    let mut input = local_runner_input(vec!["not-a-runner"], None);
    input.confirmed = false;

    let error = service()
        .dispatch_to_local_runner(input)
        .expect_err("unconfirmed dispatch must stop before runner parsing");

    assert_eq!(
        error.to_string(),
        "invalid local draft: explicit local runner confirmation is required"
    );
}

#[test]
fn local_runner_mapping_rejects_missing_target_before_dispatch() {
    let error = match crate::runner::local_runner_registry(
        &["dy=tcp:127.0.0.1:39001".into()],
        &[Platform::Douyin, Platform::Xiaohongshu],
    ) {
        Err(error) => error,
        Ok(_) => panic!("every selected platform must have a runner"),
    };

    assert_eq!(
        error.to_string(),
        "invalid local draft: declare exactly one local runner for every selected platform"
    );
}

#[test]
fn local_runner_mapping_rejects_duplicate_platform_before_dispatch() {
    let error = match crate::runner::local_runner_registry(
        &[
            "dy=tcp:127.0.0.1:39001".into(),
            "dy=tcp:127.0.0.1:39002".into(),
        ],
        &[Platform::Douyin],
    ) {
        Err(error) => error,
        Ok(_) => panic!("a target may only map to one runner"),
    };

    assert_eq!(
        error.to_string(),
        "invalid local draft: declare each selected platform at most once"
    );
}

#[test]
fn local_runner_mapping_accepts_complete_multi_target_loopback_declarations() {
    let registry = crate::runner::local_runner_registry(
        &[
            "dy=tcp:127.0.0.1:39001".into(),
            "xhs=tcp:127.0.0.1:39002".into(),
        ],
        &[Platform::Douyin, Platform::Xiaohongshu],
    )
    .expect("complete local runner mapping");

    assert_eq!(
        registry.availability(Platform::Douyin),
        ProviderAvailability::Available
    );
    assert_eq!(
        registry.availability(Platform::Xiaohongshu),
        ProviderAvailability::Available
    );
}

#[test]
fn local_runner_dispatch_rejects_schedules_before_runner_transport() {
    let error = service()
        .dispatch_to_local_runner(local_runner_input(
            vec!["dy=tcp:127.0.0.1:39001"],
            Some("2030-01-02 03:04:05"),
        ))
        .expect_err("direct dispatch cannot be scheduled");

    assert_eq!(
        error.to_string(),
        "invalid local draft: scheduled dispatch is not available; save a local draft instead"
    );
}

#[test]
fn local_runner_dispatch_reports_unavailable_runner_without_sensitive_details() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(Box::new(UnavailableLocalRunner))
        .expect("registered unavailable local runner");
    let repository = SqliteRepository::in_memory().expect("in-memory state");
    let report = crate::runner::local_runner_dispatch_report(
        &repository,
        &registry,
        &direct_runner_request(),
    )
    .expect("unavailable local runner is reported without transport");

    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].platform, "dy");
    assert_eq!(report.outcomes[0].state, "runner_unavailable");
    assert_eq!(
        report.outcomes[0].reason,
        "the declared local runner is unavailable for this platform"
    );
    assert!(!report.remote_publish_confirmed);
    let rendered = format!("{report:?}");
    assert!(!rendered.contains("private runner endpoint"));
    let history = repository.history().expect("local dispatch history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, PublishState::Unavailable);
}

#[test]
fn local_runner_rejection_projection_is_safe_and_never_confirms_remote_publication() {
    let outcome = crate::runner::local_runner_dispatch_outcome(
        Platform::Douyin,
        DispatchOutcome::Rejected {
            reason: "private runner response".into(),
        },
    );

    assert_eq!(outcome.platform, "dy");
    assert_eq!(outcome.state, "runner_rejected");
    assert_eq!(
        outcome.reason,
        "the local runner did not accept this request"
    );
    assert!(!outcome.reason.contains("private runner response"));
}

#[test]
fn local_runner_input_rejects_unknown_fields() {
    let input = [("unexpected", "not accepted")]
        .into_iter()
        .map(|(key, value)| {
            (
                StringDeserializer::<ValueError>::new(key.to_owned()),
                StringDeserializer::<ValueError>::new(value.to_owned()),
            )
        });
    let error = DispatchToLocalRunnerInput::deserialize(MapDeserializer::new(input))
        .expect_err("unknown IPC input must fail");

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn saving_account_metadata_persists_without_credentials() {
    let service = service();
    let saved = service
        .save_account(SaveAccountInput {
            platform: "dy".into(),
            display_name: "Studio account".into(),
            status: "logged_out".into(),
            phone: "route-01".into(),
            partition: "persist:studio".into(),
        })
        .expect("safe account metadata");

    assert_eq!(saved.id, "dy-studio-account");
    assert_eq!(
        service.snapshot().expect("snapshot").accounts,
        vec![super::AccountEntry {
            id: saved.id,
            platform: "dy",
            display_name: "Studio account".into(),
            status: "logged_out",
        }]
    );
    let rendered = format!("{:?}", service.snapshot().expect("snapshot").accounts);
    assert!(!rendered.contains("route-01"));
    assert!(!rendered.contains("persist:studio"));
}

#[test]
fn saving_account_rejects_invalid_routing_metadata() {
    let error = service()
        .save_account(SaveAccountInput {
            platform: "dy".into(),
            display_name: "Studio account".into(),
            status: "logged_out".into(),
            phone: "".into(),
            partition: "not-a-partition".into(),
        })
        .expect_err("invalid route must fail");

    assert!(
        error
            .to_string()
            .contains("partition must start with persist:")
    );
}

#[test]
fn account_input_rejects_secret_named_unknown_fields() {
    let input = [
        ("platform", "dy"),
        ("displayName", "Studio account"),
        ("status", "logged_out"),
        ("phone", "route-01"),
        ("partition", "persist:studio"),
        ("password", "must-not-be-accepted"),
    ]
    .into_iter()
    .map(|(key, value)| {
        (
            StringDeserializer::<ValueError>::new(key.to_owned()),
            StringDeserializer::<ValueError>::new(value.to_owned()),
        )
    });
    let error = SaveAccountInput::deserialize(MapDeserializer::new(input))
        .expect_err("secret-named unknown field must fail");

    assert!(error.to_string().contains("unknown field `password`"));
}

#[test]
fn saving_juejin_article_metadata_persists_only_the_safe_desktop_entry() {
    let service = service();
    let saved = service
        .save_article_account(SaveArticleAccountInput {
            display_name: "Juejin Notes".into(),
            status: "logged_out".into(),
            phone: "route-jj-01".into(),
            partition: "persist:juejin-notes".into(),
        })
        .expect("safe Juejin metadata");
    assert_eq!(saved.id, "juejin-juejin-notes");
    assert_eq!(saved.status, "logged_out");
    assert_eq!(
        service.snapshot().expect("snapshot").article_accounts,
        vec![super::ArticleAccountEntry {
            id: "juejin-juejin-notes".into(),
            display_name: "Juejin Notes".into(),
            status: "logged_out",
        }]
    );
}

#[test]
fn saving_juejin_article_metadata_rejects_invalid_routing() {
    let error = service()
        .save_article_account(SaveArticleAccountInput {
            display_name: "Juejin Notes".into(),
            status: "logged_out".into(),
            phone: String::new(),
            partition: "not-a-partition".into(),
        })
        .expect_err("invalid route must fail");
    assert!(
        error
            .to_string()
            .contains("partition must start with persist:")
    );
}

#[test]
fn article_account_input_rejects_secret_named_unknown_fields() {
    let input = [
        ("displayName", "Juejin Notes"),
        ("status", "logged_out"),
        ("phone", "route-jj-01"),
        ("partition", "persist:juejin-notes"),
        ("token", "must-not-be-accepted"),
    ]
    .into_iter()
    .map(|(key, value)| {
        (
            StringDeserializer::<ValueError>::new(key.to_owned()),
            StringDeserializer::<ValueError>::new(value.to_owned()),
        )
    });
    let error = SaveArticleAccountInput::deserialize(MapDeserializer::new(input))
        .expect_err("secret-named unknown field must fail");
    assert!(error.to_string().contains("unknown field `token`"));
}

#[test]
fn history_query_accepts_camel_case_fields_and_rejects_secret_unknown_fields() {
    let parse = |fields: Vec<(&str, bool)>| {
        HistoryQueryInput::deserialize(MapDeserializer::new(fields.into_iter().map(
            |(key, value)| {
                (
                    StringDeserializer::<ValueError>::new(key.to_owned()),
                    BoolDeserializer::<ValueError>::new(value),
                )
            },
        )))
    };
    let query = parse(vec![("all", false)]).expect("valid camelCase history query");
    assert_eq!(query.days, None);
    assert!(!query.all);
    assert_eq!(query.platform, None);
    assert_eq!(query.status, None);

    let error = parse(vec![("all", false), ("token", true)])
        .expect_err("secret-named unknown field must fail");
    assert!(error.to_string().contains("unknown field `token`"));
}

#[test]
fn history_defaults_to_seven_days_and_all_removes_the_cutoff() {
    let service = service();
    let now = Utc
        .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
        .single()
        .expect("fixed clock");
    service
        .repository
        .append_history(&history_record(
            "recent",
            "Recent",
            matrixpost_core::Platform::Douyin,
            PublishState::Published,
            now - Duration::days(7),
            false,
            false,
        ))
        .expect("recent history");
    service
        .repository
        .append_history(&history_record(
            "old",
            "Old",
            matrixpost_core::Platform::Douyin,
            PublishState::Published,
            now - Duration::days(8),
            false,
            false,
        ))
        .expect("old history");

    assert_eq!(
        service
            .history_entries(history_input(None, false, None, None), now)
            .expect("default history")
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["recent"]
    );
    assert_eq!(
        service
            .history_entries(history_input(None, true, None, None), now)
            .expect("all history")
            .len(),
        2
    );
}

#[test]
fn history_intersects_platform_and_status_and_scheduled_excludes_drafts() {
    let service = service();
    let now = Utc
        .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
        .single()
        .expect("fixed clock");
    for record in [
        history_record(
            "dy-success",
            "Dy success",
            matrixpost_core::Platform::Douyin,
            PublishState::Published,
            now,
            false,
            false,
        ),
        history_record(
            "dy-failed",
            "Dy failed",
            matrixpost_core::Platform::Douyin,
            PublishState::Failed,
            now,
            false,
            false,
        ),
        history_record(
            "xhs-success",
            "Xhs success",
            matrixpost_core::Platform::Xiaohongshu,
            PublishState::Published,
            now,
            false,
            false,
        ),
        history_record(
            "draft",
            "Draft",
            matrixpost_core::Platform::Douyin,
            PublishState::Draft,
            now,
            true,
            true,
        ),
        history_record(
            "queued",
            "Queued",
            matrixpost_core::Platform::Douyin,
            PublishState::Queued,
            now,
            false,
            true,
        ),
    ] {
        service.repository.append_history(&record).expect("history");
    }

    assert_eq!(
        service
            .history_entries(history_input(None, true, Some("dy"), Some("success")), now)
            .expect("intersected history")
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["dy-success"]
    );
    let scheduled = service
        .history_entries(
            history_input(None, true, Some("dy"), Some("scheduled")),
            now,
        )
        .expect("scheduled history");
    assert_eq!(scheduled.len(), 1);
    assert_eq!(scheduled[0].id, "queued");
    assert!(scheduled[0].scheduled);
}

#[test]
fn history_entries_never_include_media_or_account_routing() {
    let service = service();
    let now = Utc
        .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
        .single()
        .expect("fixed clock");
    service
        .repository
        .append_history(&history_record(
            "safe",
            "Safe title",
            matrixpost_core::Platform::Douyin,
            PublishState::Draft,
            now,
            true,
            false,
        ))
        .expect("history");

    let entry = service
        .history_entries(history_input(None, true, None, None), now)
        .expect("safe history")
        .pop()
        .expect("history entry");
    let rendered = format!("{entry:?}");
    assert!(!rendered.contains("/private/video.mp4"));
    assert!(!rendered.contains("private-route"));
    assert!(!rendered.contains("persist:private"));
    assert!(!rendered.contains("private detail"));
    assert!(entry.draft);
    assert!(!entry.scheduled);
}

#[test]
fn lifecycle_input_rejects_unknown_fields() {
    let input = [("businessObjectId", "object-1"), ("unexpected", "value")]
        .into_iter()
        .map(|(key, value)| {
            (
                StringDeserializer::<ValueError>::new(key.to_owned()),
                StringDeserializer::<ValueError>::new(value.to_owned()),
            )
        });
    let error = LifecycleObjectIdInput::deserialize(MapDeserializer::new(input))
        .expect_err("unknown lifecycle field must fail");

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn lifecycle_child_lists_reject_missing_objects_without_exposing_the_identifier() {
    let service = service();

    for result in [
        service
            .lifecycle_ledger_entries("missing-object".into())
            .map(|_| ()),
        service
            .lifecycle_content_attributions("missing-object".into())
            .map(|_| ()),
        service
            .lifecycle_business_relations("missing-object".into())
            .map(|_| ()),
    ] {
        assert_eq!(
            result
                .expect_err("missing object must not look like an empty list")
                .to_string(),
            "local lifecycle record was not found: the requested lifecycle record does not exist"
        );
    }
}

#[test]
fn lifecycle_service_round_trips_object_ledger_and_content_attribution() {
    let service = service();
    let object = service
        .create_lifecycle_object(CreateLifecycleObjectInput {
            id: "project-1".into(),
            kind: "project".into(),
            external_id: Some("external-1".into()),
            display_name: "Launch plan".into(),
            attributes: BTreeMap::from([("region".into(), "north".into())]),
        })
        .expect("lifecycle object");
    assert_eq!(object.lifecycle_status, "draft");
    assert_eq!(object.approval_status, "pending");
    assert_eq!(object.revision, 0);
    assert_eq!(service.lifecycle_objects().expect("objects"), vec![object]);

    let entry = service
        .append_lifecycle_ledger_entry(AppendLifecycleLedgerEntryInput {
            id: "entry-1".into(),
            business_object_id: "project-1".into(),
            direction: LifecycleLedgerDirectionInput::Expense,
            category: "materials".into(),
            amount_minor: 1250,
            currency: "CNY".into(),
            approval_status: Some(LifecycleApprovalStatusInput::Approved),
            counterparty: Some("Supplier".into()),
            reference: None,
            description: Some("Sample purchase".into()),
        })
        .expect("ledger entry");
    assert_eq!(entry.direction, "expense");
    assert_eq!(entry.amount_minor, 1250);
    assert_eq!(
        service
            .lifecycle_ledger_entries("project-1".into())
            .expect("ledger entries"),
        vec![entry]
    );

    let recorded_at = Utc
        .with_ymd_and_hms(2030, 1, 10, 12, 0, 0)
        .single()
        .expect("fixed clock");
    service
        .repository
        .append_history(&history_record(
            "history-1",
            "Local draft",
            matrixpost_core::Platform::Douyin,
            PublishState::Draft,
            recorded_at,
            true,
            false,
        ))
        .expect("seeded history");
    let attribution = service
        .add_lifecycle_content_attribution(AddLifecycleContentAttributionInput {
            business_object_id: "project-1".into(),
            history_id: "history-1".into(),
        })
        .expect("content attribution");
    assert_eq!(attribution.history_id, "history-1");
    assert_eq!(
        service
            .lifecycle_content_attributions("project-1".into())
            .expect("attributions"),
        vec![attribution]
    );

    service
        .create_lifecycle_object(CreateLifecycleObjectInput {
            id: "customer-1".into(),
            kind: "customer".into(),
            external_id: None,
            display_name: "Example customer".into(),
            attributes: BTreeMap::new(),
        })
        .expect("related object");
    let relation = service
        .add_lifecycle_business_relation(AddLifecycleBusinessRelationInput {
            id: "relation-1".into(),
            source_business_object_id: "project-1".into(),
            target_business_object_id: "customer-1".into(),
            relation_type: "customer_interest".into(),
            attributes: BTreeMap::from([("priority".into(), "high".into())]),
        })
        .expect("business relation");
    assert_eq!(relation.relation_type, "customer_interest");
    assert_eq!(
        service
            .lifecycle_business_relations("customer-1".into())
            .expect("inbound relation"),
        vec![relation]
    );
}

#[test]
fn lifecycle_transition_increments_revision_and_rejects_stale_updates() {
    let service = service();
    service
        .create_lifecycle_object(CreateLifecycleObjectInput {
            id: "asset-1".into(),
            kind: "asset".into(),
            external_id: None,
            display_name: "Reusable asset".into(),
            attributes: BTreeMap::new(),
        })
        .expect("lifecycle object");
    let transitioned = service
        .transition_lifecycle_object(TransitionLifecycleObjectInput {
            id: "asset-1".into(),
            expected_revision: 0,
            lifecycle_status: LifecycleStatusInput::Active,
            approval_status: LifecycleApprovalStatusInput::Pending,
        })
        .expect("transition");
    assert_eq!(transitioned.lifecycle_status, "active");
    assert_eq!(transitioned.revision, 1);

    let error = service
        .transition_lifecycle_object(TransitionLifecycleObjectInput {
            id: "asset-1".into(),
            expected_revision: 0,
            lifecycle_status: LifecycleStatusInput::Completed,
            approval_status: LifecycleApprovalStatusInput::Pending,
        })
        .expect_err("stale transition must fail");
    assert_eq!(
        error.to_string(),
        "invalid local draft: lifecycle request could not be completed"
    );
}
