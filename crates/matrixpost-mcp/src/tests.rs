use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use matrixpost_core::{
    ArticleAccountStatus, ArticlePlatform, ProviderAvailability, PublishProvider,
};
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
        provider_registry: Arc::new(ProviderRegistry::new()),
        provider_runners: Arc::new(Vec::new()),
        article_runner: None,
    }
}

struct QueuedProvider(Arc<AtomicUsize>);

impl PublishProvider for QueuedProvider {
    fn platform(&self) -> Platform {
        Platform::Douyin
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Available
    }

    fn enqueue(&self, _: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(DispatchOutcome::Queued {
            job_id: "local-job".into(),
        })
    }
}

fn service_with_queued_provider() -> (MatrixpostMcp, Arc<AtomicUsize>) {
    let mut provider_registry = ProviderRegistry::new();
    let calls = Arc::new(AtomicUsize::new(0));
    provider_registry
        .register(Box::new(QueuedProvider(Arc::clone(&calls))))
        .unwrap();
    (
        MatrixpostMcp {
            repository: Arc::new(SqliteRepository::in_memory().unwrap()),
            provider_registry: Arc::new(provider_registry),
            provider_runners: Arc::new(Vec::new()),
            article_runner: None,
        },
        calls,
    )
}

fn video_input(draft: Option<bool>, publish_at: Option<&str>) -> PublishVideoInput {
    PublishVideoInput {
        platform: VideoPlatform::Dy,
        file: "/tmp/video.mp4".into(),
        title: "Title".into(),
        phone: "13800138000".into(),
        bt2: None,
        tags: None,
        address: None,
        publish_at: publish_at.map(str::to_owned),
        show: None,
        draft,
        creative_statement: None,
        sph_product_id: None,
        sph_link: None,
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

async fn disconnect(client: RunningService<RoleClient, TestClient>, server_handle: JoinHandle<()>) {
    client.cancel().await.unwrap();
    server_handle.await.unwrap();
}

mod router;
mod unit;
