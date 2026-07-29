use std::{ffi::OsStr, path::PathBuf, sync::Arc};

use matrixpost_core::{ArticleRunner, ProviderRegistry, ProviderRunner};

use crate::{DEFAULT_STATE_PATH, LOG_ENV};

pub(crate) struct McpConfig {
    pub(crate) state_path: PathBuf,
    pub(crate) provider_registry: Arc<ProviderRegistry>,
    pub(crate) provider_runners: Arc<Vec<ProviderRunner>>,
    pub(crate) article_runner: Option<ArticleRunner>,
}

pub(crate) fn mcp_config(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<McpConfig, String> {
    let mut state_path = None;
    let mut provider_runners = Vec::new();
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
        } else if argument == "--provider-runner" {
            let value = args.next().ok_or_else(|| {
                "--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".to_owned()
            })?;
            provider_runners.push(mcp_provider_runner(&value)?);
        } else if let Some(value) = argument.strip_prefix("--provider-runner=") {
            if value.is_empty() {
                return Err("--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".into());
            }
            provider_runners.push(mcp_provider_runner(value)?);
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
    let provider_registry = ProviderRegistry::from_runners(provider_runners.clone())
        .map_err(|error| error.to_string())?;
    Ok(McpConfig {
        state_path: state_path
            .or_else(|| env_path.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_PATH)),
        provider_registry: Arc::new(provider_registry),
        provider_runners: Arc::new(provider_runners),
        article_runner,
    })
}

fn mcp_provider_runner(value: &str) -> Result<ProviderRunner, String> {
    let runner = ProviderRunner::parse_cli(value).map_err(|error| error.to_string())?;
    if runner.loopback_tcp_address().is_none() {
        return Err("--provider-runner requires PLATFORM=tcp:127.0.0.1:PORT".into());
    }
    Ok(runner)
}

#[cfg(test)]
pub(crate) fn state_path(
    args: impl IntoIterator<Item = String>,
    env_path: Option<&OsStr>,
) -> Result<PathBuf, String> {
    mcp_config(args, env_path).map(|config| config.state_path)
}

pub(crate) fn logging_enabled(value: Option<&OsStr>) -> bool {
    matches!(value.and_then(OsStr::to_str), Some("1" | "true" | "yes"))
}

pub(crate) fn log_error(message: impl std::fmt::Display) {
    if logging_enabled(std::env::var_os(LOG_ENV).as_deref()) {
        eprintln!("matrixpost-mcp: {message}");
    }
}
