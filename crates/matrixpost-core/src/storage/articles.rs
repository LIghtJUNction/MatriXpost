//! Durable scheduled-article queue and redacted terminal history.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::{SqliteRepository, from_json, json, parse_time};
use crate::{
    DomainError,
    types::{
        ArticleHistoryRecord, ArticlePlatform, ArticleScheduledJob, LocalSchedule,
        PublishArticleRequest, PublishState,
    },
};

/// Durable queue for scheduled Juejin article work. It is deliberately
/// separate from the video queue so its runner protocol and history stay
/// independent.
pub trait ArticlePublicationQueue: Send + Sync {
    const MAX_CLAIM_BATCH: usize = 64;

    /// Persists only a non-draft request with a valid local due time.
    fn enqueue_article(
        &self,
        request: &PublishArticleRequest,
        now: DateTime<Utc>,
    ) -> Result<ArticleScheduledJob, DomainError>;
    fn claim_due_articles(
        &self,
        due_through: &LocalSchedule,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<ArticleScheduledJob>, DomainError>;
    fn complete_article_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
        detail: Option<&str>,
    ) -> Result<(ArticleScheduledJob, ArticleHistoryRecord), DomainError>;
    fn requeue_article_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: DateTime<Utc>,
    ) -> Result<ArticleScheduledJob, DomainError>;
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), DomainError> {
    let version_eight: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=8)",
            [],
            |row| row.get(0),
        )
        .map_err(DomainError::database)?;
    if !version_eight {
        connection.execute_batch("BEGIN; CREATE TABLE article_jobs (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, due_at TEXT NOT NULL, revision INTEGER NOT NULL, updated_at TEXT NOT NULL); CREATE TABLE article_history (id TEXT PRIMARY KEY NOT NULL, request_json TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); CREATE TABLE article_job_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); CREATE TABLE article_history_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); INSERT INTO schema_migrations(version) VALUES (8); COMMIT;").map_err(DomainError::database)?;
    }

    let version_nine: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=9)",
            [],
            |row| row.get(0),
        )
        .map_err(DomainError::database)?;
    if !version_nine {
        migrate_history_to_redacted_projection(connection)?;
    }
    Ok(())
}

fn migrate_history_to_redacted_projection(connection: &Connection) -> Result<(), DomainError> {
    let legacy = {
        let mut statement = connection
            .prepare(
                "SELECT id, request_json, state, recorded_at FROM article_history ORDER BY recorded_at, id",
            )
            .map_err(DomainError::database)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(DomainError::database)?
            .map(|row| row.map_err(DomainError::database))
            .collect::<Result<Vec<_>, _>>()?
    };

    connection
        .execute_batch("BEGIN; CREATE TABLE article_history_redacted (id TEXT PRIMARY KEY NOT NULL, platform TEXT NOT NULL, title TEXT NOT NULL, state TEXT NOT NULL, recorded_at TEXT NOT NULL, detail TEXT); ")
        .map_err(DomainError::database)?;
    let migration = (|| {
        for (id, request_json, state, recorded_at) in legacy {
            let request: PublishArticleRequest = from_json(&request_json)?;
            let platform = request.article_platform()?;
            let state = PublishState::from_db(&state)?;
            connection
                .execute(
                    "INSERT INTO article_history_redacted(id, platform, title, state, recorded_at, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        id,
                        article_platform_db(platform),
                        request.title,
                        state.db(),
                        recorded_at,
                        fixed_history_detail(state),
                    ],
                )
                .map_err(DomainError::database)?;
        }
        connection.execute_batch("DROP TABLE article_history; ALTER TABLE article_history_redacted RENAME TO article_history; INSERT INTO schema_migrations(version) VALUES (9); COMMIT;").map_err(DomainError::database)
    })();
    if migration.is_err() {
        let _ = connection.execute_batch("ROLLBACK;");
    }
    migration
}

pub(crate) fn history(
    repository: &SqliteRepository,
) -> Result<Vec<ArticleHistoryRecord>, DomainError> {
    let connection = repository.locked()?;
    let mut statement = connection
        .prepare("SELECT id, platform, title, state, recorded_at, detail FROM article_history ORDER BY recorded_at, id")
        .map_err(DomainError::database)?;
    statement
        .query_map([], row_to_article_history)
        .map_err(DomainError::database)?
        .map(|row| row.map_err(DomainError::database)?)
        .collect()
}

impl ArticlePublicationQueue for SqliteRepository {
    fn enqueue_article(
        &self,
        request: &PublishArticleRequest,
        now: DateTime<Utc>,
    ) -> Result<ArticleScheduledJob, DomainError> {
        request.validate()?;
        let due_at = request.scheduled_at.clone().ok_or_else(|| {
            DomainError::InvalidSchedule("scheduled article requires publish_at".into())
        })?;
        let safe = request.runner_safe();
        let connection = self.locked()?;
        connection
            .execute("INSERT INTO article_job_sequence DEFAULT VALUES", [])
            .map_err(DomainError::database)?;
        let job = ArticleScheduledJob {
            id: format!("article-job-{}", connection.last_insert_rowid()),
            request: safe,
            state: PublishState::Queued,
            due_at,
            revision: 0,
            updated_at: now,
        };
        connection.execute("INSERT INTO article_jobs(id, request_json, state, due_at, revision, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![job.id, json(&job.request)?, job.state.db(), job.due_at.0, job.revision, job.updated_at.to_rfc3339()]).map_err(DomainError::database)?;
        Ok(job)
    }

    fn claim_due_articles(
        &self,
        due_through: &LocalSchedule,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<ArticleScheduledJob>, DomainError> {
        let limit = limit.min(<Self as ArticlePublicationQueue>::MAX_CLAIM_BATCH);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut connection = self.locked()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(DomainError::database)?;
        let candidates = {
            let mut statement = transaction.prepare("SELECT id, request_json, state, due_at, revision, updated_at FROM article_jobs WHERE state='queued' AND due_at <= ?1 ORDER BY due_at, id LIMIT ?2").map_err(DomainError::database)?;
            statement
                .query_map(params![due_through.0, limit as i64], row_to_article_job)
                .map_err(DomainError::database)?
                .map(|row| row.map_err(DomainError::database)?)
                .collect::<Result<Vec<_>, DomainError>>()?
        };
        let mut claimed = Vec::with_capacity(candidates.len());
        for current in candidates {
            let changed = transaction.execute("UPDATE article_jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND state='queued' AND revision=?5", params![PublishState::Dispatching.db(), current.revision + 1, now.to_rfc3339(), current.id, current.revision]).map_err(DomainError::database)?;
            if changed != 1 {
                return Err(DomainError::ConcurrentJobUpdate(current.id));
            }
            claimed.push(ArticleScheduledJob {
                state: PublishState::Dispatching,
                revision: current.revision + 1,
                updated_at: now,
                ..current
            });
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(claimed)
    }

    fn complete_article_with_history(
        &self,
        id: &str,
        expected_revision: u64,
        next: PublishState,
        updated_at: DateTime<Utc>,
        _detail: Option<&str>,
    ) -> Result<(ArticleScheduledJob, ArticleHistoryRecord), DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current = load_article_job_tx(&transaction, id)?
            .ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
        current.state.transition(next)?;
        if current.revision != expected_revision {
            return Err(DomainError::StaleJobRevision {
                id: id.to_owned(),
                expected: expected_revision,
                actual: current.revision,
            });
        }
        let changed = transaction.execute("UPDATE article_jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND state='dispatching' AND revision=?5", params![next.db(), expected_revision + 1, updated_at.to_rfc3339(), id, expected_revision]).map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        transaction
            .execute("INSERT INTO article_history_sequence DEFAULT VALUES", [])
            .map_err(DomainError::database)?;
        let history = ArticleHistoryRecord {
            id: format!(
                "article-scheduled-history-{}",
                transaction.last_insert_rowid()
            ),
            platform: current.request.article_platform()?,
            title: current.request.title.clone(),
            state: next,
            recorded_at: updated_at,
            detail: Some(fixed_history_detail(next).to_owned()),
        };
        transaction.execute("INSERT INTO article_history(id, platform, title, state, recorded_at, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![history.id, article_platform_db(history.platform), history.title, history.state.db(), history.recorded_at.to_rfc3339(), history.detail]).map_err(DomainError::database)?;
        transaction.commit().map_err(DomainError::database)?;
        Ok((
            ArticleScheduledJob {
                state: next,
                revision: expected_revision + 1,
                updated_at,
                ..current
            },
            history,
        ))
    }

    fn requeue_article_claim(
        &self,
        id: &str,
        expected_revision: u64,
        now: DateTime<Utc>,
    ) -> Result<ArticleScheduledJob, DomainError> {
        let mut connection = self.locked()?;
        let transaction = connection.transaction().map_err(DomainError::database)?;
        let current = load_article_job_tx(&transaction, id)?
            .ok_or_else(|| DomainError::UnknownJob(id.to_owned()))?;
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
        let changed = transaction.execute("UPDATE article_jobs SET state=?1, revision=?2, updated_at=?3 WHERE id=?4 AND state='dispatching' AND revision=?5", params![PublishState::Queued.db(), expected_revision + 1, now.to_rfc3339(), id, expected_revision]).map_err(DomainError::database)?;
        if changed != 1 {
            return Err(DomainError::ConcurrentJobUpdate(id.to_owned()));
        }
        transaction.commit().map_err(DomainError::database)?;
        Ok(ArticleScheduledJob {
            state: PublishState::Queued,
            revision: expected_revision + 1,
            updated_at: now,
            ..current
        })
    }
}

fn load_article_job_tx(
    transaction: &Transaction<'_>,
    id: &str,
) -> Result<Option<ArticleScheduledJob>, DomainError> {
    transaction
        .query_row(
            "SELECT id, request_json, state, due_at, revision, updated_at FROM article_jobs WHERE id=?1",
            [id],
            row_to_article_job,
        )
        .optional()
        .map_err(DomainError::database)?
        .transpose()
}

fn row_to_article_job(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ArticleScheduledJob, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let request = row.get::<_, String>(1)?;
    let state = row.get::<_, String>(2)?;
    let due_at = row.get::<_, String>(3)?;
    let revision = row.get::<_, u64>(4)?;
    let updated_at = row.get::<_, String>(5)?;
    Ok((|| {
        Ok(ArticleScheduledJob {
            id,
            request: from_json(&request)?,
            state: PublishState::from_db(&state)?,
            due_at: LocalSchedule::parse(&due_at)?,
            revision,
            updated_at: parse_time(&updated_at)?,
        })
    })())
}

fn row_to_article_history(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ArticleHistoryRecord, DomainError>> {
    let id = row.get::<_, String>(0)?;
    let platform = row.get::<_, String>(1)?;
    let title = row.get::<_, String>(2)?;
    let state = row.get::<_, String>(3)?;
    let recorded_at = row.get::<_, String>(4)?;
    let detail = row.get::<_, Option<String>>(5)?;
    Ok((|| {
        Ok(ArticleHistoryRecord {
            id,
            platform: article_platform_from_db(&platform)?,
            title,
            state: PublishState::from_db(&state)?,
            recorded_at: parse_time(&recorded_at)?,
            detail,
        })
    })())
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

fn fixed_history_detail(state: PublishState) -> &'static str {
    match state {
        PublishState::Published => "scheduled local article runner workflow completed",
        PublishState::Unavailable => "scheduled local article runner unavailable",
        PublishState::Failed => "scheduled local article runner workflow incomplete",
        _ => "scheduled local article runner workflow recorded",
    }
}
