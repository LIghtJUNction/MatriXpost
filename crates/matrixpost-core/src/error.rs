//! Typed domain and persistence failures.

use crate::{
    lifecycle::{ApprovalStatus, BusinessObjectStatus},
    types::{Platform, PublishState},
};
use thiserror::Error;

/// Typed failures returned at domain and persistence boundaries.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    #[error("unknown platform: {0}")]
    UnknownPlatform(String),
    #[error("publish title must not be empty")]
    EmptyTitle,
    #[error("account phone must be non-empty and partition must start with persist:")]
    InvalidAccountRoute,
    #[error("short title must not be empty")]
    EmptyShortTitle,
    #[error("task name must not be empty")]
    EmptyTaskName,
    #[error("article content or file is required")]
    EmptyArticleContent,
    #[error("at least one platform target is required")]
    MissingTargets,
    #[error("platform targets must be unique")]
    DuplicateTargets,
    #[error("platform overrides must be unique")]
    DuplicateOverrides,
    #[error("platform override is not among targets")]
    OverrideOutsideTargets,
    #[error("provider platform is not among request targets: {platform:?}")]
    ProviderPlatformNotTarget { platform: Platform },
    #[error("local file path must not be empty")]
    EmptyLocalPath,
    #[error("remote source scheme is not supported: {0}")]
    UnsupportedRemoteScheme(String),
    #[error("scheduled time must use YYYY-MM-DD HH:mm:ss: {0}")]
    InvalidSchedule(String),
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: PublishState,
        to: PublishState,
    },
    #[error("unknown scheduled job: {0}")]
    UnknownJob(String),
    #[error("stale job revision for {id}: expected {expected}, actual {actual}")]
    StaleJobRevision {
        id: String,
        expected: u64,
        actual: u64,
    },
    #[error("concurrent job update: {0}")]
    ConcurrentJobUpdate(String),
    #[error("corrupt durable state: {0}")]
    CorruptState(String),
    #[error("repository mutex was poisoned")]
    RepositoryPoisoned,
    #[error("database error: {0}")]
    Database(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("remote media error: {0}")]
    RemoteMedia(String),
    #[error("remote media content type is not allowed: {0}")]
    DisallowedContentType(String),
    #[error("remote media is too large: {actual} bytes exceeds {limit}")]
    RemoteMediaTooLarge { limit: u64, actual: u64 },
    #[error("lifecycle field must not be empty: {0}")]
    EmptyLifecycleField(&'static str),
    #[error("business object attribute key is invalid: {0}")]
    InvalidBusinessObjectAttributeKey(String),
    #[error("business object attribute key is sensitive and must not be stored: {0}")]
    SensitiveBusinessObjectAttributeKey(String),
    #[error("business object attribute value must not be empty: {0}")]
    InvalidBusinessObjectAttributeValue(String),
    #[error("ledger amount must be positive minor units: {0}")]
    InvalidLedgerAmount(i64),
    #[error("currency must be a three-letter uppercase ISO code: {0}")]
    InvalidCurrency(String),
    #[error("business object already exists: {0}")]
    DuplicateBusinessObjectId(String),
    #[error("business object external id already exists for {kind}: {external_id}")]
    DuplicateBusinessObjectExternalId { kind: String, external_id: String },
    #[error("unknown business object: {0}")]
    UnknownBusinessObject(String),
    #[error("business object must be inserted at revision zero, received: {0}")]
    InvalidInitialBusinessObjectRevision(u64),
    #[error("invalid business object lifecycle transition from {from:?} to {to:?}")]
    InvalidBusinessObjectLifecycleTransition {
        from: BusinessObjectStatus,
        to: BusinessObjectStatus,
    },
    #[error("invalid business object approval transition from {from:?} to {to:?}")]
    InvalidBusinessObjectApprovalTransition {
        from: ApprovalStatus,
        to: ApprovalStatus,
    },
    #[error("business object transition does not change any status: {0}")]
    BusinessObjectTransitionNoop(String),
    #[error("stale business object revision for {id}: expected {expected}, actual {actual}")]
    StaleBusinessObjectRevision {
        id: String,
        expected: u64,
        actual: u64,
    },
    #[error("concurrent business object update: {0}")]
    ConcurrentBusinessObjectUpdate(String),
    #[error("business object revision overflow: {0}")]
    BusinessObjectRevisionOverflow(String),
    #[error("ledger entry already exists: {0}")]
    DuplicateLedgerEntryId(String),
    #[error("business relation already exists: {0}")]
    DuplicateBusinessRelationId(String),
    #[error(
        "business relation already exists from {source_business_object_id} to {target_business_object_id} with type {relation_type}"
    )]
    DuplicateBusinessRelation {
        source_business_object_id: String,
        target_business_object_id: String,
        relation_type: String,
    },
    #[error("business relation cannot reference itself: {0}")]
    BusinessRelationSelfReference(String),
    #[error("business relation attribute key is invalid: {0}")]
    InvalidBusinessRelationAttributeKey(String),
    #[error("business relation attribute key is sensitive and must not be stored: {0}")]
    SensitiveBusinessRelationAttributeKey(String),
    #[error("business relation attribute value must not be empty: {0}")]
    InvalidBusinessRelationAttributeValue(String),
    #[error("unknown publication history record: {0}")]
    UnknownHistoryRecord(String),
    #[error(
        "content attribution already exists for business object {business_object_id} and history {history_id}"
    )]
    DuplicateContentAttribution {
        business_object_id: String,
        history_id: String,
    },
}
impl DomainError {
    pub(crate) fn database(error: rusqlite::Error) -> Self {
        Self::Database(error.to_string())
    }
    pub(crate) fn serialization(error: serde_json::Error) -> Self {
        Self::Serialization(error.to_string())
    }
    pub(crate) fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
