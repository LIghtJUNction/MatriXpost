use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use matrixpost_core::{ArticleRunner, ProviderRunner, PublicationQueue, SqliteRepository};
use serde::Deserialize;

/// Secret-free daemon configuration read from TOML.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DaemonConfig {
    #[serde(default = "default_bind")]
    pub(crate) bind: SocketAddr,
    #[serde(default = "default_state_path")]
    pub(crate) state_path: PathBuf,
    /// Explicit loopback local-runner declarations used by immediate HTTP
    /// dispatch and by due-job scheduler passes; neither path opens a browser
    /// or calls a platform directly.
    #[serde(default)]
    pub(crate) provider_runners: Vec<ProviderRunner>,
    /// Optional explicit loopback Juejin article runner for durable article jobs.
    #[serde(default)]
    pub(crate) article_runner: Option<ArticleRunner>,
    /// Period between durable due-job claim passes. Must be positive.
    #[serde(default = "default_scheduler_interval_seconds")]
    pub(crate) scheduler_interval_seconds: u64,
    /// Maximum jobs claimed in one scheduler pass, capped by the core queue.
    #[serde(default = "default_scheduler_batch_size")]
    pub(crate) scheduler_batch_size: usize,
}
pub(crate) fn default_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8788))
}
fn default_state_path() -> PathBuf {
    PathBuf::from("matrixpost.db")
}
const fn default_scheduler_interval_seconds() -> u64 {
    5
}
const fn default_scheduler_batch_size() -> usize {
    16
}
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            state_path: default_state_path(),
            provider_runners: Vec::new(),
            article_runner: None,
            scheduler_interval_seconds: default_scheduler_interval_seconds(),
            scheduler_batch_size: default_scheduler_batch_size(),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "matrixpostd",
    version,
    about = "Headless MatriXpost API daemon"
)]
pub(crate) struct Args {
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    #[arg(long)]
    pub(crate) bind: Option<SocketAddr>,
    #[arg(long)]
    pub(crate) state_path: Option<PathBuf>,
}

impl DaemonConfig {
    pub(crate) fn load(args: Args) -> Result<Self, String> {
        let mut config = match args.config {
            Some(path) => toml::from_str(
                &std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
            )
            .map_err(|error| format!("invalid TOML configuration: {error}"))?,
            None => Self::default(),
        };
        if let Some(bind) = args.bind {
            config.bind = bind;
        }
        if let Some(path) = args.state_path {
            config.state_path = path;
        }
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if let Some(runner) = &self.article_runner {
            runner.validate().map_err(|error| error.to_string())?;
        }
        if self.scheduler_interval_seconds == 0 {
            return Err("scheduler_interval_seconds must be greater than zero".into());
        }
        if self.scheduler_batch_size == 0
            || self.scheduler_batch_size > <SqliteRepository as PublicationQueue>::MAX_CLAIM_BATCH
        {
            return Err(format!(
                "scheduler_batch_size must be between 1 and {}",
                <SqliteRepository as PublicationQueue>::MAX_CLAIM_BATCH
            ));
        }
        Ok(())
    }
}
