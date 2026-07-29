use std::{num::NonZeroUsize, path::PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand};
use matrixpost_core::{
    ApprovalStatus, BusinessObjectStatus, HistoryStatus, LedgerDirection, LocalSchedule, Platform,
};

use crate::query::{
    parse_approval_status, parse_business_object_status, parse_currency, parse_history_date,
    parse_history_platform, parse_ledger_direction, parse_positive_minor_amount, parse_rfc3339,
    parse_video_platform,
};

/// MatriXpost CLI. Mutating commands never claim that a provider published media.
#[derive(Debug, Parser)]
#[command(name = "matrixpost", version, about)]
pub(crate) struct Cli {
    #[arg(long, global = true, default_value = "matrixpost.db")]
    pub(crate) state_path: PathBuf,
    /// Declare a local runner without executing it: PLATFORM=unix:/path,
    /// PLATFORM=pipe:\\\\.\\pipe\\name, or PLATFORM=tcp:127.0.0.1:PORT.
    #[arg(long, global = true, value_name = "RUNNER")]
    pub(crate) provider_runner: Vec<String>,
    /// Declare the explicit Juejin article runner: tcp:127.0.0.1:PORT.
    #[arg(long, global = true, value_name = "RUNNER")]
    pub(crate) article_runner: Vec<String>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Login {
        #[arg(short, long)]
        platform: String,
        /// Render a local runner QR code in this terminal (Douyin and WeChat Channels only).
        #[arg(long)]
        terminal_qr: bool,
    },
    Publish(PublishArgs),
    #[command(name = "publish-article")]
    PublishArticle {
        #[arg(short, long, alias = "juejin", alias = "掘金")]
        platform: String,
        #[arg(short, long)]
        title: String,
        #[arg(long)]
        phone: Option<String>,
        #[arg(long)]
        partition: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        cover: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long = "publish-at")]
        publish_at: Option<LocalSchedule>,
    },
    Accounts(AccountsArgs),
    /// Query only a bounded Fanqie title's safe review-status label through a matching explicit local runner.
    #[command(name = "review-status")]
    ReviewStatus {
        #[arg(long)]
        title: String,
    },
    History(HistoryArgs),
    /// List terminal scheduled-article local workflow records.
    #[command(name = "article-history")]
    ArticleHistory,
    /// Show deterministic availability for every supported platform.
    Providers {
        #[arg(long)]
        json: bool,
    },
    /// Manage generic objects, immutable financial entries, and content attribution.
    Lifecycle(LifecycleArgs),
}

#[derive(Debug, Args)]
pub(crate) struct AccountsArgs {
    #[arg(long)]
    pub(crate) json: bool,
    /// Exact video platform code or one of its established aliases.
    #[arg(short, long, value_parser = parse_video_platform)]
    pub(crate) platform: Option<Platform>,
    /// Exact non-secret account phone route.
    #[arg(long)]
    pub(crate) phone: Option<String>,
    /// Retain only accounts whose configured local runner reports ready.
    #[arg(long, conflicts_with = "logged_out")]
    pub(crate) logged_in: bool,
    /// Retain only accounts whose configured local runner reports not_ready.
    #[arg(long, conflicts_with = "logged_in")]
    pub(crate) logged_out: bool,
}

#[derive(Debug, Args)]
pub(crate) struct LifecycleArgs {
    #[command(subcommand)]
    pub(crate) command: LifecycleCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum LifecycleCommand {
    /// List all generic business objects.
    Objects,
    /// Create or inspect a generic business object.
    Object(ObjectArgs),
    /// List or append immutable ledger entries.
    Ledger(LedgerArgs),
    /// List or create links from published content to an object.
    Attribution(AttributionArgs),
    /// List or create immutable directed links between generic objects.
    Relation(RelationArgs),
    /// Change controlled object lifecycle and approval states.
    Transition(TransitionArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ObjectArgs {
    #[command(subcommand)]
    pub(crate) command: ObjectCommand,
}
#[derive(Debug, Subcommand)]
pub(crate) enum ObjectCommand {
    /// Read one object by stable ID.
    Get {
        #[arg(long)]
        id: String,
    },
    /// Create an object from a caller-defined template kind.
    Create(ObjectCreateArgs),
}
#[derive(Debug, Args)]
pub(crate) struct ObjectCreateArgs {
    #[arg(long)]
    pub(crate) id: String,
    #[arg(long)]
    pub(crate) kind: String,
    #[arg(long)]
    pub(crate) display_name: String,
    #[arg(long)]
    pub(crate) external_id: Option<String>,
    #[arg(long, default_value = "draft", value_parser = parse_business_object_status)]
    pub(crate) lifecycle_status: BusinessObjectStatus,
    #[arg(long, default_value = "pending", value_parser = parse_approval_status)]
    pub(crate) approval_status: ApprovalStatus,
    /// Object metadata as KEY=VALUE. Repeat the flag for multiple attributes.
    #[arg(long = "attribute", value_name = "KEY=VALUE")]
    pub(crate) attributes: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct LedgerArgs {
    #[command(subcommand)]
    pub(crate) command: LedgerCommand,
}
#[derive(Debug, Subcommand)]
pub(crate) enum LedgerCommand {
    /// List immutable ledger entries for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Append an immutable cost or income entry.
    Add(LedgerAddArgs),
}
#[derive(Debug, Args)]
pub(crate) struct LedgerAddArgs {
    #[arg(long)]
    pub(crate) id: String,
    #[arg(long = "object")]
    pub(crate) business_object_id: String,
    #[arg(long, value_parser = parse_ledger_direction)]
    pub(crate) direction: LedgerDirection,
    #[arg(long)]
    pub(crate) category: String,
    #[arg(long, value_parser = parse_positive_minor_amount)]
    pub(crate) amount_minor: i64,
    #[arg(long, value_parser = parse_currency)]
    pub(crate) currency: String,
    #[arg(long, default_value = "pending", value_parser = parse_approval_status)]
    pub(crate) approval_status: ApprovalStatus,
    #[arg(long, value_parser = parse_rfc3339)]
    pub(crate) occurred_at: Option<DateTime<Utc>>,
    #[arg(long)]
    pub(crate) counterparty: Option<String>,
    #[arg(long)]
    pub(crate) reference: Option<String>,
    #[arg(long)]
    pub(crate) description: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct AttributionArgs {
    #[command(subcommand)]
    pub(crate) command: AttributionCommand,
}
#[derive(Debug, Subcommand)]
pub(crate) enum AttributionCommand {
    /// List publication-history links for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Link one existing publication-history record to an object.
    Add(AttributionAddArgs),
}
#[derive(Debug, Args)]
pub(crate) struct AttributionAddArgs {
    #[arg(long = "object")]
    pub(crate) business_object_id: String,
    #[arg(long = "history")]
    pub(crate) history_id: String,
    #[arg(long, value_parser = parse_rfc3339)]
    pub(crate) created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
pub(crate) struct RelationArgs {
    #[command(subcommand)]
    pub(crate) command: RelationCommand,
}
#[derive(Debug, Subcommand)]
pub(crate) enum RelationCommand {
    /// List both incoming and outgoing relations for an object.
    List {
        #[arg(long = "object")]
        business_object_id: String,
    },
    /// Add an immutable directed relation between two existing objects.
    Add(RelationAddArgs),
}
#[derive(Debug, Args)]
pub(crate) struct RelationAddArgs {
    #[arg(long)]
    pub(crate) id: String,
    #[arg(long = "source")]
    pub(crate) source_business_object_id: String,
    #[arg(long = "target")]
    pub(crate) target_business_object_id: String,
    #[arg(long = "type")]
    pub(crate) relation_type: String,
    /// Relation metadata as KEY=VALUE. Repeat the flag for multiple attributes.
    #[arg(long = "attribute", value_name = "KEY=VALUE")]
    pub(crate) attributes: Vec<String>,
}
#[derive(Debug, Args)]
pub(crate) struct TransitionArgs {
    #[arg(long)]
    pub(crate) id: String,
    #[arg(long)]
    pub(crate) expected_revision: u64,
    #[arg(long, value_parser = parse_business_object_status)]
    pub(crate) lifecycle_status: BusinessObjectStatus,
    #[arg(long, value_parser = parse_approval_status)]
    pub(crate) approval_status: ApprovalStatus,
    #[arg(long, value_parser = parse_rfc3339)]
    pub(crate) updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Args)]
pub(crate) struct HistoryArgs {
    #[arg(long)]
    pub(crate) json: bool,
    /// Number of trailing days; defaults to seven unless --all is supplied.
    #[arg(long)]
    pub(crate) days: Option<u16>,
    /// Exact upstream history platform code (Fanqie video is not part of this query).
    #[arg(long, value_parser = parse_history_platform)]
    pub(crate) platform: Option<Platform>,
    /// One of success, failed, publishing, or scheduled.
    #[arg(long)]
    pub(crate) status: Option<HistoryStatus>,
    /// Exact non-secret account phone route stored on the publication request.
    #[arg(long)]
    pub(crate) phone: Option<String>,
    /// Maximum retained records after filtering, newest first.
    #[arg(short = 'n', long, default_value_t = NonZeroUsize::new(50).expect("nonzero"))]
    pub(crate) limit: NonZeroUsize,
    /// Inclusive local-calendar lower bound. Overrides --days and --all when supplied.
    #[arg(long, value_parser = parse_history_date)]
    pub(crate) since: Option<NaiveDate>,
    /// Inclusive local-calendar upper bound. Overrides --days and --all when supplied.
    #[arg(long, value_parser = parse_history_date)]
    pub(crate) until: Option<NaiveDate>,
    /// Return all local history without a trailing-days cutoff.
    #[arg(long)]
    pub(crate) all: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PublishArgs {
    #[arg(short = 'p', long = "platform", required = true)]
    pub(crate) platforms: Vec<String>,
    /// A single local path or an HTTP(S) media URL. Mutually exclusive with --dir.
    #[arg(
        short = 'f',
        long,
        required_unless_present = "dir",
        conflicts_with = "dir"
    )]
    pub(crate) file: Option<String>,
    /// A local directory whose direct media files are selected by --config/--xlsx.
    #[arg(long, required_unless_present = "file", conflicts_with = "file")]
    pub(crate) dir: Option<PathBuf>,
    /// XLSX batch configuration. Required for --dir and forbidden for --file.
    #[arg(long = "config", visible_alias = "xlsx", conflicts_with = "file")]
    pub(crate) config: Option<PathBuf>,
    /// Required for one file. Batch rows provide their own titles.
    #[arg(short = 't', long)]
    pub(crate) title: Option<String>,
    #[arg(long = "short-title")]
    pub(crate) short_title: Option<String>,
    #[arg(long = "tags", alias = "bq", value_delimiter = ',')]
    pub(crate) tags: Vec<String>,
    #[arg(long)]
    pub(crate) phone: Option<String>,
    #[arg(long)]
    pub(crate) partition: Option<String>,
    #[arg(long = "name", alias = "book-name")]
    pub(crate) task_name: Option<String>,
    #[arg(long)]
    pub(crate) bt2: Option<String>,
    #[arg(long)]
    pub(crate) address: Option<String>,
    #[arg(long = "publish-at")]
    pub(crate) publish_at: Option<LocalSchedule>,
    #[arg(long)]
    pub(crate) draft: bool,
    #[arg(long = "sph-product-id")]
    pub(crate) sph_product_id: Option<String>,
    #[arg(long = "sph-link-type")]
    pub(crate) sph_link_type: Option<String>,
    #[arg(long = "sph-link-value")]
    pub(crate) sph_link_value: Option<String>,
    /// JSON `PlatformOverride`; repeat once per platform override.
    #[arg(long = "platform-override")]
    pub(crate) platform_overrides: Vec<String>,
    /// Applies the same declaration statement to every selected platform.
    #[arg(long = "creative-statement")]
    pub(crate) creative_statement: Option<String>,
}
