//! Local stdio MCP adapter for MatriXpost's credential-free SQLite state.
//!
//! The server intentionally has no browser, provider, shell, daemon, or network
//! integration. It can inspect local account/history metadata and record a
//! validated video job, but never reports remote publication success.

use std::{ffi::OsStr, path::PathBuf, process::ExitCode, sync::Arc};

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use matrixpost_core::{
    Account, AccountSelection, ArticleAccount, ArticleDispatchOutcome, ArticleRunner,
    HistoryFilter, HistoryRecord, HistoryStatus, LocalSchedule, MediaSource, Platform,
    PlatformOverride, PublicationQueue, PublishArticleRequest, PublishRequest, PublishState,
    Repository, ScheduledJob, SqliteRepository, WechatLink,
};
use rmcp::{
    ServiceExt, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const DEFAULT_STATE_PATH: &str = "matrixpost.db";
const STATE_PATH_ENV: &str = "MATRIXPOST_STATE_PATH";
const LOG_ENV: &str = "MATRIXPOST_MCP_LOG";
const PROVIDER_MESSAGE: &str =
    "no provider implementation is configured; no remote publishing was attempted";

/// Exact upstream account-query platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AccountsPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
    Xhs,
    Juejin,
    Fqsp,
}

/// Exact upstream video-publication platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum VideoPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
}

/// Exact upstream article-publication platform set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ArticlePlatformInput {
    Juejin,
}

/// Exact documented history filter set; Fanqie is intentionally absent.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum HistoryPlatform {
    Dy,
    Ks,
    Blbl,
    Bjh,
    Tt,
    Sph,
    Xhs,
}

/// Exact upstream publication-state filter set.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum HistoryStatusInput {
    Success,
    Failed,
    Publishing,
    Scheduled,
}

impl From<HistoryStatusInput> for HistoryStatus {
    fn from(value: HistoryStatusInput) -> Self {
        match value {
            HistoryStatusInput::Success => Self::Success,
            HistoryStatusInput::Failed => Self::Failed,
            HistoryStatusInput::Publishing => Self::Publishing,
            HistoryStatusInput::Scheduled => Self::Scheduled,
        }
    }
}

/// The only accepted account-list filter from the upstream MCP contract.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAccountsInput {
    /// Optional exact upstream platform code.
    platform: Option<AccountsPlatform>,
}

/// The upstream-compatible history query. Filters operate only on local state.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListHistoryInput {
    /// Number of trailing days to return; defaults to seven unless `all` is true.
    days: Option<u16>,
    /// Optional exact upstream platform code.
    platform: Option<HistoryPlatform>,
    /// One of `success`, `failed`, `publishing`, or `scheduled`.
    status: Option<HistoryStatusInput>,
    /// When true, do not apply the default trailing-seven-day filter.
    all: Option<bool>,
}

/// Video-link metadata accepted by MatrixMedia's WeChat Channels request form.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum SphLinkInput {
    /// Explicitly disables a link.
    None {},
    /// Links to a product and therefore requires its provider value.
    Product { value: String },
}

/// Upstream-compatible video publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishVideoInput {
    /// One exact upstream video platform code.
    platform: VideoPlatform,
    /// Local absolute video path or an `http`/`https` URL.
    file: String,
    /// Publication title.
    title: String,
    /// Account phone/partition selector used only as local routing metadata.
    phone: String,
    /// Upstream secondary-title field.
    bt2: Option<String>,
    /// Comma- or whitespace-separated tags.
    tags: Option<String>,
    /// Optional publication address.
    address: Option<String>,
    /// Upstream schedule in `YYYY-MM-DD HH:MM` or `YYYY-MM-DD HH:MM:SS` form.
    publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    show: Option<bool>,
    /// Record the job as a draft instead of a queued local intent.
    draft: Option<bool>,
    /// Optional platform-specific creative declaration.
    creative_statement: Option<String>,
    /// WeChat Channels product identifier.
    sph_product_id: Option<String>,
    /// WeChat Channels link data.
    sph_link: Option<SphLinkInput>,
}

/// Upstream-compatible Juejin article publication arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishArticleInput {
    /// The only upstream MCP article target: `juejin`.
    platform: ArticlePlatformInput,
    /// Account phone/partition selector used only as local routing metadata.
    phone: String,
    /// Article title.
    title: String,
    /// Inline article body; required when `file` is omitted.
    content: Option<String>,
    /// Markdown file path; required when `content` is omitted.
    file: Option<String>,
    /// Optional cover image path.
    cover: Option<String>,
    /// Optional Juejin category.
    category: Option<String>,
    /// Upstream single-string tags, normalized into the typed core vector.
    tags: Option<String>,
    /// Optional article summary.
    summary: Option<String>,
    /// CLI-compatible `HH:MM`, `YYYY-MM-DD HH:MM`, or `YYYY-MM-DD HH:MM:SS` schedule.
    publish_at: Option<String>,
    /// Accepted for upstream compatibility but never opens a browser here.
    show: Option<bool>,
}

/// Exact upstream account-list item contract.
#[derive(Debug, Serialize)]
struct ListedAccount {
    phone: String,
    platform: &'static str,
    partition: String,
}

/// The non-sensitive, durable part of a locally queued video job.
#[derive(Debug, Serialize)]
struct JobResult {
    id: String,
    state: PublishState,
    due_at: Option<LocalSchedule>,
    revision: u64,
}

/// Explicit provider boundary returned by both publication tools.
#[derive(Debug, Serialize)]
struct PublicationResult {
    outcome: &'static str,
    provider_available: bool,
    remote_publish_attempted: bool,
    persisted: bool,
    job: Option<JobResult>,
    message: &'static str,
}

/// Typed, inspectable validation result for an MCP tool call.
#[derive(Debug, Serialize)]
struct ToolFailure {
    outcome: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Clone)]
struct MatrixpostMcp {
    repository: Arc<SqliteRepository>,
    article_runner: Option<ArticleRunner>,
}

#[tool_router(server_handler)]
impl MatrixpostMcp {
    #[tool(
        description = "List credential-free local MatriXpost accounts, optionally filtered by platform."
    )]
    async fn list_accounts(
        &self,
        Parameters(input): Parameters<ListAccountsInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_accounts_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "List local MatriXpost publication history with upstream-compatible filters."
    )]
    async fn list_history(
        &self,
        Parameters(input): Parameters<ListHistoryInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.list_history_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "Validate and persist a local video job. No provider automation or remote publication is attempted."
    )]
    async fn publish_video(
        &self,
        Parameters(input): Parameters<PublishVideoInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.publish_video_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }

    #[tool(
        description = "Validate a Juejin article request and, only with an explicit local article runner, dispatch it through that runner. A queued result confirms local workflow completion only, never remote publication."
    )]
    async fn publish_article(
        &self,
        Parameters(input): Parameters<PublishArticleInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(match self.publish_article_result(input) {
            Ok(result) => structured(result),
            Err(message) => tool_error("invalid_input", message),
        })
    }
}

impl MatrixpostMcp {
    fn list_accounts_result(&self, input: ListAccountsInput) -> Result<Vec<ListedAccount>, String> {
        let video_filter = input.platform.and_then(accounts_video_platform);
        let include_articles = input
            .platform
            .is_none_or(|platform| matches!(platform, AccountsPlatform::Juejin));
        let mut accounts = self
            .repository
            .accounts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|account| video_filter.is_none_or(|platform| account.platform == platform))
            .map(listed_video_account)
            .collect::<Vec<_>>();
        if include_articles {
            accounts.extend(
                self.repository
                    .article_accounts()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .map(listed_article_account),
            );
        }
        Ok(accounts)
    }

    fn list_history_result(&self, input: ListHistoryInput) -> Result<Vec<HistoryRecord>, String> {
        let platform = input.platform.and_then(history_video_platform);
        let status = input.status.map(HistoryStatus::from);
        let filter = HistoryFilter::from_query(
            input.days,
            input.all.unwrap_or(false),
            platform,
            status,
            Utc::now(),
        )
        .map_err(|error| error.to_string())?;
        let history = self
            .repository
            .history()
            .map_err(|error| error.to_string())?;
        Ok(filter.filter(history))
    }

    fn publish_video_result(&self, input: PublishVideoInput) -> Result<PublicationResult, String> {
        let request = video_request(input)?;
        let job = self
            .repository
            .enqueue(&request, Utc::now())
            .map_err(|error| error.to_string())?;
        Ok(PublicationResult {
            outcome: "queued_locally",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: true,
            job: Some(job_result(job)),
            message: PROVIDER_MESSAGE,
        })
    }

    fn publish_article_result(
        &self,
        input: PublishArticleInput,
    ) -> Result<PublicationResult, String> {
        let request = article_request(input)?;
        let Some(runner) = &self.article_runner else {
            return Ok(article_unavailable_result());
        };
        let outcome = runner
            .dispatch(&request)
            .map_err(|error| error.to_string())?;
        Ok(article_dispatch_result(outcome))
    }
}

fn article_unavailable_result() -> PublicationResult {
    PublicationResult {
        outcome: "unavailable",
        provider_available: false,
        remote_publish_attempted: false,
        persisted: false,
        job: None,
        message: "no article runner is configured; no remote publishing was attempted",
    }
}

fn article_dispatch_result(outcome: ArticleDispatchOutcome) -> PublicationResult {
    match outcome {
        ArticleDispatchOutcome::Queued { .. } => PublicationResult {
            outcome: "queued",
            provider_available: true,
            remote_publish_attempted: true,
            persisted: false,
            job: None,
            message: "local article runner completed its WebDriver workflow; remote publication is not confirmed",
        },
        ArticleDispatchOutcome::Unavailable { .. } => PublicationResult {
            outcome: "unavailable",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: false,
            job: None,
            message: "article runner was unavailable; no remote publishing was attempted",
        },
        ArticleDispatchOutcome::Rejected {
            automation_attempted,
            ..
        } => PublicationResult {
            outcome: "rejected",
            provider_available: false,
            remote_publish_attempted: automation_attempted,
            persisted: false,
            job: None,
            message: "article runner rejected the request; no remote publication success is claimed",
        },
    }
}

fn accounts_video_platform(value: AccountsPlatform) -> Option<Platform> {
    Some(match value {
        AccountsPlatform::Dy => Platform::Douyin,
        AccountsPlatform::Ks => Platform::Kuaishou,
        AccountsPlatform::Blbl => Platform::Bilibili,
        AccountsPlatform::Bjh => Platform::Baijiahao,
        AccountsPlatform::Tt => Platform::Toutiao,
        AccountsPlatform::Sph => Platform::WechatChannels,
        AccountsPlatform::Xhs => Platform::Xiaohongshu,
        AccountsPlatform::Fqsp => Platform::FanqieVideo,
        AccountsPlatform::Juejin => return None,
    })
}

fn listed_video_account(account: Account) -> ListedAccount {
    ListedAccount {
        phone: account.phone,
        platform: account.platform.as_str(),
        partition: account.partition,
    }
}

fn listed_article_account(account: ArticleAccount) -> ListedAccount {
    ListedAccount {
        phone: account.phone,
        platform: "juejin",
        partition: account.partition,
    }
}

fn history_video_platform(value: HistoryPlatform) -> Option<Platform> {
    Some(match value {
        HistoryPlatform::Dy => Platform::Douyin,
        HistoryPlatform::Ks => Platform::Kuaishou,
        HistoryPlatform::Blbl => Platform::Bilibili,
        HistoryPlatform::Bjh => Platform::Baijiahao,
        HistoryPlatform::Tt => Platform::Toutiao,
        HistoryPlatform::Sph => Platform::WechatChannels,
        HistoryPlatform::Xhs => Platform::Xiaohongshu,
    })
}

fn video_request(input: PublishVideoInput) -> Result<PublishRequest, String> {
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let platform = video_platform(input.platform);
    let _ = input.show;
    let source = match url::Url::parse(&input.file) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
        Ok(url) => {
            return Err(format!(
                "unsupported remote source scheme: {}",
                url.scheme()
            ));
        }
        Err(_) => MediaSource::LocalFile(PathBuf::from(&input.file)),
    };
    let scheduled_at = input
        .publish_at
        .as_deref()
        .map(parse_video_schedule)
        .transpose()?;
    let wechat_link = if platform == Platform::WechatChannels {
        effective_sph_link(input.sph_product_id, input.sph_link)?
    } else {
        WechatLink::default()
    };
    let overrides = input.creative_statement.map(|creative_statement| {
        vec![PlatformOverride {
            platform,
            title: None,
            short_title: None,
            tags: None,
            creative_statement: Some(creative_statement),
            account: None,
            wechat_link: None,
        }]
    });
    let request = PublishRequest {
        source,
        title: input.title,
        short_title: None,
        tags: split_tags(input.tags),
        address: input.address,
        draft: input.draft.unwrap_or(false),
        bt2: input.bt2,
        scheduled_at,
        task_name: None,
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        wechat_link,
        overrides: overrides.unwrap_or_default(),
        targets: vec![platform],
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn video_platform(value: VideoPlatform) -> Platform {
    match value {
        VideoPlatform::Dy => Platform::Douyin,
        VideoPlatform::Ks => Platform::Kuaishou,
        VideoPlatform::Blbl => Platform::Bilibili,
        VideoPlatform::Bjh => Platform::Baijiahao,
        VideoPlatform::Tt => Platform::Toutiao,
        VideoPlatform::Sph => Platform::WechatChannels,
    }
}

fn parse_video_schedule(value: &str) -> Result<LocalSchedule, String> {
    normalize_full_schedule(
        value,
        "publishAt must use YYYY-MM-DD HH:mm or YYYY-MM-DD HH:mm:ss",
    )
}

fn parse_article_schedule(value: &str, today: NaiveDate) -> Result<LocalSchedule, String> {
    if let Ok(time) = NaiveTime::parse_from_str(value, "%H:%M") {
        return LocalSchedule::parse(&format!("{} {}:00", today, time.format("%H:%M")))
            .map_err(|error| error.to_string());
    }
    normalize_full_schedule(
        value,
        "publishAt must use HH:mm, YYYY-MM-DD HH:mm, or YYYY-MM-DD HH:mm:ss",
    )
}

fn normalize_full_schedule(value: &str, message: &str) -> Result<LocalSchedule, String> {
    if let Ok(schedule) = LocalSchedule::parse(value) {
        return Ok(schedule);
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")
        .map(|time| LocalSchedule(time.format("%Y-%m-%d %H:%M:%S").to_string()))
        .map_err(|_| message.to_owned())
}

fn effective_sph_link(
    product_id: Option<String>,
    link: Option<SphLinkInput>,
) -> Result<WechatLink, String> {
    if let Some(product_id) = product_id {
        if product_id.trim().is_empty() {
            return Err("sphProductId must not be empty".into());
        }
        return Ok(WechatLink {
            link_type: Some("product".into()),
            link_value: Some(product_id.clone()),
            product_id: Some(product_id),
        });
    }
    match link {
        None => Ok(WechatLink::default()),
        Some(SphLinkInput::None {}) => Ok(WechatLink {
            product_id: None,
            link_type: Some("none".into()),
            link_value: None,
        }),
        Some(SphLinkInput::Product { value }) if !value.trim().is_empty() => Ok(WechatLink {
            product_id: None,
            link_type: Some("product".into()),
            link_value: Some(value),
        }),
        Some(SphLinkInput::Product { .. }) => {
            Err("sphLink.value must not be empty when sphLink.type is product".into())
        }
    }
}

fn article_request(input: PublishArticleInput) -> Result<PublishArticleRequest, String> {
    let _ = input.platform;
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let _ = input.show;
    let request = PublishArticleRequest {
        platform: "juejin".into(),
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        title: input.title,
        content: input.content,
        file: input.file.map(PathBuf::from),
        cover: input.cover,
        category: input.category,
        tags: split_tags(input.tags),
        summary: input.summary,
        scheduled_at: input
            .publish_at
            .as_deref()
            .map(|value| parse_article_schedule(value, Local::now().date_naive()))
            .transpose()?,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn split_tags(tags: Option<String>) -> Vec<String> {
    tags.unwrap_or_default()
        .split([' ', ','])
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn job_result(job: ScheduledJob) -> JobResult {
    JobResult {
        id: job.id,
        state: job.state,
        due_at: job.due_at,
        revision: job.revision,
    }
}

fn structured<T: Serialize>(value: T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => tool_error("serialization_failure", error.to_string()),
    }
}

fn tool_error(code: &'static str, message: String) -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!(ToolFailure {
        outcome: "rejected",
        code,
        message,
    }))
}

struct McpConfig {
    state_path: PathBuf,
    article_runner: Option<ArticleRunner>,
}

fn mcp_config(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<McpConfig, String> {
    let mut state_path = None;
    let mut article_runner = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--state-path" {
            let value = args
                .next()
                .ok_or_else(|| "--state-path requires a path".to_owned())?;
            if state_path.replace(PathBuf::from(value)).is_some() {
                return Err("--state-path may be supplied only once".into());
            }
        } else if let Some(value) = argument.strip_prefix("--state-path=") {
            if value.is_empty() || state_path.replace(PathBuf::from(value)).is_some() {
                return Err("--state-path must be supplied once with a non-empty path".into());
            }
        } else if argument == "--article-runner" {
            let value = args
                .next()
                .ok_or_else(|| "--article-runner requires tcp:127.0.0.1:PORT".to_owned())?;
            if article_runner.is_some() {
                return Err("--article-runner may be supplied only once".into());
            }
            article_runner =
                Some(ArticleRunner::parse_cli(&value).map_err(|error| error.to_string())?);
        } else if let Some(value) = argument.strip_prefix("--article-runner=") {
            if value.is_empty() || article_runner.is_some() {
                return Err(
                    "--article-runner must be supplied once with tcp:127.0.0.1:PORT".into(),
                );
            }
            article_runner =
                Some(ArticleRunner::parse_cli(value).map_err(|error| error.to_string())?);
        } else {
            return Err(format!("unsupported argument: {argument}"));
        }
    }
    Ok(McpConfig {
        state_path: state_path
            .or_else(|| env_path.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_PATH)),
        article_runner,
    })
}

#[cfg(test)]
fn state_path(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<PathBuf, String> {
    mcp_config(args, env_path).map(|config| config.state_path)
}

fn logging_enabled(value: Option<&OsStr>) -> bool {
    matches!(value.and_then(OsStr::to_str), Some("1" | "true" | "yes"))
}

fn log_error(message: impl std::fmt::Display) {
    if logging_enabled(std::env::var_os(LOG_ENV).as_deref()) {
        eprintln!("matrixpost-mcp: {message}");
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let config = match mcp_config(
        std::env::args().skip(1),
        std::env::var_os(STATE_PATH_ENV).as_deref(),
    ) {
        Ok(config) => config,
        Err(error) => {
            log_error(error);
            return ExitCode::from(2);
        }
    };
    let repository = match SqliteRepository::open(&config.state_path) {
        Ok(repository) => Arc::new(repository),
        Err(error) => {
            log_error(format!(
                "failed to open {}: {error}",
                config.state_path.display()
            ));
            return ExitCode::from(4);
        }
    };
    let service = match (MatrixpostMcp {
        repository,
        article_runner: config.article_runner,
    })
    .serve(stdio())
    .await
    {
        Ok(service) => service,
        Err(error) => {
            log_error(format!("stdio service failed: {error}"));
            return ExitCode::from(4);
        }
    };
    match service.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            log_error(format!("stdio service stopped unexpectedly: {error}"));
            ExitCode::from(4)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use matrixpost_core::{ArticleAccountStatus, ArticlePlatform};
    use rmcp::{
        ClientHandler, RoleClient, ServiceExt,
        model::{CallToolRequestParams, ClientInfo},
        service::RunningService,
    };
    use tokio::task::JoinHandle;

    use super::*;

    fn service() -> MatrixpostMcp {
        MatrixpostMcp {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            article_runner: None,
        }
    }

    #[derive(Clone, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    async fn connect(
        server: MatrixpostMcp,
    ) -> (RunningService<RoleClient, TestClient>, JoinHandle<()>) {
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server_handle = tokio::spawn(async move {
            server
                .serve(server_transport)
                .await
                .unwrap()
                .waiting()
                .await
                .unwrap();
        });
        let client = TestClient.serve(client_transport).await.unwrap();
        (client, server_handle)
    }

    async fn disconnect(
        client: RunningService<RoleClient, TestClient>,
        server_handle: JoinHandle<()>,
    ) {
        client.cancel().await.unwrap();
        server_handle.await.unwrap();
    }

    #[test]
    fn video_request_maps_upstream_arguments_without_provider_side_effects() {
        let request = video_request(PublishVideoInput {
            platform: VideoPlatform::Sph,
            file: "https://example.invalid/video.mp4".into(),
            title: "Title".into(),
            phone: "13800138000".into(),
            bt2: Some("Short".into()),
            tags: Some("one,two three".into()),
            address: Some("Somewhere".into()),
            publish_at: Some("2026-08-01 10:20".into()),
            show: Some(true),
            draft: Some(true),
            creative_statement: Some("original".into()),
            sph_product_id: Some("product-1".into()),
            sph_link: None,
        })
        .unwrap();
        assert_eq!(request.targets, vec![Platform::WechatChannels]);
        assert_eq!(request.wechat_link.product_id.as_deref(), Some("product-1"));
        assert_eq!(request.wechat_link.link_type.as_deref(), Some("product"));
        assert_eq!(request.wechat_link.link_value.as_deref(), Some("product-1"));
        assert_eq!(request.scheduled_at.unwrap().0, "2026-08-01 10:20:00");
        assert_eq!(request.tags, vec!["one", "two", "three"]);
    }

    #[test]
    fn list_accounts_reads_persisted_juejin_account_metadata() {
        let service = service();
        service
            .repository
            .save_article_account(&ArticleAccount {
                id: "juejin-primary".into(),
                platform: ArticlePlatform::Juejin,
                display_name: "Primary".into(),
                status: ArticleAccountStatus::LoggedIn,
                phone: "13800138000".into(),
                partition: "persist:juejin-primary".into(),
            })
            .unwrap();
        let result = service
            .list_accounts_result(ListAccountsInput {
                platform: Some(AccountsPlatform::Juejin),
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!([{
                "phone": "13800138000",
                "platform": "juejin",
                "partition": "persist:juejin-primary"
            }])
        );
    }

    #[test]
    fn video_platform_schema_excludes_non_upstream_targets() {
        assert_eq!(video_platform(VideoPlatform::Sph), Platform::WechatChannels);
        assert!(
            serde_json::from_value::<PublishVideoInput>(serde_json::json!({
                "platform": "xhs", "file": "/tmp/video.mp4", "title": "T", "phone": "p"
            }))
            .is_err()
        );
    }

    #[test]
    fn article_tags_accept_the_upstream_string_and_normalize_to_core_tags() {
        let input = serde_json::from_value::<PublishArticleInput>(serde_json::json!({
            "platform": "juejin",
            "phone": "13800138000",
            "title": "Title",
            "content": "Body",
            "tags": "one,two three"
        }))
        .unwrap();
        let request = article_request(input).unwrap();
        assert_eq!(request.tags, vec!["one", "two", "three"]);
    }

    #[test]
    fn history_platform_schema_rejects_fqsp_and_filters_through_the_core_query() {
        assert!(
            serde_json::from_value::<ListHistoryInput>(serde_json::json!({
                "platform": "fqsp"
            }))
            .is_err()
        );
        let history = service()
            .list_history_result(ListHistoryInput {
                days: None,
                platform: None,
                status: None,
                all: Some(true),
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(history).unwrap(),
            serde_json::json!([])
        );

        let service = service();
        let request = video_request(PublishVideoInput {
            platform: VideoPlatform::Dy,
            file: "/tmp/video.mp4".into(),
            title: "Title".into(),
            phone: "13800138000".into(),
            bt2: None,
            tags: None,
            address: None,
            publish_at: None,
            show: None,
            draft: Some(true),
            creative_statement: None,
            sph_product_id: None,
            sph_link: None,
        })
        .unwrap();
        let mut other_platform = request.clone();
        other_platform.targets = vec![Platform::Bilibili];
        let records = vec![
            HistoryRecord {
                id: "scheduled-dy".into(),
                request: request.clone(),
                state: PublishState::Queued,
                recorded_at: Utc::now(),
                detail: None,
            },
            HistoryRecord {
                id: "draft-dy".into(),
                request: request.clone(),
                state: PublishState::Draft,
                recorded_at: Utc::now(),
                detail: None,
            },
            HistoryRecord {
                id: "published-dy".into(),
                request,
                state: PublishState::Published,
                recorded_at: Utc::now(),
                detail: None,
            },
            HistoryRecord {
                id: "scheduled-blbl".into(),
                request: other_platform,
                state: PublishState::Queued,
                recorded_at: Utc::now(),
                detail: None,
            },
        ];
        for record in &records {
            service.repository.append_history(record).unwrap();
        }
        let input = ListHistoryInput {
            days: None,
            platform: Some(HistoryPlatform::Dy),
            status: Some(HistoryStatusInput::Scheduled),
            all: Some(true),
        };
        let expected = HistoryFilter::from_query(
            input.days,
            true,
            Some(Platform::Douyin),
            Some(HistoryStatus::Scheduled),
            Utc::now(),
        )
        .unwrap()
        .filter(records);
        let actual = service.list_history_result(input).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            actual
                .into_iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            ["scheduled-dy"]
        );
    }

    #[test]
    fn article_schedule_accepts_time_only_and_full_seconds_forms() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        assert_eq!(
            parse_article_schedule("10:20", date).unwrap().0,
            "2026-08-01 10:20:00"
        );
        assert_eq!(
            parse_article_schedule("2026-08-02 10:20:30", date)
                .unwrap()
                .0,
            "2026-08-02 10:20:30"
        );
    }

    #[test]
    fn sph_link_schema_rejects_missing_or_arbitrary_link_details() {
        assert!(serde_json::from_value::<SphLinkInput>(serde_json::json!({})).is_err());
        assert!(
            serde_json::from_value::<SphLinkInput>(serde_json::json!({
                "type": "article", "value": "value"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SphLinkInput>(serde_json::json!({
                "type": "none", "value": "unexpected"
            }))
            .is_err()
        );
    }

    #[test]
    fn sph_product_link_requires_value_and_product_id_takes_precedence() {
        let missing_value = effective_sph_link(
            None,
            Some(SphLinkInput::Product {
                value: String::new(),
            }),
        )
        .unwrap_err();
        assert_eq!(
            missing_value,
            "sphLink.value must not be empty when sphLink.type is product"
        );
        let effective =
            effective_sph_link(Some("product-id".into()), Some(SphLinkInput::None {})).unwrap();
        assert_eq!(effective.product_id.as_deref(), Some("product-id"));
        assert_eq!(effective.link_type.as_deref(), Some("product"));
        assert_eq!(effective.link_value.as_deref(), Some("product-id"));
    }

    #[test]
    fn article_request_rejects_missing_content_and_file() {
        let error = article_request(PublishArticleInput {
            platform: ArticlePlatformInput::Juejin,
            phone: "13800138000".into(),
            title: "Title".into(),
            content: None,
            file: None,
            cover: None,
            category: None,
            tags: None,
            summary: None,
            publish_at: None,
            show: None,
        })
        .unwrap_err();
        assert_eq!(error, "article content or file is required");
    }

    #[test]
    fn publish_video_persists_only_a_local_job_and_reports_provider_unavailable() {
        let result = service()
            .publish_video_result(PublishVideoInput {
                platform: VideoPlatform::Dy,
                file: "/tmp/video.mp4".into(),
                title: "Title".into(),
                phone: "13800138000".into(),
                bt2: None,
                tags: None,
                address: None,
                publish_at: None,
                show: None,
                draft: None,
                creative_statement: None,
                sph_product_id: None,
                sph_link: None,
            })
            .unwrap();
        assert_eq!(result.outcome, "queued_locally");
        assert!(!result.provider_available);
        assert!(!result.remote_publish_attempted);
        assert!(result.persisted);
        assert!(result.job.is_some());
    }

    #[test]
    fn state_path_flag_overrides_environment_path() {
        let path = state_path(
            ["--state-path".to_owned(), "flag.db".to_owned()],
            Some(OsStr::new("environment.db")),
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("flag.db"));
    }

    #[test]
    fn mcp_arguments_accept_only_state_path_and_one_loopback_article_runner() {
        let config = mcp_config(
            [
                "--state-path".to_owned(),
                "state.db".to_owned(),
                "--article-runner=tcp:127.0.0.1:39002".to_owned(),
            ],
            None,
        )
        .unwrap();
        assert_eq!(config.state_path, PathBuf::from("state.db"));
        assert_eq!(
            config.article_runner.unwrap().address.to_string(),
            "127.0.0.1:39002"
        );
        assert!(mcp_config(["--provider-runner=tcp:127.0.0.1:39001".into()], None).is_err());
        assert!(mcp_config(["--article-runner=tcp:192.0.2.1:39002".into()], None).is_err());
        assert!(
            mcp_config(
                [
                    "--article-runner=tcp:127.0.0.1:39002".into(),
                    "--article-runner=tcp:127.0.0.1:39003".into(),
                ],
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn article_service_reports_default_unavailable_and_queued_runner_truthfully() {
        let unavailable = service()
            .publish_article_result(PublishArticleInput {
                platform: ArticlePlatformInput::Juejin,
                phone: "13800138000".into(),
                title: "Title".into(),
                content: Some("Body".into()),
                file: None,
                cover: None,
                category: None,
                tags: None,
                summary: None,
                publish_at: None,
                show: None,
            })
            .unwrap();
        assert_eq!(unavailable.outcome, "unavailable");
        assert!(!unavailable.provider_available);
        assert!(!unavailable.remote_publish_attempted);

        let queued = article_dispatch_result(ArticleDispatchOutcome::Queued {
            job_id: "mock-article-job".into(),
        });
        assert_eq!(queued.outcome, "queued");
        assert!(queued.provider_available);
        assert!(queued.remote_publish_attempted);

        let preflight_rejection = article_dispatch_result(ArticleDispatchOutcome::Rejected {
            reason: "unsupported schedule".into(),
            automation_attempted: false,
        });
        assert_eq!(preflight_rejection.outcome, "rejected");
        assert!(!preflight_rejection.provider_available);
        assert!(!preflight_rejection.remote_publish_attempted);

        let attempted_rejection = article_dispatch_result(ArticleDispatchOutcome::Rejected {
            reason: "mock automation failure".into(),
            automation_attempted: true,
        });
        assert_eq!(attempted_rejection.outcome, "rejected");
        assert!(!attempted_rejection.provider_available);
        assert!(attempted_rejection.remote_publish_attempted);
    }

    #[test]
    fn stderr_logging_is_opt_in() {
        assert!(!logging_enabled(None));
        assert!(logging_enabled(Some(OsStr::new("1"))));
    }

    #[test]
    fn macro_generated_router_lists_exactly_the_four_upstream_tools_with_closed_schemas() {
        let router = MatrixpostMcp::tool_router();
        let tools = router.list_all();
        let names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_accounts",
                "list_history",
                "publish_article",
                "publish_video"
            ]
        );
        let publish_video = router.get("publish_video").unwrap();
        let schema = serde_json::to_value(&publish_video.input_schema).unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["$defs"]["VideoPlatform"]["enum"],
            serde_json::json!(["dy", "ks", "blbl", "bjh", "tt", "sph"])
        );
        assert!(schema["$defs"]["SphLinkInput"]["oneOf"].is_array());
        assert!(
            schema["$defs"]["SphLinkInput"]["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .all(|variant| variant["required"]
                    .as_array()
                    .is_some_and(|required| required.iter().any(|field| field == "type")))
        );
        assert!(
            schema["$defs"]["SphLinkInput"]["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|variant| variant["required"]
                    .as_array()
                    .is_some_and(|required| required.iter().any(|field| field == "value")))
        );
        let publish_article = router.get("publish_article").unwrap();
        let article_schema = serde_json::to_value(&publish_article.input_schema).unwrap();
        assert_eq!(article_schema["additionalProperties"], false);
        assert_eq!(
            article_schema["properties"]["tags"]["type"],
            serde_json::json!(["string", "null"])
        );
        let list_accounts = router.get("list_accounts").unwrap();
        let accounts_schema = serde_json::to_value(&list_accounts.input_schema).unwrap();
        assert_eq!(
            accounts_schema["$defs"]["AccountsPlatform"]["enum"],
            serde_json::json!([
                "dy", "ks", "blbl", "bjh", "tt", "sph", "xhs", "juejin", "fqsp"
            ])
        );
        let list_history = router.get("list_history").unwrap();
        let history_schema = serde_json::to_value(&list_history.input_schema).unwrap();
        assert_eq!(
            history_schema["$defs"]["HistoryPlatform"]["enum"],
            serde_json::json!(["dy", "ks", "blbl", "bjh", "tt", "sph", "xhs"])
        );
        assert_eq!(
            history_schema["$defs"]["HistoryStatusInput"]["enum"],
            serde_json::json!(["success", "failed", "publishing", "scheduled"])
        );
    }

    #[tokio::test]
    async fn macro_generated_router_returns_a_tool_error_for_unknown_input_fields() {
        let (client, server_handle) = connect(service()).await;
        let result = client
            .call_tool(
                CallToolRequestParams::new("publish_video").with_arguments(
                    serde_json::json!({
                        "platform": "dy",
                        "file": "/tmp/video.mp4",
                        "title": "Title",
                        "phone": "13800138000",
                        "cookie": "must-not-be-accepted"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        let message = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .unwrap();
        assert!(message.contains("unknown field `cookie`"));
        disconnect(client, server_handle).await;
    }

    #[tokio::test]
    async fn macro_generated_router_rejects_fqsp_history_filter() {
        let (client, server_handle) = connect(service()).await;
        let result = client
            .call_tool(
                CallToolRequestParams::new("list_history").with_arguments(
                    serde_json::json!({"platform":"fqsp"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        disconnect(client, server_handle).await;
    }

    #[tokio::test]
    async fn macro_generated_router_lists_no_accounts_from_fresh_state() {
        let (client, server_handle) = connect(service()).await;
        let result = client
            .call_tool(CallToolRequestParams::new("list_accounts"))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content, Some(serde_json::json!([])));
        disconnect(client, server_handle).await;
    }

    #[tokio::test]
    async fn macro_generated_router_returns_persisted_juejin_account_as_structured_array() {
        let service = service();
        service
            .repository
            .save_article_account(&ArticleAccount {
                id: "j".into(),
                platform: ArticlePlatform::Juejin,
                display_name: "Primary".into(),
                status: ArticleAccountStatus::LoggedIn,
                phone: "13800138000".into(),
                partition: "persist:j".into(),
            })
            .unwrap();
        let (client, server_handle) = connect(service).await;
        let result = client
            .call_tool(
                CallToolRequestParams::new("list_accounts").with_arguments(
                    serde_json::json!({"platform":"juejin"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            result.structured_content,
            Some(
                serde_json::json!([{"phone":"13800138000","platform":"juejin","partition":"persist:j"}])
            )
        );
        disconnect(client, server_handle).await;
    }
}
