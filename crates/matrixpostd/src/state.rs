use std::sync::Arc;

use matrixpost_core::{ArticleRunner, ProviderRegistry, SqliteRepository};

pub(crate) struct AppState<R = SqliteRepository> {
    pub(crate) repository: Arc<R>,
    pub(crate) providers: Arc<ProviderRegistry>,
    pub(crate) article_runner: Option<ArticleRunner>,
}
impl<R> Clone for AppState<R> {
    fn clone(&self) -> Self {
        Self {
            repository: Arc::clone(&self.repository),
            providers: Arc::clone(&self.providers),
            article_runner: self.article_runner.clone(),
        }
    }
}
