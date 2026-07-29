//! Persistence for generic business-object lifecycle data.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{SqliteRepository, from_json, json, parse_time};
use crate::{error::DomainError, lifecycle::*, runner::contains_credential_like_term};

/// Persistence boundary for generic business-object lifecycle data.
///
/// Ledger entries are append-only: corrections must be represented by a new
/// entry, and this trait intentionally provides no update or delete operation.
pub trait LifecycleRepository: Send + Sync {
    /// Inserts a new generic business object after validating its identifiers and attributes.
    fn insert_business_object(&self, object: &BusinessObject) -> Result<(), DomainError>;
    /// Returns a business object by its stable identifier.
    fn business_object(&self, id: &str) -> Result<Option<BusinessObject>, DomainError>;
    /// Lists all business objects in creation order.
    fn business_objects(&self) -> Result<Vec<BusinessObject>, DomainError>;
    /// Atomically changes one or both controlled object states when expected revision matches.
    fn transition_business_object(
        &self,
        id: &str,
        expected_revision: u64,
        next_lifecycle_status: BusinessObjectStatus,
        next_approval_status: ApprovalStatus,
        updated_at: DateTime<Utc>,
    ) -> Result<BusinessObject, DomainError>;
    /// Appends an immutable ledger entry for an existing business object.
    fn insert_ledger_entry(&self, entry: &LedgerEntry) -> Result<(), DomainError>;
    /// Lists a business object's ledger entries in occurrence order.
    fn ledger_entries(&self, business_object_id: &str) -> Result<Vec<LedgerEntry>, DomainError>;
    /// Links a business object to an existing publication-history record.
    fn insert_content_attribution(
        &self,
        attribution: &ContentAttribution,
    ) -> Result<(), DomainError>;
    /// Lists publication-history links for a business object.
    fn content_attributions(
        &self,
        business_object_id: &str,
    ) -> Result<Vec<ContentAttribution>, DomainError>;
    /// Inserts an immutable directed relation between two existing objects.
    fn insert_business_relation(&self, relation: &BusinessRelation) -> Result<(), DomainError>;
    /// Lists both incoming and outgoing relations for an existing object.
    fn business_relations(
        &self,
        business_object_id: &str,
    ) -> Result<Vec<BusinessRelation>, DomainError>;
}

impl LifecycleRepository for SqliteRepository {
    fn insert_business_object(&self, object: &BusinessObject) -> Result<(), DomainError> {
        validate_business_object(object)?;
        let connection = self.locked()?;
        let duplicate_id: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM business_objects WHERE id=?1)",
                [&object.id],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if duplicate_id {
            return Err(DomainError::DuplicateBusinessObjectId(object.id.clone()));
        }
        if let Some(external_id) = &object.external_id {
            let duplicate_external_id: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM business_objects WHERE kind=?1 AND external_id=?2)",
                    params![object.kind, external_id],
                    |row| row.get(0),
                )
                .map_err(DomainError::database)?;
            if duplicate_external_id {
                return Err(DomainError::DuplicateBusinessObjectExternalId {
                    kind: object.kind.clone(),
                    external_id: external_id.clone(),
                });
            }
        }
        connection.execute("INSERT INTO business_objects(id, kind, external_id, display_name, lifecycle_status, approval_status, revision, attributes_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", params![object.id, object.kind, object.external_id, object.display_name, object.lifecycle_status.db(), object.approval_status.db(), object.revision, json(&object.attributes)?, object.created_at.to_rfc3339(), object.updated_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }

    fn business_object(&self, id: &str) -> Result<Option<BusinessObject>, DomainError> {
        let connection = self.locked()?;
        load_business_object(&connection, id)
    }

    fn business_objects(&self) -> Result<Vec<BusinessObject>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection
            .prepare("SELECT id, kind, external_id, display_name, lifecycle_status, approval_status, revision, attributes_json, created_at, updated_at FROM business_objects ORDER BY created_at, id")
            .map_err(DomainError::database)?;
        statement
            .query_map([], row_to_business_object)
            .map_err(DomainError::database)?
            .map(|row| row.map_err(DomainError::database)?)
            .collect()
    }

    fn transition_business_object(
        &self,
        id: &str,
        expected_revision: u64,
        next_lifecycle_status: BusinessObjectStatus,
        next_approval_status: ApprovalStatus,
        updated_at: DateTime<Utc>,
    ) -> Result<BusinessObject, DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current = load_business_object_tx(&transaction, id)?
            .ok_or_else(|| DomainError::UnknownBusinessObject(id.to_owned()))?;

        if current.revision != expected_revision {
            return Err(DomainError::StaleBusinessObjectRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let lifecycle_changed = current.lifecycle_status != next_lifecycle_status;
        let approval_changed = current.approval_status != next_approval_status;
        if !lifecycle_changed && !approval_changed {
            return Err(DomainError::BusinessObjectTransitionNoop(id.to_owned()));
        }
        if lifecycle_changed
            && !current
                .lifecycle_status
                .can_transition_to(next_lifecycle_status)
        {
            return Err(DomainError::InvalidBusinessObjectLifecycleTransition {
                from: current.lifecycle_status,
                to: next_lifecycle_status,
            });
        }
        if approval_changed
            && !current
                .approval_status
                .can_transition_to(next_approval_status)
        {
            return Err(DomainError::InvalidBusinessObjectApprovalTransition {
                from: current.approval_status,
                to: next_approval_status,
            });
        }

        let revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| DomainError::BusinessObjectRevisionOverflow(id.to_owned()))?;
        let changed = transaction
            .execute(
                "UPDATE business_objects SET lifecycle_status=?1, approval_status=?2, revision=?3, updated_at=?4 WHERE id=?5 AND revision=?6",
                params![next_lifecycle_status.db(), next_approval_status.db(), revision, updated_at.to_rfc3339(), id, expected_revision],
            )
            .map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentBusinessObjectUpdate(id.to_owned()));
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(BusinessObject {
            lifecycle_status: next_lifecycle_status,
            approval_status: next_approval_status,
            revision,
            updated_at,
            ..current
        })
    }

    fn insert_ledger_entry(&self, entry: &LedgerEntry) -> Result<(), DomainError> {
        validate_ledger_entry(entry)?;
        let connection = self.locked()?;
        if !business_object_exists(&connection, &entry.business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                entry.business_object_id.clone(),
            ));
        }
        let duplicate_id: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ledger_entries WHERE id=?1)",
                [&entry.id],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if duplicate_id {
            return Err(DomainError::DuplicateLedgerEntryId(entry.id.clone()));
        }
        connection.execute("INSERT INTO ledger_entries(id, business_object_id, direction, category, amount_minor, currency, occurred_at, approval_status, counterparty, reference, description, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", params![entry.id, entry.business_object_id, entry.direction.db(), entry.category, entry.amount_minor, entry.currency, entry.occurred_at.to_rfc3339(), entry.approval_status.db(), entry.counterparty, entry.reference, entry.description, entry.created_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }

    fn ledger_entries(&self, business_object_id: &str) -> Result<Vec<LedgerEntry>, DomainError> {
        let connection = self.locked()?;
        if !business_object_exists(&connection, business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                business_object_id.to_owned(),
            ));
        }
        let mut statement = connection
            .prepare("SELECT id, business_object_id, direction, category, amount_minor, currency, occurred_at, approval_status, counterparty, reference, description, created_at FROM ledger_entries WHERE business_object_id=?1 ORDER BY occurred_at, id")
            .map_err(DomainError::database)?;
        statement
            .query_map([business_object_id], row_to_ledger_entry)
            .map_err(DomainError::database)?
            .map(|row| row.map_err(DomainError::database)?)
            .collect()
    }

    fn insert_content_attribution(
        &self,
        attribution: &ContentAttribution,
    ) -> Result<(), DomainError> {
        validate_non_empty(
            "content attribution business object id",
            &attribution.business_object_id,
        )?;
        validate_non_empty("content attribution history id", &attribution.history_id)?;
        let connection = self.locked()?;
        if !business_object_exists(&connection, &attribution.business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                attribution.business_object_id.clone(),
            ));
        }
        let history_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history WHERE id=?1)",
                [&attribution.history_id],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !history_exists {
            return Err(DomainError::UnknownHistoryRecord(
                attribution.history_id.clone(),
            ));
        }
        let duplicate_pair: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM content_attributions WHERE business_object_id=?1 AND history_id=?2)",
                params![attribution.business_object_id, attribution.history_id],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if duplicate_pair {
            return Err(DomainError::DuplicateContentAttribution {
                business_object_id: attribution.business_object_id.clone(),
                history_id: attribution.history_id.clone(),
            });
        }
        connection.execute("INSERT INTO content_attributions(business_object_id, history_id, created_at) VALUES (?1, ?2, ?3)", params![attribution.business_object_id, attribution.history_id, attribution.created_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }

    fn content_attributions(
        &self,
        business_object_id: &str,
    ) -> Result<Vec<ContentAttribution>, DomainError> {
        let connection = self.locked()?;
        if !business_object_exists(&connection, business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                business_object_id.to_owned(),
            ));
        }
        let mut statement = connection
            .prepare("SELECT business_object_id, history_id, created_at FROM content_attributions WHERE business_object_id=?1 ORDER BY created_at, history_id")
            .map_err(DomainError::database)?;
        statement
            .query_map([business_object_id], row_to_content_attribution)
            .map_err(DomainError::database)?
            .map(|row| row.map_err(DomainError::database)?)
            .collect()
    }

    fn insert_business_relation(&self, relation: &BusinessRelation) -> Result<(), DomainError> {
        validate_business_relation(relation)?;
        let connection = self.locked()?;
        if !business_object_exists(&connection, &relation.source_business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                relation.source_business_object_id.clone(),
            ));
        }
        if !business_object_exists(&connection, &relation.target_business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                relation.target_business_object_id.clone(),
            ));
        }
        let duplicate_id: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM business_relations WHERE id=?1)",
                [&relation.id],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if duplicate_id {
            return Err(DomainError::DuplicateBusinessRelationId(
                relation.id.clone(),
            ));
        }
        let duplicate_pair: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM business_relations WHERE source_business_object_id=?1 AND target_business_object_id=?2 AND relation_type=?3)",
                params![relation.source_business_object_id, relation.target_business_object_id, relation.relation_type],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if duplicate_pair {
            return Err(DomainError::DuplicateBusinessRelation {
                source_business_object_id: relation.source_business_object_id.clone(),
                target_business_object_id: relation.target_business_object_id.clone(),
                relation_type: relation.relation_type.clone(),
            });
        }
        connection.execute("INSERT INTO business_relations(id, source_business_object_id, target_business_object_id, relation_type, attributes_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![relation.id, relation.source_business_object_id, relation.target_business_object_id, relation.relation_type, json(&relation.attributes)?, relation.created_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }

    fn business_relations(
        &self,
        business_object_id: &str,
    ) -> Result<Vec<BusinessRelation>, DomainError> {
        let connection = self.locked()?;
        if !business_object_exists(&connection, business_object_id)? {
            return Err(DomainError::UnknownBusinessObject(
                business_object_id.to_owned(),
            ));
        }
        let mut statement = connection
            .prepare("SELECT id, source_business_object_id, target_business_object_id, relation_type, attributes_json, created_at FROM business_relations WHERE source_business_object_id=?1 OR target_business_object_id=?1 ORDER BY created_at, id")
            .map_err(DomainError::database)?;
        statement
            .query_map([business_object_id], row_to_business_relation)
            .map_err(DomainError::database)?
            .map(|row| row.map_err(DomainError::database)?)
            .collect()
    }
}
fn load_business_object(
    connection: &Connection,
    id: &str,
) -> Result<Option<BusinessObject>, DomainError> {
    connection
        .query_row(
            "SELECT id, kind, external_id, display_name, lifecycle_status, approval_status, revision, attributes_json, created_at, updated_at FROM business_objects WHERE id=?1",
            [id],
            row_to_business_object,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}

fn load_business_object_tx(
    transaction: &Transaction<'_>,
    id: &str,
) -> Result<Option<BusinessObject>, DomainError> {
    transaction
        .query_row(
            "SELECT id, kind, external_id, display_name, lifecycle_status, approval_status, revision, attributes_json, created_at, updated_at FROM business_objects WHERE id=?1",
            [id],
            row_to_business_object,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}

fn business_object_exists(connection: &Connection, id: &str) -> Result<bool, DomainError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM business_objects WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(DomainError::database)
}

fn row_to_business_object(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<BusinessObject, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let kind = row.get::<_, String>(1)?;
    let external_id = row.get::<_, Option<String>>(2)?;
    let display_name = row.get::<_, String>(3)?;
    let lifecycle_status = row.get::<_, String>(4)?;
    let approval_status = row.get::<_, String>(5)?;
    let revision = row.get::<_, u64>(6)?;
    let attributes = row.get::<_, String>(7)?;
    let created_at = row.get::<_, String>(8)?;
    let updated_at = row.get::<_, String>(9)?;
    Ok((|| {
        Ok(BusinessObject {
            id,
            kind,
            external_id,
            display_name,
            lifecycle_status: BusinessObjectStatus::from_db(&lifecycle_status)?,
            approval_status: ApprovalStatus::from_db(&approval_status)?,
            revision,
            attributes: from_json(&attributes)?,
            created_at: parse_time(&created_at)?,
            updated_at: parse_time(&updated_at)?,
        })
    })())
}

fn row_to_ledger_entry(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<LedgerEntry, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let business_object_id = row.get::<_, String>(1)?;
    let direction = row.get::<_, String>(2)?;
    let category = row.get::<_, String>(3)?;
    let amount_minor = row.get::<_, i64>(4)?;
    let currency = row.get::<_, String>(5)?;
    let occurred_at = row.get::<_, String>(6)?;
    let approval_status = row.get::<_, String>(7)?;
    let counterparty = row.get::<_, Option<String>>(8)?;
    let reference = row.get::<_, Option<String>>(9)?;
    let description = row.get::<_, Option<String>>(10)?;
    let created_at = row.get::<_, String>(11)?;
    Ok((|| {
        Ok(LedgerEntry {
            id,
            business_object_id,
            direction: LedgerDirection::from_db(&direction)?,
            category,
            amount_minor,
            currency,
            occurred_at: parse_time(&occurred_at)?,
            approval_status: ApprovalStatus::from_db(&approval_status)?,
            counterparty,
            reference,
            description,
            created_at: parse_time(&created_at)?,
        })
    })())
}

fn row_to_content_attribution(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ContentAttribution, DomainError>> {
    let business_object_id = row.get::<_, String>(0)?;
    let history_id = row.get::<_, String>(1)?;
    let created_at = row.get::<_, String>(2)?;
    Ok(
        parse_time(&created_at).map(|created_at| ContentAttribution {
            business_object_id,
            history_id,
            created_at,
        }),
    )
}

fn row_to_business_relation(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<BusinessRelation, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let source_business_object_id = row.get::<_, String>(1)?;
    let target_business_object_id = row.get::<_, String>(2)?;
    let relation_type = row.get::<_, String>(3)?;
    let attributes = row.get::<_, String>(4)?;
    let created_at = row.get::<_, String>(5)?;
    Ok((|| {
        Ok(BusinessRelation {
            id,
            source_business_object_id,
            target_business_object_id,
            relation_type,
            attributes: from_json(&attributes)?,
            created_at: parse_time(&created_at)?,
        })
    })())
}

fn validate_business_object(object: &BusinessObject) -> Result<(), DomainError> {
    validate_non_empty("business object id", &object.id)?;
    validate_non_empty("business object kind", &object.kind)?;
    validate_non_empty("business object display name", &object.display_name)?;
    if object.revision != 0 {
        return Err(DomainError::InvalidInitialBusinessObjectRevision(
            object.revision,
        ));
    }
    if let Some(external_id) = &object.external_id {
        validate_non_empty("business object external id", external_id)?;
    }
    for (key, value) in &object.attributes {
        if key.trim().is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(DomainError::InvalidBusinessObjectAttributeKey(key.clone()));
        }
        if contains_credential_like_term(key) {
            return Err(DomainError::SensitiveBusinessObjectAttributeKey(
                key.clone(),
            ));
        }
        if value.trim().is_empty() {
            return Err(DomainError::InvalidBusinessObjectAttributeValue(
                key.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_ledger_entry(entry: &LedgerEntry) -> Result<(), DomainError> {
    validate_non_empty("ledger entry id", &entry.id)?;
    validate_non_empty("ledger entry business object id", &entry.business_object_id)?;
    validate_non_empty("ledger entry category", &entry.category)?;
    if entry.amount_minor <= 0 {
        return Err(DomainError::InvalidLedgerAmount(entry.amount_minor));
    }
    if entry.currency.len() != 3 || !entry.currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(DomainError::InvalidCurrency(entry.currency.clone()));
    }
    for (name, value) in [
        ("ledger counterparty", entry.counterparty.as_deref()),
        ("ledger reference", entry.reference.as_deref()),
        ("ledger description", entry.description.as_deref()),
    ] {
        if let Some(value) = value {
            validate_non_empty(name, value)?;
        }
    }
    Ok(())
}

fn validate_business_relation(relation: &BusinessRelation) -> Result<(), DomainError> {
    validate_non_empty("business relation id", &relation.id)?;
    validate_non_empty(
        "business relation source business object id",
        &relation.source_business_object_id,
    )?;
    validate_non_empty(
        "business relation target business object id",
        &relation.target_business_object_id,
    )?;
    validate_non_empty("business relation type", &relation.relation_type)?;
    if relation.source_business_object_id == relation.target_business_object_id {
        return Err(DomainError::BusinessRelationSelfReference(
            relation.source_business_object_id.clone(),
        ));
    }
    for (key, value) in &relation.attributes {
        if key.trim().is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(DomainError::InvalidBusinessRelationAttributeKey(
                key.clone(),
            ));
        }
        if contains_credential_like_term(key) {
            return Err(DomainError::SensitiveBusinessRelationAttributeKey(
                key.clone(),
            ));
        }
        if value.trim().is_empty() {
            return Err(DomainError::InvalidBusinessRelationAttributeValue(
                key.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::EmptyLifecycleField(field));
    }
    Ok(())
}
