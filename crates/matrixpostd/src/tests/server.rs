use super::support::*;
use crate::server::serve_until_shutdown;

#[tokio::test]
async fn serve_until_shutdown_exits_cleanly_when_shutdown_is_requested() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener must bind");
    let (shutdown, receiver) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_until_shutdown(
        listener,
        app(AppState {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
        }),
        async move {
            receiver
                .await
                .expect("shutdown sender must remain available");
        },
    ));

    shutdown
        .send(())
        .expect("shutdown request must be delivered");
    let result = tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("graceful server shutdown must not hang")
        .expect("server task must not panic");
    assert!(result.is_ok());
}
