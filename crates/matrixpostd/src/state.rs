use std::sync::Arc;

use matrixpost_core::{ProviderRegistry, SqliteRepository};

pub(crate) struct AppState<R = SqliteRepository> {
    pub(crate) repository: Arc<R>,
    pub(crate) providers: Arc<ProviderRegistry>,
}
impl<R> Clone for AppState<R> {
    fn clone(&self) -> Self {
        Self {
            repository: Arc::clone(&self.repository),
            providers: Arc::clone(&self.providers),
        }
    }
}
