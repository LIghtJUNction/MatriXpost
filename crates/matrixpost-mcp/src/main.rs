//! Local stdio MCP adapter for MatriXpost's credential-free SQLite state.
//!
//! The server never starts a browser, provider, shell, or daemon. Video
//! publication can use only an explicitly declared loopback local runner; it
//! never reports remote publication success.

use std::{process::ExitCode, sync::Arc};

use matrixpost_core::SqliteRepository;
use rmcp::{ServiceExt, transport::stdio};

#[cfg(test)]
use chrono::{NaiveDate, Utc};
#[cfg(test)]
use matrixpost_core::{
    ArticleAccount, ArticleDispatchOutcome, DispatchOutcome, DomainError, HistoryFilter,
    HistoryRecord, HistoryStatus, Platform, ProviderDispatchReport, ProviderRegistry,
    PublishRequest, PublishState, Repository,
};
#[cfg(test)]
use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf};

mod config;
mod model;
mod request;
mod service;

use config::*;
use model::*;
use service::*;

#[cfg(test)]
use request::*;

const DEFAULT_STATE_PATH: &str = "matrixpost.db";
const STATE_PATH_ENV: &str = "MATRIXPOST_STATE_PATH";
const LOG_ENV: &str = "MATRIXPOST_MCP_LOG";
const PROVIDER_MESSAGE: &str =
    "no local provider runner is configured; no remote publishing was attempted";

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
        provider_registry: config.provider_registry,
        provider_runners: config.provider_runners,
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
mod tests;
