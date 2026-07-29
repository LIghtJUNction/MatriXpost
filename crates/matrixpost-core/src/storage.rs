//! SQLite-backed durable repositories and private persistence helpers.

mod articles;
mod lifecycle;

pub use articles::ArticlePublicationQueue;
pub use lifecycle::LifecycleRepository;

use crate::{
    error::DomainError,
    lifecycle::*,
    runner::{DispatchOutcome, ProviderDispatchReport},
    types::*,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, str::FromStr, sync::Mutex};

/// Durable account/history/job storage boundary.
pub trait Repository: Send + Sync {
    fn save_account(&self, account: &Account) -> Result<(), DomainError>;
    fn accounts(&self) -> Result<Vec<Account>, DomainError>;
    fn save_article_account(&self, account: &ArticleAccount) -> Result<(), DomainError>;
    fn article_accounts(&self) -> Result<Vec<ArticleAccount>, DomainError>;
    fn append_history(&self, record: &HistoryRecord) -> Result<(), DomainError>;
    /// Atomically records the terminal local outcome of an already-completed
    /// provider dispatch. The persisted request is stripped of account routing,
    /// and provider diagnostics are never retained.
    fn record_provider_dispatch_history(
        &self,
        request: &PublishRequest,
        report: &ProviderDispatchReport,
        recorded_at: DateTime<Utc>,
    ) -> Result<HistoryRecord, DomainError>;
    fn history(&self) -> Result<Vec<HistoryRecord>, DomainError>;
    fn insert_job(&self, job: &ScheduledJob) -> Result<(), DomainError>;
    fn job(&self, id: &str) -> Result<Option<ScheduledJob>, DomainError>;
    fn transition_job(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
    /// Atomically completes a claimed job and records its one terminal history
    /// entry. The transition and insert share one SQLite transaction so a
    /// process interruption cannot leave a terminal job without history or a
    /// duplicate retry record. The durable store allocates the history ID in
    /// that transaction; callers cannot predict or select it.
    fn complete_job_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
        detail: Option<&str>,
    ) -> Result<(ScheduledJob, HistoryRecord), DomainError>;
    fn set_config(&self, key: &str, value: &str) -> Result<(), DomainError>;
    fn config(&self, key: &str) -> Result<Option<String>, DomainError>;
    fn delete_config(&self, key: &str) -> Result<bool, DomainError>;
    /// Lists terminal, local article-runner workflow records in chronological order.
    fn article_history(&self) -> Result<Vec<ArticleHistoryRecord>, DomainError>;
}

/// Queue semantics separated from persistence so schedulers are replaceable.
pub trait PublicationQueue: Send + Sync {
    /// The largest number of jobs one scheduler transaction may claim.
    ///
    /// Keeping this bound in the core makes a misconfigured embedding unable
    /// to turn one periodic pass into an unbounded local side-effect burst.
    const MAX_CLAIM_BATCH: usize = 64;

    fn enqueue(
        &self,
        request: &PublishRequest,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
    fn advance(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;

    /// Atomically claims at most `limit` queued, scheduled jobs due on or
    /// before `due_through`.
    ///
    /// Claimed jobs move to [`PublishState::Dispatching`] and have their
    /// revision incremented before this call returns. Drafts, unscheduled
    /// jobs, future jobs, and jobs already claimed by another scheduler are
    /// excluded. `limit` is capped at [`Self::MAX_CLAIM_BATCH`].
    fn claim_due(
        &self,
        due_through: &LocalSchedule,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<ScheduledJob>, DomainError>;

    /// Returns one uncompleted dispatch claim to the due queue.
    ///
    /// This is intentionally narrower than [`Self::advance`]: it only accepts
    /// the exact `Dispatching` revision claimed by a scheduler. It is used
    /// after the local runner may already have accepted work but the durable
    /// terminal transition could not be recorded. The subsequent retry is
    /// therefore **at-least-once** delivery to a local runner, never an
    /// exactly-once remote-platform guarantee.
    fn requeue_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError>;
}

/// SQLite repository with schema migrations and transactional optimistic transitions.
pub struct SqliteRepository {
    connection: Mutex<Connection>,
}
impl SqliteRepository {
    /// Opens (or creates) a database and applies all forward migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DomainError> {
        let connection = Connection::open(path).map_err(DomainError::database)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    /// Opens an in-memory repository for deterministic tests and embedded use.
    pub fn in_memory() -> Result<Self, DomainError> {
        let connection = Connection::open_in_memory().map_err(DomainError::database)?;
        Self::migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub(crate) fn migrate(connection: &Connection) -> Result<(), DomainError> {
        connection.execute_batch("PRAGMA foreign_keys=ON; BEGIN; CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY); CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE IF NOT EXISTS article_accounts (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, display_name TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE IF NOT EXISTS history (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, due_at TEXT, revision INTEGER NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS job_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); INSERT OR IGNORE INTO schema_migrations(version) VALUES (2); INSERT OR IGNORE INTO schema_migrations(version) VALUES (3); COMMIT;").map_err(DomainError::database)?;
        let version_four: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=4)",
                [],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !version_four {
            connection.execute_batch("BEGIN; ALTER TABLE accounts ADD COLUMN phone TEXT NOT NULL DEFAULT ''; ALTER TABLE accounts ADD COLUMN partition TEXT NOT NULL DEFAULT ''; ALTER TABLE article_accounts ADD COLUMN phone TEXT NOT NULL DEFAULT ''; ALTER TABLE article_accounts ADD COLUMN partition TEXT NOT NULL DEFAULT ''; INSERT INTO schema_migrations(version) VALUES (4); COMMIT;").map_err(DomainError::database)?;
        }
        let version_five: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=5)",
                [],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !version_five {
            connection.execute_batch("BEGIN; CREATE TABLE business_objects (id TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL, external_id TEXT, display_name TEXT NOT NULL, lifecycle_status TEXT NOT NULL, approval_status TEXT NOT NULL, revision INTEGER NOT NULL, attributes_json TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL); CREATE UNIQUE INDEX business_objects_kind_external_id_unique ON business_objects(kind, external_id) WHERE external_id IS NOT NULL; CREATE TABLE ledger_entries (id TEXT PRIMARY KEY NOT NULL, business_object_id TEXT NOT NULL REFERENCES business_objects(id), direction TEXT NOT NULL, category TEXT NOT NULL, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL, occurred_at TEXT NOT NULL, approval_status TEXT NOT NULL, counterparty TEXT, reference TEXT, description TEXT, created_at TEXT NOT NULL); CREATE TABLE content_attributions (business_object_id TEXT NOT NULL REFERENCES business_objects(id), history_id TEXT NOT NULL REFERENCES history(id), created_at TEXT NOT NULL, PRIMARY KEY(business_object_id, history_id)); INSERT INTO schema_migrations(version) VALUES (5); COMMIT;").map_err(DomainError::database)?;
        }
        let version_six: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=6)",
                [],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !version_six {
            connection.execute_batch("BEGIN; CREATE TABLE business_relations (id TEXT PRIMARY KEY NOT NULL, source_business_object_id TEXT NOT NULL REFERENCES business_objects(id), target_business_object_id TEXT NOT NULL REFERENCES business_objects(id), relation_type TEXT NOT NULL, attributes_json TEXT NOT NULL, created_at TEXT NOT NULL, UNIQUE(source_business_object_id, target_business_object_id, relation_type)); INSERT INTO schema_migrations(version) VALUES (6); COMMIT;").map_err(DomainError::database)?;
        }
        let version_seven: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=7)",
                [],
                |row| row.get(0),
            )
            .map_err(DomainError::database)?;
        if !version_seven {
            connection.execute_batch("BEGIN; CREATE TABLE history_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); INSERT INTO schema_migrations(version) VALUES (7); COMMIT;").map_err(DomainError::database)?;
        }
        articles::migrate(connection)?;
        Ok(())
    }
    pub(crate) fn locked(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DomainError> {
        self.connection
            .lock()
            .map_err(|_| DomainError::RepositoryPoisoned)
    }
    fn allocate_job_id(&self) -> Result<String, DomainError> {
        let connection = self.locked()?;
        connection
            .execute("INSERT INTO job_sequence DEFAULT VALUES", [])
            .map_err(DomainError::database)?;
        Ok(format!("job-{}", connection.last_insert_rowid()))
    }
}
impl Repository for SqliteRepository {
    fn save_account(&self, account: &Account) -> Result<(), DomainError> {
        let connection = self.locked()?;
        validate_account_route(&account.phone, &account.partition)?;
        connection.execute("INSERT INTO accounts(id, platform, display_name, status, phone, partition) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET platform=excluded.platform, display_name=excluded.display_name, status=excluded.status, phone=excluded.phone, partition=excluded.partition", params![account.id, account.platform.as_str(), account.display_name, account_status_db(account.status), account.phone, account.partition]).map_err(DomainError::database)?;
        Ok(())
    }
    fn accounts(&self) -> Result<Vec<Account>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection
            .prepare("SELECT id, platform, display_name, status, phone, partition FROM accounts ORDER BY id")
            .map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, platform, display_name, status, phone, partition) =
                    row.map_err(DomainError::database)?;
                Ok(Account {
                    id,
                    platform: Platform::from_str(&platform)?,
                    display_name,
                    status: account_status_from_db(&status)?,
                    phone,
                    partition,
                })
            })
            .collect()
    }
    fn save_article_account(&self, account: &ArticleAccount) -> Result<(), DomainError> {
        let connection = self.locked()?;
        validate_account_route(&account.phone, &account.partition)?;
        connection.execute("INSERT INTO article_accounts(id, platform, display_name, status, phone, partition) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET platform=excluded.platform, display_name=excluded.display_name, status=excluded.status, phone=excluded.phone, partition=excluded.partition", params![account.id, article_platform_db(account.platform), account.display_name, article_account_status_db(account.status), account.phone, account.partition]).map_err(DomainError::database)?;
        Ok(())
    }
    fn article_accounts(&self) -> Result<Vec<ArticleAccount>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection
            .prepare("SELECT id, platform, display_name, status, phone, partition FROM article_accounts ORDER BY id")
            .map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, platform, display_name, status, phone, partition) =
                    row.map_err(DomainError::database)?;
                Ok(ArticleAccount {
                    id,
                    platform: article_platform_from_db(&platform)?,
                    display_name,
                    status: article_account_status_from_db(&status)?,
                    phone,
                    partition,
                })
            })
            .collect()
    }
    fn append_history(&self, record: &HistoryRecord) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO history(id, request_json, state, recorded_at, detail) VALUES (?1, ?2, ?3, ?4, ?5)", params![record.id, json(&record.request)?, record.state.db(), record.recorded_at.to_rfc3339(), record.detail]).map_err(DomainError::database)?;
        Ok(())
    }
    fn record_provider_dispatch_history(
        &self,
        request: &PublishRequest,
        report: &ProviderDispatchReport,
        recorded_at: DateTime<Utc>,
    ) -> Result<HistoryRecord, DomainError> {
        let (state, detail) = provider_dispatch_history_outcome(report);
        let safe_request = request.runner_safe();
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let record = insert_terminal_history(
            &transaction,
            "dispatch-history",
            &safe_request,
            state,
            recorded_at,
            Some(detail),
        )?;
        transaction.commit().map_err(DomainError::database)?;
        Ok(record)
    }
    fn history(&self) -> Result<Vec<HistoryRecord>, DomainError> {
        let connection = self.locked()?;
        let mut statement = connection.prepare("SELECT id, request_json, state, recorded_at, detail FROM history ORDER BY recorded_at, id").map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| {
                let (id, request, state, time, detail) = row.map_err(DomainError::database)?;
                Ok(HistoryRecord {
                    id,
                    request: from_json(&request)?,
                    state: PublishState::from_db(&state)?,
                    recorded_at: parse_time(&time)?,
                    detail,
                })
            })
            .collect()
    }
    fn insert_job(&self, job: &ScheduledJob) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO jobs(id, request_json, state, due_at, revision, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![job.id, json(&job.request)?, job.state.db(), job.due_at.as_ref().map(|value| &value.0), job.revision, job.updated_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(())
    }
    fn job(&self, id: &str) -> Result<Option<ScheduledJob>, DomainError> {
        let connection = self.locked()?;
        load_job(&connection, id)
    }
    fn transition_job(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current =
            load_job_tx(&transaction, id)?.ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
        current.state.transition(next)?;
        if current.revision != expected_revision {
            return Err(DomainError::StaleJobRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let changed = transaction
            .execute(
                "UPDATE jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND revision=?5",
                params![
                    next.db(),
                    expected_revision + 1,
                    updated_at.to_rfc3339(),
                    id,
                    expected_revision
                ],
            )
            .map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(ScheduledJob {
            state: next,
            revision: expected_revision + 1,
            updated_at,
            ..current
        })
    }
    fn complete_job_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
        detail: Option<&str>,
    ) -> Result<(ScheduledJob, HistoryRecord), DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current =
            load_job_tx(&transaction, id)?.ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
        current.state.transition(next)?;
        if current.revision != expected_revision {
            return Err(DomainError::StaleJobRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let changed = transaction
            .execute(
                "UPDATE jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND revision=?5",
                params![
                    next.db(),
                    expected_revision + 1,
                    updated_at.to_rfc3339(),
                    id,
                    expected_revision
                ],
            )
            .map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        let history = insert_terminal_history(
            &transaction,
            "scheduled-history",
            &current.request.runner_safe(),
            next,
            updated_at,
            detail,
        )?;
        transaction.commit().map_err(DomainError::database)?;
        Ok((
            ScheduledJob {
                state: next,
                revision: expected_revision + 1,
                updated_at,
                ..current
            },
            history,
        ))
    }
    fn set_config(&self, key: &str, value: &str) -> Result<(), DomainError> {
        let connection = self.locked()?;
        connection.execute("INSERT INTO config(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value]).map_err(DomainError::database)?;
        Ok(())
    }
    fn config(&self, key: &str) -> Result<Option<String>, DomainError> {
        let connection = self.locked()?;
        connection
            .query_row("SELECT value FROM config WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(DomainError::database)
    }
    fn delete_config(&self, key: &str) -> Result<bool, DomainError> {
        let connection = self.locked()?;
        Ok(connection
            .execute("DELETE FROM config WHERE key=?1", [key])
            .map_err(DomainError::database)?
            > 0)
    }
    fn article_history(&self) -> Result<Vec<ArticleHistoryRecord>, DomainError> {
        articles::history(self)
    }
}

impl PublicationQueue for SqliteRepository {
    fn enqueue(
        &self,
        request: &PublishRequest,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        request.validate()?;
        let id = self.allocate_job_id()?;
        let state = if request.draft {
            PublishState::Draft
        } else {
            PublishState::Queued
        };
        let job = ScheduledJob {
            id,
            request: request.runner_safe(),
            state,
            due_at: request.scheduled_at.clone(),
            revision: 0,
            updated_at: now,
        };
        self.insert_job(&job)?;
        Ok(job)
    }
    fn advance(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        self.transition_job(id, expected_revision, next, now)
    }

    fn claim_due(
        &self,
        due_through: &LocalSchedule,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<ScheduledJob>, DomainError> {
        let limit = limit.min(<Self as PublicationQueue>::MAX_CLAIM_BATCH);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut connection = self.locked()?;
        // An IMMEDIATE transaction serializes competing repository instances
        // before either can observe a queued candidate. The subsequent
        // revision/state predicate remains a defensive guard against an
        // unexpected out-of-band writer.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(DomainError::database)?;
        let candidates = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, request_json, state, due_at, revision, updated_at \
                     FROM jobs \
                     WHERE state='queued' AND due_at IS NOT NULL AND due_at <= ?1 \
                     ORDER BY due_at, id LIMIT ?2",
                )
                .map_err(DomainError::database)?;
            statement
                .query_map(params![due_through.0, limit as i64], row_to_job)
                .map_err(DomainError::database)?
                .map(|row| row.map_err(DomainError::database)?)
                .collect::<Result<Vec<_>, DomainError>>()?
        };

        let mut claimed = Vec::with_capacity(candidates.len());
        for current in candidates {
            let changed = transaction
                .execute(
                    "UPDATE jobs SET state=?1, revision=?2, updated_at=?3 \
                     WHERE id=?4 AND state='queued' AND revision=?5",
                    params![
                        PublishState::Dispatching.db(),
                        current.revision + 1,
                        now.to_rfc3339(),
                        current.id,
                        current.revision,
                    ],
                )
                .map_err(DomainError::database)?;
            if changed != 1 {
                return Err(DomainError::ConcurrentJobUpdate(current.id));
            }
            claimed.push(ScheduledJob {
                state: PublishState::Dispatching,
                revision: current.revision + 1,
                updated_at: now,
                ..current
            });
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(claimed)
    }

    fn requeue_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: DateTime<Utc>,
    ) -> Result<ScheduledJob, DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current =
            load_job_tx(&transaction, id)?.ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
        if current.state != PublishState::Dispatching {
            return Err(DomainError::InvalidStateTransition {
                from: current.state,
                to: PublishState::Queued,
            });
        }
        if current.revision != expected_revision {
            return Err(DomainError::StaleJobRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let changed = transaction
            .execute(
                "UPDATE jobs SET state=?1, revision=?2, updated_at=?3 \
                 WHERE id=?4 AND state=?5 AND revision=?6",
                params![
                    PublishState::Queued.db(),
                    expected_revision + 1,
                    now.to_rfc3339(),
                    id,
                    PublishState::Dispatching.db(),
                    expected_revision,
                ],
            )
            .map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(ScheduledJob {
            state: PublishState::Queued,
            revision: expected_revision + 1,
            updated_at: now,
            ..current
        })
    }
}

pub(crate) fn json<T: Serialize>(value: &T) -> Result<String, DomainError> {
    serde_json::to_string(value).map_err(DomainError::serialization)
}
pub(crate) fn from_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, DomainError> {
    serde_json::from_str(value).map_err(DomainError::serialization)
}
pub(crate) fn parse_time(value: &str) -> Result<DateTime<Utc>, DomainError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| DomainError::CorruptState(value.to_owned()))
}
fn account_status_db(value: AccountStatus) -> &'static str {
    match value {
        AccountStatus::LoggedIn => "logged_in",
        AccountStatus::Expired => "expired",
        AccountStatus::LoggedOut => "logged_out",
        AccountStatus::Unavailable => "unavailable",
    }
}
fn account_status_from_db(value: &str) -> Result<AccountStatus, DomainError> {
    match value {
        "logged_in" => Ok(AccountStatus::LoggedIn),
        "expired" => Ok(AccountStatus::Expired),
        "logged_out" => Ok(AccountStatus::LoggedOut),
        "unavailable" => Ok(AccountStatus::Unavailable),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn article_platform_db(value: ArticlePlatform) -> &'static str {
    match value {
        ArticlePlatform::Juejin => "juejin",
    }
}
fn article_platform_from_db(value: &str) -> Result<ArticlePlatform, DomainError> {
    match value {
        "juejin" => Ok(ArticlePlatform::Juejin),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn article_account_status_db(value: ArticleAccountStatus) -> &'static str {
    match value {
        ArticleAccountStatus::LoggedIn => "logged_in",
        ArticleAccountStatus::Expired => "expired",
        ArticleAccountStatus::LoggedOut => "logged_out",
        ArticleAccountStatus::Unavailable => "unavailable",
    }
}
fn article_account_status_from_db(value: &str) -> Result<ArticleAccountStatus, DomainError> {
    match value {
        "logged_in" => Ok(ArticleAccountStatus::LoggedIn),
        "expired" => Ok(ArticleAccountStatus::Expired),
        "logged_out" => Ok(ArticleAccountStatus::LoggedOut),
        "unavailable" => Ok(ArticleAccountStatus::Unavailable),
        _ => Err(DomainError::CorruptState(value.to_owned())),
    }
}
fn validate_account_route(phone: &str, partition: &str) -> Result<(), DomainError> {
    if phone.trim().is_empty() || partition.trim().is_empty() || !partition.starts_with("persist:")
    {
        return Err(DomainError::InvalidAccountRoute);
    }
    Ok(())
}
fn load_job(connection: &Connection, id: &str) -> Result<Option<ScheduledJob>, DomainError> {
    connection
        .query_row(
            "SELECT id, request_json, state, due_at, revision, updated_at FROM jobs WHERE id=?1",
            [id],
            row_to_job,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}
fn load_job_tx(
    transaction: &Transaction<'_>,
    id: &str,
) -> Result<Option<ScheduledJob>, DomainError> {
    transaction
        .query_row(
            "SELECT id, request_json, state, due_at, revision, updated_at FROM jobs WHERE id=?1",
            [id],
            row_to_job,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}

const LOCAL_DISPATCH_COMPLETED: &str =
    "local provider workflow completed; remote platform processing is not confirmed";
const LOCAL_DISPATCH_UNAVAILABLE: &str =
    "all local providers were unavailable; no remote provider workflow was attempted";
const LOCAL_DISPATCH_INCOMPLETE: &str =
    "local provider workflow was incomplete; remote platform processing is not confirmed";

fn provider_dispatch_history_outcome(
    report: &ProviderDispatchReport,
) -> (PublishState, &'static str) {
    if !report.outcomes.is_empty()
        && report
            .outcomes
            .values()
            .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. }))
    {
        return (PublishState::Published, LOCAL_DISPATCH_COMPLETED);
    }
    if !report.outcomes.is_empty()
        && report
            .outcomes
            .values()
            .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. }))
    {
        return (PublishState::Unavailable, LOCAL_DISPATCH_UNAVAILABLE);
    }
    (PublishState::Failed, LOCAL_DISPATCH_INCOMPLETE)
}

/// Allocates and persists terminal history in the caller's transaction. The
/// private sequence is durable across restarts. Although ordinary history
/// import may use arbitrary IDs, a defensive conflict retry makes a generated
/// ID collision-free as well.
fn insert_terminal_history(
    transaction: &Transaction<'_>,
    id_prefix: &str,
    request: &PublishRequest,
    state: PublishState,
    recorded_at: DateTime<Utc>,
    detail: Option<&str>,
) -> Result<HistoryRecord, DomainError> {
    loop {
        transaction
            .execute("INSERT INTO history_sequence DEFAULT VALUES", [])
            .map_err(DomainError::database)?;
        let record = HistoryRecord {
            id: format!("{id_prefix}-{}", transaction.last_insert_rowid()),
            request: request.clone(),
            state,
            recorded_at,
            detail: detail.map(str::to_owned),
        };
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO history(id, request_json, state, recorded_at, detail) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.id,
                    json(&record.request)?,
                    record.state.db(),
                    record.recorded_at.to_rfc3339(),
                    record.detail,
                ],
            )
            .map_err(DomainError::database)?;
        if inserted == 1 {
            return Ok(record);
        }
    }
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ScheduledJob, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let request = row.get::<_, String>(1)?;
    let state = row.get::<_, String>(2)?;
    let due_at = row.get::<_, Option<String>>(3)?;
    let revision = row.get::<_, u64>(4)?;
    let updated = row.get::<_, String>(5)?;
    Ok((|| {
        Ok(ScheduledJob {
            id,
            request: from_json(&request)?,
            state: PublishState::from_db(&state)?,
            due_at: due_at.as_deref().map(LocalSchedule::parse).transpose()?,
            revision,
            updated_at: parse_time(&updated)?,
        })
    })())
}
