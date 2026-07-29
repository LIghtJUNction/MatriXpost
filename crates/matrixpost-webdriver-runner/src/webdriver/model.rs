use crate::profiles::AcknowledgementPolicy;
use matrixpost_core::{Platform, PublishArticleRequest, PublishRequest, ReviewStatus};
use serde_json::Value;
use std::{net::SocketAddr, sync::atomic::AtomicU64};

pub(crate) trait WebDriverTransport: Send + Sync {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String>;
}

pub(crate) trait PublicationExecutor: Send + Sync {
    fn publish(&self, platform: Platform, request: &PublishRequest) -> Result<String, String>;
}

/// Opens an existing attached browser at a platform page for a user-driven
/// login. It does not inspect session state or assert that login completed.
pub(crate) trait LoginNavigationExecutor: Send + Sync {
    fn open_manual_login(&self, platform: Platform) -> Result<(), String>;
}

/// Performs only an upload-form presence inference in an attached browser.
pub(crate) trait AccountStatusExecutor: Send + Sync {
    fn account_readiness(&self, platform: Platform) -> Result<bool, String>;
}

/// Looks up only a bounded Fanqie title in an attached browser and returns a
/// finite review outcome. It never returns DOM text or identifiers.
pub(crate) trait ReviewStatusExecutor: Send + Sync {
    fn review_status(&self, title_query: &str) -> Result<ReviewStatus, String>;
}

#[derive(Debug)]
pub(crate) struct ArticleExecutionError {
    pub(crate) reason: String,
    pub(crate) automation_attempted: bool,
}

impl ArticleExecutionError {
    pub(crate) fn local(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: false,
        }
    }

    pub(crate) fn attempted(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            automation_attempted: true,
        }
    }
}

pub(crate) trait ArticlePublicationExecutor: Send + Sync {
    fn publish_article(
        &self,
        request: &PublishArticleRequest,
    ) -> Result<String, ArticleExecutionError>;
}

pub(crate) struct WebDriverPublisher<T> {
    pub(crate) transport: T,
    pub(crate) browser_debugger_address: SocketAddr,
    pub(crate) acknowledgement: AcknowledgementPolicy,
    pub(crate) next_job: AtomicU64,
}
