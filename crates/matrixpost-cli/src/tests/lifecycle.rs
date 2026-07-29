use std::{collections::BTreeMap, process::ExitCode};

use clap::Parser;
use matrixpost_core::{
    ApprovalStatus, BusinessObjectStatus, LedgerDirection, LifecycleRepository, SqliteRepository,
};

use crate::{
    args::{
        AttributionArgs, AttributionCommand, Cli, Command, LedgerAddArgs, LedgerArgs,
        LedgerCommand, LifecycleArgs, LifecycleCommand, ObjectArgs, ObjectCommand,
        ObjectCreateArgs, RelationAddArgs, RelationArgs, RelationCommand,
    },
    lifecycle::execute_lifecycle,
    query::parse_attributes,
};

#[test]
fn lifecycle_object_create_parses_generic_attributes_and_defaults() {
    let parsed = Cli::try_parse_from([
        "matrixpost",
        "lifecycle",
        "object",
        "create",
        "--id",
        "campaign-42",
        "--kind",
        "campaign",
        "--display-name",
        "Launch",
        "--attribute",
        "region=cn",
        "--attribute",
        "channel=video",
    ])
    .unwrap();
    let Command::Lifecycle(LifecycleArgs {
        command:
            LifecycleCommand::Object(ObjectArgs {
                command: ObjectCommand::Create(args),
            }),
    }) = parsed.command
    else {
        panic!("expected lifecycle object create")
    };
    assert_eq!(args.lifecycle_status, BusinessObjectStatus::Draft);
    assert_eq!(args.approval_status, ApprovalStatus::Pending);
    assert_eq!(
        parse_attributes(args.attributes).unwrap(),
        BTreeMap::from([
            ("channel".to_owned(), "video".to_owned()),
            ("region".to_owned(), "cn".to_owned())
        ])
    );
}
#[test]
fn lifecycle_ledger_parsing_rejects_invalid_direction_and_minor_amount() {
    let valid = Cli::try_parse_from([
        "matrixpost",
        "lifecycle",
        "ledger",
        "add",
        "--id",
        "entry-1",
        "--object",
        "campaign-42",
        "--direction",
        "expense",
        "--category",
        "media",
        "--amount-minor",
        "1250",
        "--currency",
        "CNY",
    ]);
    assert!(valid.is_ok());
    assert!(
        Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "ledger",
            "add",
            "--id",
            "entry-1",
            "--object",
            "campaign-42",
            "--direction",
            "cost",
            "--category",
            "media",
            "--amount-minor",
            "1250",
            "--currency",
            "CNY"
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "matrixpost",
            "lifecycle",
            "ledger",
            "add",
            "--id",
            "entry-1",
            "--object",
            "campaign-42",
            "--direction",
            "expense",
            "--category",
            "media",
            "--amount-minor",
            "0",
            "--currency",
            "CNY"
        ])
        .is_err()
    );
}
#[test]
fn lifecycle_attribution_and_transition_arguments_are_typed() {
    let attribution = Cli::try_parse_from([
        "matrixpost",
        "lifecycle",
        "attribution",
        "add",
        "--object",
        "campaign-42",
        "--history",
        "publication-7",
        "--created-at",
        "2026-07-29T01:02:03Z",
    ])
    .unwrap();
    let Command::Lifecycle(LifecycleArgs {
        command:
            LifecycleCommand::Attribution(AttributionArgs {
                command: AttributionCommand::Add(args),
            }),
    }) = attribution.command
    else {
        panic!("expected lifecycle attribution add")
    };
    assert_eq!(args.business_object_id, "campaign-42");
    assert_eq!(args.history_id, "publication-7");
    assert_eq!(
        args.created_at.unwrap().to_rfc3339(),
        "2026-07-29T01:02:03+00:00"
    );
    let transition = Cli::try_parse_from([
        "matrixpost",
        "lifecycle",
        "transition",
        "--id",
        "campaign-42",
        "--expected-revision",
        "7",
        "--lifecycle-status",
        "active",
        "--approval-status",
        "approved",
    ])
    .unwrap();
    let Command::Lifecycle(LifecycleArgs {
        command: LifecycleCommand::Transition(args),
    }) = transition.command
    else {
        panic!("expected lifecycle transition")
    };
    assert_eq!(args.id, "campaign-42");
    assert_eq!(args.expected_revision, 7);
    assert_eq!(args.lifecycle_status, BusinessObjectStatus::Active);
    assert_eq!(args.approval_status, ApprovalStatus::Approved);
}
#[test]
fn lifecycle_relation_arguments_accept_safe_generic_attributes() {
    let parsed = Cli::try_parse_from([
        "matrixpost",
        "lifecycle",
        "relation",
        "add",
        "--id",
        "interest-1",
        "--source",
        "asset-1",
        "--target",
        "customer-1",
        "--type",
        "customer_interest",
        "--attribute",
        "priority=high",
    ])
    .unwrap();
    let Command::Lifecycle(LifecycleArgs {
        command:
            LifecycleCommand::Relation(RelationArgs {
                command: RelationCommand::Add(args),
            }),
    }) = parsed.command
    else {
        panic!("expected lifecycle relation add")
    };
    assert_eq!(args.id, "interest-1");
    assert_eq!(args.source_business_object_id, "asset-1");
    assert_eq!(args.target_business_object_id, "customer-1");
    assert_eq!(args.relation_type, "customer_interest");
    assert_eq!(
        parse_attributes(args.attributes).unwrap(),
        BTreeMap::from([("priority".to_owned(), "high".to_owned())])
    );
}
#[test]
fn lifecycle_commands_persist_a_generic_object_and_immutable_ledger_entry() {
    let repository = SqliteRepository::in_memory().unwrap();
    let create = LifecycleCommand::Object(ObjectArgs {
        command: ObjectCommand::Create(ObjectCreateArgs {
            id: "campaign-42".into(),
            kind: "campaign".into(),
            display_name: "Launch".into(),
            external_id: None,
            lifecycle_status: BusinessObjectStatus::Draft,
            approval_status: ApprovalStatus::Pending,
            attributes: vec!["channel=video".into()],
        }),
    });
    assert_eq!(execute_lifecycle(create, &repository), ExitCode::SUCCESS);
    let add = LifecycleCommand::Ledger(LedgerArgs {
        command: LedgerCommand::Add(LedgerAddArgs {
            id: "entry-1".into(),
            business_object_id: "campaign-42".into(),
            direction: LedgerDirection::Expense,
            category: "media".into(),
            amount_minor: 1250,
            currency: "CNY".into(),
            approval_status: ApprovalStatus::Pending,
            occurred_at: None,
            counterparty: None,
            reference: None,
            description: None,
        }),
    });
    assert_eq!(execute_lifecycle(add, &repository), ExitCode::SUCCESS);
    assert_eq!(repository.business_objects().unwrap().len(), 1);
    assert_eq!(repository.ledger_entries("campaign-42").unwrap().len(), 1);
}
#[test]
fn lifecycle_relation_commands_persist_and_list_directed_relations() {
    let repository = SqliteRepository::in_memory().unwrap();
    for (id, kind, display_name) in [
        ("asset-1", "asset", "Asset"),
        ("customer-1", "customer", "Customer"),
    ] {
        assert_eq!(
            execute_lifecycle(
                LifecycleCommand::Object(ObjectArgs {
                    command: ObjectCommand::Create(ObjectCreateArgs {
                        id: id.into(),
                        kind: kind.into(),
                        display_name: display_name.into(),
                        external_id: None,
                        lifecycle_status: BusinessObjectStatus::Draft,
                        approval_status: ApprovalStatus::Pending,
                        attributes: vec![]
                    })
                }),
                &repository
            ),
            ExitCode::SUCCESS
        );
    }
    let add = LifecycleCommand::Relation(RelationArgs {
        command: RelationCommand::Add(RelationAddArgs {
            id: "interest-1".into(),
            source_business_object_id: "asset-1".into(),
            target_business_object_id: "customer-1".into(),
            relation_type: "customer_interest".into(),
            attributes: vec!["priority=high".into()],
        }),
    });
    assert_eq!(execute_lifecycle(add, &repository), ExitCode::SUCCESS);
    assert_eq!(
        execute_lifecycle(
            LifecycleCommand::Relation(RelationArgs {
                command: RelationCommand::List {
                    business_object_id: "customer-1".into()
                }
            }),
            &repository
        ),
        ExitCode::SUCCESS
    );
    assert_eq!(
        repository.business_relations("customer-1").unwrap().len(),
        1
    );
}
#[test]
fn lifecycle_relation_list_rejects_a_missing_object() {
    let repository = SqliteRepository::in_memory().unwrap();
    let command = LifecycleCommand::Relation(RelationArgs {
        command: RelationCommand::List {
            business_object_id: "missing-object".into(),
        },
    });
    assert_eq!(execute_lifecycle(command, &repository), ExitCode::from(4));
}
#[test]
fn lifecycle_missing_object_is_a_generic_not_found_failure() {
    let repository = SqliteRepository::in_memory().unwrap();
    assert_eq!(
        execute_lifecycle(
            LifecycleCommand::Object(ObjectArgs {
                command: ObjectCommand::Get {
                    id: "missing-object".into()
                }
            }),
            &repository
        ),
        ExitCode::from(4)
    );
}
