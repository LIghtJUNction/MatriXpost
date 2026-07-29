use std::process::ExitCode;

use chrono::Utc;
use matrixpost_core::{
    BusinessObject, BusinessRelation, ContentAttribution, LedgerEntry, LifecycleRepository,
};

use crate::{
    args::{AttributionCommand, LedgerCommand, LifecycleCommand, ObjectCommand, RelationCommand},
    output::emit,
    query::parse_attributes,
};

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}
fn require_optional_non_empty(field: &str, value: &Option<String>) -> Result<(), String> {
    if let Some(value) = value {
        require_non_empty(field, value)?;
    }
    Ok(())
}
fn lifecycle_input_error(error: String) -> ExitCode {
    emit(2, serde_json::Value::Null, Some(&error))
}
pub(crate) fn lifecycle_repository_error(error: impl ToString) -> ExitCode {
    let error = error.to_string();
    emit(4, serde_json::Value::Null, Some(&error))
}
fn object_or_not_found(
    repository: &impl LifecycleRepository,
    id: &str,
) -> Result<BusinessObject, ExitCode> {
    require_non_empty("object id", id).map_err(lifecycle_input_error)?;
    match repository.business_object(id) {
        Ok(Some(object)) => Ok(object),
        Ok(None) => Err(emit(
            4,
            serde_json::Value::Null,
            Some("business object was not found"),
        )),
        Err(error) => Err(lifecycle_repository_error(error)),
    }
}

pub(crate) fn execute_lifecycle(
    command: LifecycleCommand,
    repository: &impl LifecycleRepository,
) -> ExitCode {
    match command {
        LifecycleCommand::Objects => match repository.business_objects() {
            Ok(objects) => emit(0, serde_json::json!({ "objects": objects }), None),
            Err(error) => lifecycle_repository_error(error),
        },
        LifecycleCommand::Object(args) => match args.command {
            ObjectCommand::Get { id } => match object_or_not_found(repository, &id) {
                Ok(object) => emit(0, serde_json::json!({ "object": object }), None),
                Err(exit_code) => exit_code,
            },
            ObjectCommand::Create(args) => {
                for (field, value) in [
                    ("object id", &args.id),
                    ("object kind", &args.kind),
                    ("object display name", &args.display_name),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                if let Err(error) =
                    require_optional_non_empty("object external id", &args.external_id)
                {
                    return lifecycle_input_error(error);
                }
                let attributes = match parse_attributes(args.attributes) {
                    Ok(attributes) => attributes,
                    Err(error) => return lifecycle_input_error(error),
                };
                let now = Utc::now();
                let object = BusinessObject {
                    id: args.id,
                    kind: args.kind,
                    external_id: args.external_id,
                    display_name: args.display_name,
                    lifecycle_status: args.lifecycle_status,
                    approval_status: args.approval_status,
                    revision: 0,
                    attributes,
                    created_at: now,
                    updated_at: now,
                };
                match repository.insert_business_object(&object) {
                    Ok(()) => emit(0, serde_json::json!({ "object": object }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Ledger(args) => match args.command {
            LedgerCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.ledger_entries(&business_object_id) {
                    Ok(entries) => emit(0, serde_json::json!({ "ledger_entries": entries }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            LedgerCommand::Add(args) => {
                for (field, value) in [
                    ("ledger entry id", &args.id),
                    ("object id", &args.business_object_id),
                    ("ledger category", &args.category),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                for (field, value) in [
                    ("ledger counterparty", &args.counterparty),
                    ("ledger reference", &args.reference),
                    ("ledger description", &args.description),
                ] {
                    if let Err(error) = require_optional_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let now = Utc::now();
                let entry = LedgerEntry {
                    id: args.id,
                    business_object_id: args.business_object_id,
                    direction: args.direction,
                    category: args.category,
                    amount_minor: args.amount_minor,
                    currency: args.currency,
                    occurred_at: args.occurred_at.unwrap_or(now),
                    approval_status: args.approval_status,
                    counterparty: args.counterparty,
                    reference: args.reference,
                    description: args.description,
                    created_at: now,
                };
                match repository.insert_ledger_entry(&entry) {
                    Ok(()) => emit(0, serde_json::json!({ "ledger_entry": entry }), None),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Attribution(args) => match args.command {
            AttributionCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.content_attributions(&business_object_id) {
                    Ok(attributions) => emit(
                        0,
                        serde_json::json!({ "content_attributions": attributions }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            AttributionCommand::Add(args) => {
                for (field, value) in [
                    ("object id", &args.business_object_id),
                    ("history id", &args.history_id),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let attribution = ContentAttribution {
                    business_object_id: args.business_object_id,
                    history_id: args.history_id,
                    created_at: args.created_at.unwrap_or_else(Utc::now),
                };
                match repository.insert_content_attribution(&attribution) {
                    Ok(()) => emit(
                        0,
                        serde_json::json!({ "content_attribution": attribution }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Relation(args) => match args.command {
            RelationCommand::List { business_object_id } => {
                if let Err(exit_code) = object_or_not_found(repository, &business_object_id) {
                    return exit_code;
                }
                match repository.business_relations(&business_object_id) {
                    Ok(relations) => emit(
                        0,
                        serde_json::json!({ "business_relations": relations }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
            RelationCommand::Add(args) => {
                for (field, value) in [
                    ("business relation id", &args.id),
                    ("source object id", &args.source_business_object_id),
                    ("target object id", &args.target_business_object_id),
                    ("business relation type", &args.relation_type),
                ] {
                    if let Err(error) = require_non_empty(field, value) {
                        return lifecycle_input_error(error);
                    }
                }
                let attributes = match parse_attributes(args.attributes) {
                    Ok(attributes) => attributes,
                    Err(error) => return lifecycle_input_error(error),
                };
                let relation = BusinessRelation {
                    id: args.id,
                    source_business_object_id: args.source_business_object_id,
                    target_business_object_id: args.target_business_object_id,
                    relation_type: args.relation_type,
                    attributes,
                    created_at: Utc::now(),
                };
                match repository.insert_business_relation(&relation) {
                    Ok(()) => emit(
                        0,
                        serde_json::json!({ "business_relation": relation }),
                        None,
                    ),
                    Err(error) => lifecycle_repository_error(error),
                }
            }
        },
        LifecycleCommand::Transition(args) => {
            if let Err(error) = require_non_empty("object id", &args.id) {
                return lifecycle_input_error(error);
            }
            match repository.transition_business_object(
                &args.id,
                args.expected_revision,
                args.lifecycle_status,
                args.approval_status,
                args.updated_at.unwrap_or_else(Utc::now),
            ) {
                Ok(object) => emit(0, serde_json::json!({ "object": object }), None),
                Err(error) => lifecycle_repository_error(error),
            }
        }
    }
}
