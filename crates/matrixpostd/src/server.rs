use std::future::Future;

use axum::Router;

/// Runs the HTTP server until the supplied shutdown future resolves.
///
/// Keeping the shutdown signal injectable makes the process lifecycle
/// testable without delivering a signal to the test runner.
pub(crate) async fn serve_until_shutdown<F>(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: F,
) -> Result<(), std::io::Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Waits for the signals used by interactive and systemd-managed processes.
#[cfg(unix)]
pub(crate) async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing SIGTERM handler must succeed");
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result.expect("installing Ctrl-C handler must succeed");
        }
        _ = terminate.recv() => {}
    }
}

/// Non-Unix targets receive the portable console interrupt signal only.
#[cfg(not(unix))]
pub(crate) async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("installing Ctrl-C handler must succeed");
}
