use matrixpost_core::DomainError;
use serde::Serialize;
use thiserror::Error;

/// IPC-safe error returned to the static frontend.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum DesktopError {
    #[error("invalid local draft: {0}")]
    InvalidRequest(String),
    #[error("local lifecycle record was not found: {0}")]
    NotFound(String),
    #[error("local state is unavailable: {0}")]
    Storage(String),
}

impl From<DomainError> for DesktopError {
    fn from(error: DomainError) -> Self {
        Self::InvalidRequest(error.to_string())
    }
}
