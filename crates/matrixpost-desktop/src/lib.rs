//! Local-only Tauri adapter for the credential-free MatriXpost core.
//!
//! The desktop process owns its SQLite state in the operating system's
//! application-data directory. It never starts the daemon, a shell, a browser,
//! or a provider adapter.

use std::{path::PathBuf, str::FromStr, sync::Arc};

use chrono::Utc;
use matrixpost_core::{
    Account, AccountStatus, ArticleAccount, ArticleAccountStatus, ArticlePlatform, DomainError,
    HistoryFilter, HistoryRecord, HistoryStatus, LocalSchedule, MediaSource, Platform,
    PlatformMetadata, PublicationQueue, PublishRequest, PublishState, Repository, SqliteRepository,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use thiserror::Error;

/// Values shown by the desktop overview. All account data is credential-free.
#[derive(Debug, Serialize)]
pub struct DesktopSnapshot {
    pub platforms: Vec<PlatformMetadata>,
    pub accounts: Vec<AccountEntry>,
    pub article_accounts: Vec<ArticleAccountEntry>,
    pub history_count: usize,
    pub provider_automation_available: bool,
}

/// Video-account metadata safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct AccountEntry {
    pub id: String,
    pub platform: &'static str,
    pub display_name: String,
    pub status: &'static str,
}

/// Small input surface deliberately limited to creating a local video draft.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDraftInput {
    pub title: String,
    pub media_path: String,
    pub targets: Vec<String>,
    pub scheduled_at: Option<String>,
}

/// The durable result of a draft save, with no implication of remote dispatch.
#[derive(Debug, Serialize)]
pub struct DraftSaved {
    pub id: String,
    pub state: &'static str,
    pub remote_publish_attempted: bool,
}

/// Credential-free account metadata accepted from the local desktop form.
///
/// `phone` and `partition` are the existing upstream routing fields. They are
/// required by the durable account model, but never treated as authentication
/// material by this application.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAccountInput {
    pub platform: String,
    pub display_name: String,
    pub status: String,
    pub phone: String,
    pub partition: String,
}

/// The local result of saving safe account metadata.
#[derive(Debug, Serialize)]
pub struct AccountSaved {
    pub id: String,
}

/// Strict, credential-free Juejin article-account metadata from the desktop UI.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveArticleAccountInput {
    pub display_name: String,
    pub status: String,
    pub phone: String,
    pub partition: String,
}

/// Article-account metadata safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct ArticleAccountEntry {
    pub id: String,
    pub display_name: String,
    pub status: &'static str,
}

/// Local result of saving Juejin account metadata.
#[derive(Debug, Serialize)]
pub struct ArticleAccountSaved {
    pub id: String,
    pub status: &'static str,
}

/// Strict, local-only history filtering accepted through Tauri IPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryQueryInput {
    pub days: Option<u16>,
    #[serde(default)]
    pub all: bool,
    pub platform: Option<String>,
    pub status: Option<String>,
}

/// A history record safe to render in the desktop shell.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub state: &'static str,
    pub recorded_at: String,
    pub title: String,
    pub targets: Vec<String>,
    pub draft: bool,
    pub scheduled: bool,
}

/// IPC-safe error returned to the static frontend.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum DesktopError {
    #[error("invalid local draft: {0}")]
    InvalidRequest(String),
    #[error("local state is unavailable: {0}")]
    Storage(String),
}

impl From<DomainError> for DesktopError {
    fn from(error: DomainError) -> Self {
        Self::InvalidRequest(error.to_string())
    }
}

/// Testable local application service, independent of the Tauri runtime.
#[derive(Clone)]
pub struct DesktopService {
    repository: Arc<SqliteRepository>,
}

impl DesktopService {
    pub fn new(repository: Arc<SqliteRepository>) -> Self {
        Self { repository }
    }

    pub fn open(state_path: PathBuf) -> Result<Self, DesktopError> {
        SqliteRepository::open(state_path)
            .map(|repository| Self::new(Arc::new(repository)))
            .map_err(|error| DesktopError::Storage(error.to_string()))
    }

    pub fn snapshot(&self) -> Result<DesktopSnapshot, DesktopError> {
        Ok(DesktopSnapshot {
            platforms: Platform::ALL
                .iter()
                .copied()
                .map(Platform::metadata)
                .collect(),
            accounts: self
                .repository
                .accounts()?
                .into_iter()
                .map(AccountEntry::from)
                .collect(),
            article_accounts: self
                .repository
                .article_accounts()?
                .into_iter()
                .map(ArticleAccountEntry::from)
                .collect(),
            history_count: self.repository.history()?.len(),
            provider_automation_available: false,
        })
    }

    pub fn save_local_draft(&self, input: SaveDraftInput) -> Result<DraftSaved, DesktopError> {
        let request = PublishRequest {
            source: MediaSource::LocalFile(PathBuf::from(input.media_path.trim())),
            title: input.title,
            short_title: None,
            tags: Vec::new(),
            address: None,
            // This adapter is intentionally unable to create a queued job.
            draft: true,
            bt2: None,
            scheduled_at: input
                .scheduled_at
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(LocalSchedule::parse)
                .transpose()?,
            task_name: None,
            account: Default::default(),
            wechat_link: Default::default(),
            overrides: Vec::new(),
            targets: input
                .targets
                .iter()
                .map(|target| Platform::from_str(target))
                .collect::<Result<Vec<_>, _>>()?,
        };
        request.validate()?;
        let job = PublicationQueue::enqueue(self.repository.as_ref(), &request, Utc::now())?;
        debug_assert_eq!(job.state, matrixpost_core::PublishState::Draft);
        Ok(DraftSaved {
            id: job.id,
            state: "draft",
            remote_publish_attempted: false,
        })
    }

    pub fn save_account(&self, input: SaveAccountInput) -> Result<AccountSaved, DesktopError> {
        let platform = Platform::from_str(&input.platform)?;
        let display_name = input.display_name.trim();
        if display_name.is_empty() {
            return Err(DesktopError::InvalidRequest(
                "account display name cannot be empty".into(),
            ));
        }
        let status = match input.status.trim() {
            "logged_in" => AccountStatus::LoggedIn,
            "expired" => AccountStatus::Expired,
            "logged_out" => AccountStatus::LoggedOut,
            "unavailable" => AccountStatus::Unavailable,
            value => {
                return Err(DesktopError::InvalidRequest(format!(
                    "unknown account status: {value}"
                )));
            }
        };
        let id = account_id(platform, display_name);
        let account = Account {
            id: id.clone(),
            platform,
            display_name: display_name.to_owned(),
            status,
            phone: input.phone.trim().to_owned(),
            partition: input.partition.trim().to_owned(),
        };
        self.repository.save_account(&account)?;
        Ok(AccountSaved { id })
    }

    pub fn save_article_account(
        &self,
        input: SaveArticleAccountInput,
    ) -> Result<ArticleAccountSaved, DesktopError> {
        let display_name = input.display_name.trim();
        if display_name.is_empty() {
            return Err(DesktopError::InvalidRequest(
                "article account display name cannot be empty".into(),
            ));
        }
        let status = article_account_status(&input.status)?;
        let id = article_account_id(display_name);
        self.repository.save_article_account(&ArticleAccount {
            id: id.clone(),
            platform: ArticlePlatform::Juejin,
            display_name: display_name.to_owned(),
            status,
            phone: input.phone.trim().to_owned(),
            partition: input.partition.trim().to_owned(),
        })?;
        Ok(ArticleAccountSaved {
            id,
            status: article_account_status_label(status),
        })
    }

    pub fn history_entries(
        &self,
        input: HistoryQueryInput,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<HistoryEntry>, DesktopError> {
        let platform = input
            .platform
            .as_deref()
            .map(Platform::from_str)
            .transpose()?;
        let status = input
            .status
            .as_deref()
            .map(HistoryStatus::from_str)
            .transpose()
            .map_err(|error| DesktopError::InvalidRequest(error.to_string()))?;
        let filter = HistoryFilter::from_query(input.days, input.all, platform, status, now)
            .map_err(|error| DesktopError::InvalidRequest(error.to_string()))?;

        Ok(filter
            .filter(self.repository.history()?)
            .into_iter()
            .map(HistoryEntry::from)
            .collect())
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

fn article_account_status(value: &str) -> Result<ArticleAccountStatus, DesktopError> {
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

const fn article_account_status_label(status: ArticleAccountStatus) -> &'static str {
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

fn account_id(platform: Platform, display_name: &str) -> String {
    format!("{}-{}", platform.as_str(), account_slug(display_name))
}

fn article_account_id(display_name: &str) -> String {
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

/// Managed Tauri state; the service itself has no dependency on Tauri.
pub struct DesktopState {
    service: DesktopService,
}

#[tauri::command]
fn desktop_snapshot(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopSnapshot, DesktopError> {
    state.service.snapshot()
}

#[tauri::command]
fn save_local_draft(
    state: tauri::State<'_, DesktopState>,
    input: SaveDraftInput,
) -> Result<DraftSaved, DesktopError> {
    state.service.save_local_draft(input)
}

#[tauri::command]
fn save_account(
    state: tauri::State<'_, DesktopState>,
    input: SaveAccountInput,
) -> Result<AccountSaved, DesktopError> {
    state.service.save_account(input)
}

#[tauri::command]
fn save_article_account(
    state: tauri::State<'_, DesktopState>,
    input: SaveArticleAccountInput,
) -> Result<ArticleAccountSaved, DesktopError> {
    state.service.save_article_account(input)
}

#[tauri::command]
fn local_history(
    state: tauri::State<'_, DesktopState>,
    input: HistoryQueryInput,
) -> Result<Vec<HistoryEntry>, DesktopError> {
    state.service.history_entries(input, Utc::now())
}

/// Starts the platform-native shell. All UI access is through Tauri IPC.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let state_path = directory.join("matrixpost.db");
            app.manage(DesktopState {
                service: DesktopService::open(state_path)?,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_snapshot,
            save_local_draft,
            save_account,
            save_article_account,
            local_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running MatriXpost desktop");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, TimeZone, Utc};
    use matrixpost_core::{
        AccountSelection, HistoryRecord, LocalSchedule, MediaSource, PublishRequest, PublishState,
        Repository, SqliteRepository,
    };
    use serde::Deserialize;
    use serde::de::value::{
        BoolDeserializer, Error as ValueError, MapDeserializer, StringDeserializer,
    };

    use super::{
        DesktopService, HistoryQueryInput, SaveAccountInput, SaveArticleAccountInput,
        SaveDraftInput,
    };

    fn service() -> DesktopService {
        DesktopService::new(Arc::new(
            SqliteRepository::in_memory().expect("in-memory state"),
        ))
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
}
