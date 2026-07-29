//! Headless HTTP adapter backed by the durable core repository.

mod api;
mod config;
mod scheduler;
mod server;
mod state;

#[cfg(test)]
mod tests;

use std::{process::ExitCode, sync::Arc, time::Duration};

use clap::Parser;
use matrixpost_core::{ProviderRegistry, SqliteRepository};

use crate::{
    api::app,
    config::{Args, DaemonConfig},
    scheduler::scheduler_loop,
    server::{serve_until_shutdown, shutdown_signal},
    state::AppState,
};

#[tokio::main]
async fn main() -> ExitCode {
    let config = match DaemonConfig::load(Args::parse()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("matrixpostd configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let providers = match ProviderRegistry::from_runners(config.provider_runners) {
        Ok(providers) => Arc::new(providers),
        Err(error) => {
            eprintln!("matrixpostd provider-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let repository = match SqliteRepository::open(&config.state_path) {
        Ok(repository) => Arc::new(repository),
        Err(error) => {
            eprintln!(
                "matrixpostd failed to open {}: {error}",
                config.state_path.display()
            );
            return ExitCode::from(4);
        }
    };
    let listener = match tokio::net::TcpListener::bind(config.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("matrixpostd failed to bind {}: {error}", config.bind);
            return ExitCode::from(4);
        }
    };
    let state = AppState {
        repository,
        providers,
        article_runner: config.article_runner,
    };
    tokio::spawn(scheduler_loop(
        state.clone(),
        Duration::from_secs(config.scheduler_interval_seconds),
        config.scheduler_batch_size,
    ));
    match serve_until_shutdown(listener, app(state), shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("matrixpostd stopped unexpectedly: {error}");
            ExitCode::from(4)
        }
    }
}
