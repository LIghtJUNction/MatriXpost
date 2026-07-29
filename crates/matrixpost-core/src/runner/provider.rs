//! Video provider transport, local runner declarations, and dispatch registry.

mod config;
mod login;
mod registry;
mod terminal_qr;
mod transport;

pub use config::{ProviderRunner, ProviderRunnerConfigError, ProviderRunnerTransport};
pub use login::{
    AccountStatusHttpTransport, ManualLoginHttpTransport, ManualLoginTransportError,
    ReviewStatusHttpTransport,
};
pub use registry::{ProviderDispatchReport, ProviderRegistrationError, ProviderRegistry};
pub use terminal_qr::{TerminalQrLoginHttpTransport, TerminalQrLoginTransportError};
pub use transport::{ProviderRunnerRequest, ProviderRunnerResponse, PublishProvider};

pub(crate) use config::{contains_credential_like_term, reject_credential_like_endpoint};
// Intentional internal injection boundary used by sibling crate tests.
#[allow(unused_imports)]
pub(crate) use transport::RunnerHttpTransport;
pub(crate) use transport::TcpRunnerProvider;
