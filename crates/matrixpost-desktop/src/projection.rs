use matrixpost_core::{
    Account, AccountStatus, ApprovalStatus, ArticleAccount, ArticleAccountStatus, BusinessObject,
    BusinessObjectStatus, BusinessRelation, ContentAttribution, HistoryRecord, LedgerDirection,
    LedgerEntry, Platform, PublishState,
};

use crate::{
    AccountEntry, ArticleAccountEntry, DesktopError, HistoryEntry, LifecycleBusinessRelationEntry,
    LifecycleContentAttributionEntry, LifecycleLedgerEntry, LifecycleObjectEntry,
};

impl From<BusinessObject> for LifecycleObjectEntry {
    fn from(object: BusinessObject) -> Self {
        Self {
            id: object.id,
            kind: object.kind,
            external_id: object.external_id,
            display_name: object.display_name,
            lifecycle_status: lifecycle_status_label(object.lifecycle_status),
            approval_status: approval_status_label(object.approval_status),
            revision: object.revision,
            attributes: object.attributes,
            created_at: object.created_at.to_rfc3339(),
            updated_at: object.updated_at.to_rfc3339(),
        }
    }
}

impl From<LedgerEntry> for LifecycleLedgerEntry {
    fn from(entry: LedgerEntry) -> Self {
        Self {
            id: entry.id,
            business_object_id: entry.business_object_id,
            direction: ledger_direction_label(entry.direction),
            category: entry.category,
            amount_minor: entry.amount_minor,
            currency: entry.currency,
            occurred_at: entry.occurred_at.to_rfc3339(),
            approval_status: approval_status_label(entry.approval_status),
            counterparty: entry.counterparty,
            reference: entry.reference,
            description: entry.description,
            created_at: entry.created_at.to_rfc3339(),
        }
    }
}

impl From<ContentAttribution> for LifecycleContentAttributionEntry {
    fn from(attribution: ContentAttribution) -> Self {
        Self {
            business_object_id: attribution.business_object_id,
            history_id: attribution.history_id,
            created_at: attribution.created_at.to_rfc3339(),
        }
    }
}

impl From<BusinessRelation> for LifecycleBusinessRelationEntry {
    fn from(relation: BusinessRelation) -> Self {
        Self {
            id: relation.id,
            source_business_object_id: relation.source_business_object_id,
            target_business_object_id: relation.target_business_object_id,
            relation_type: relation.relation_type,
            attributes: relation.attributes,
            created_at: relation.created_at.to_rfc3339(),
        }
    }
}

impl From<HistoryRecord> for HistoryEntry {
    fn from(record: HistoryRecord) -> Self {
        let scheduled =
            record.state == PublishState::Queued && record.request.scheduled_at.is_some();
        Self {
            id: record.id,
            state: publish_state_label(record.state),
            recorded_at: record.recorded_at.to_rfc3339(),
            title: record.request.title,
            targets: record
                .request
                .targets
                .into_iter()
                .map(|platform| platform.as_str().to_owned())
                .collect(),
            draft: record.request.draft,
            scheduled,
        }
    }
}

impl From<ArticleAccount> for ArticleAccountEntry {
    fn from(account: ArticleAccount) -> Self {
        Self {
            id: account.id,
            display_name: account.display_name,
            status: article_account_status_label(account.status),
        }
    }
}

impl From<Account> for AccountEntry {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            platform: account.platform.as_str(),
            display_name: account.display_name,
            status: account_status_label(account.status),
        }
    }
}

const fn account_status_label(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::LoggedIn => "logged_in",
        AccountStatus::Expired => "expired",
        AccountStatus::LoggedOut => "logged_out",
        AccountStatus::Unavailable => "unavailable",
    }
}

pub(crate) fn article_account_status(value: &str) -> Result<ArticleAccountStatus, DesktopError> {
    match value.trim() {
        "logged_in" => Ok(ArticleAccountStatus::LoggedIn),
        "expired" => Ok(ArticleAccountStatus::Expired),
        "logged_out" => Ok(ArticleAccountStatus::LoggedOut),
        "unavailable" => Ok(ArticleAccountStatus::Unavailable),
        value => Err(DesktopError::InvalidRequest(format!(
            "unknown article account status: {value}"
        ))),
    }
}

pub(crate) const fn article_account_status_label(status: ArticleAccountStatus) -> &'static str {
    match status {
        ArticleAccountStatus::LoggedIn => "logged_in",
        ArticleAccountStatus::Expired => "expired",
        ArticleAccountStatus::LoggedOut => "logged_out",
        ArticleAccountStatus::Unavailable => "unavailable",
    }
}

const fn publish_state_label(state: PublishState) -> &'static str {
    match state {
        PublishState::Draft => "draft",
        PublishState::Queued => "queued",
        PublishState::Dispatching => "dispatching",
        PublishState::Published => "published",
        PublishState::Failed => "failed",
        PublishState::Unavailable => "unavailable",
    }
}

const fn lifecycle_status_label(status: BusinessObjectStatus) -> &'static str {
    match status {
        BusinessObjectStatus::Draft => "draft",
        BusinessObjectStatus::Active => "active",
        BusinessObjectStatus::Completed => "completed",
        BusinessObjectStatus::Archived => "archived",
    }
}

const fn approval_status_label(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    }
}

const fn ledger_direction_label(direction: LedgerDirection) -> &'static str {
    match direction {
        LedgerDirection::Expense => "expense",
        LedgerDirection::Revenue => "revenue",
    }
}

pub(crate) fn account_id(platform: Platform, display_name: &str) -> String {
    format!("{}-{}", platform.as_str(), account_slug(display_name))
}

pub(crate) fn article_account_id(display_name: &str) -> String {
    format!("juejin-{}", account_slug(display_name))
}

fn account_slug(display_name: &str) -> String {
    let slug = display_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "account".into()
    } else {
        slug.into()
    }
}
