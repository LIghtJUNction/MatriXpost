use crate::types::Platform;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, str::FromStr};
use thiserror::Error;

/// A credential-free description of a local runner owned by an embedding.
///
/// MatriXpost never launches a runner. A loopback-TCP declaration installs the
/// stable v1 HTTP adapter and opens the endpoint only when dispatching a valid
/// request; other declared transports remain visibility-only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRunner {
    pub platform: Platform,
    #[serde(flatten)]
    pub transport: ProviderRunnerTransport,
}

/// Local-only transports understood by a [`ProviderRunner`] declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum ProviderRunnerTransport {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
    Tcp { address: SocketAddr },
}

/// Invalid provider-runner configuration.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderRunnerConfigError {
    #[error("provider runner platform is configured more than once: {platform:?}")]
    DuplicatePlatform { platform: Platform },
    #[error("provider runner unix socket path must be absolute")]
    UnixSocketPathMustBeAbsolute,
    #[error("provider runner named pipe must use the \\\\.\\pipe\\ namespace")]
    NamedPipeMustBeLocal,
    #[error("provider runner TCP address must bind to loopback")]
    TcpMustBeLoopback,
    #[error("provider runner endpoint must not contain credential-like data")]
    CredentialLikeEndpoint,
    #[error("provider runner argument must use PLATFORM=TRANSPORT:ENDPOINT")]
    InvalidArgument,
}

impl ProviderRunner {
    /// Validates that this is a local, credential-free runner declaration.
    pub fn validate(&self) -> Result<(), ProviderRunnerConfigError> {
        match &self.transport {
            ProviderRunnerTransport::UnixSocket { path } => {
                if !path.is_absolute() {
                    return Err(ProviderRunnerConfigError::UnixSocketPathMustBeAbsolute);
                }
                reject_credential_like_endpoint(&path.to_string_lossy())?;
            }
            ProviderRunnerTransport::NamedPipe { name } => {
                if !name.starts_with(r"\\.\pipe\") {
                    return Err(ProviderRunnerConfigError::NamedPipeMustBeLocal);
                }
                reject_credential_like_endpoint(name)?;
            }
            ProviderRunnerTransport::Tcp { address } => {
                if !address.ip().is_loopback() {
                    return Err(ProviderRunnerConfigError::TcpMustBeLoopback);
                }
            }
        }
        Ok(())
    }

    /// Parses the CLI form `PLATFORM=unix:/absolute/path`,
    /// `PLATFORM=pipe:\\\\.\\pipe\\name`, or `PLATFORM=tcp:127.0.0.1:PORT`.
    pub fn parse_cli(value: &str) -> Result<Self, ProviderRunnerConfigError> {
        let (platform, transport) = value
            .split_once('=')
            .ok_or(ProviderRunnerConfigError::InvalidArgument)?;
        let platform =
            Platform::from_str(platform).map_err(|_| ProviderRunnerConfigError::InvalidArgument)?;
        let transport = if let Some(path) = transport.strip_prefix("unix:") {
            ProviderRunnerTransport::UnixSocket {
                path: PathBuf::from(path),
            }
        } else if let Some(name) = transport.strip_prefix("pipe:") {
            ProviderRunnerTransport::NamedPipe {
                name: name.to_owned(),
            }
        } else if let Some(address) = transport.strip_prefix("tcp:") {
            ProviderRunnerTransport::Tcp {
                address: address
                    .parse()
                    .map_err(|_| ProviderRunnerConfigError::InvalidArgument)?,
            }
        } else {
            return Err(ProviderRunnerConfigError::InvalidArgument);
        };
        let runner = Self {
            platform,
            transport,
        };
        runner.validate()?;
        Ok(runner)
    }

    pub(crate) fn unavailable_reason(&self) -> String {
        let transport = match &self.transport {
            ProviderRunnerTransport::UnixSocket { .. } => "Unix socket",
            ProviderRunnerTransport::NamedPipe { .. } => "named pipe",
            ProviderRunnerTransport::Tcp { .. } => "loopback TCP",
        };
        format!(
            "{transport} runner configured for {}; no execution adapter is installed",
            self.platform.as_str()
        )
    }

    /// Returns the configured loopback TCP endpoint, if this declaration can
    /// use the local HTTP runner protocol.
    ///
    /// Callers must not substitute arbitrary endpoints: this accessor enforces
    /// loopback-only TCP even if a caller constructed the public fields
    /// directly instead of using [`Self::validate`].
    pub fn loopback_tcp_address(&self) -> Option<SocketAddr> {
        match &self.transport {
            ProviderRunnerTransport::Tcp { address } if address.ip().is_loopback() => {
                Some(*address)
            }
            ProviderRunnerTransport::UnixSocket { .. }
            | ProviderRunnerTransport::NamedPipe { .. }
            | ProviderRunnerTransport::Tcp { .. } => None,
        }
    }
}

const CREDENTIAL_LIKE_TERMS: &[&str] = &[
    "cookie",
    "token",
    "password",
    "secret",
    "session",
    "authorization",
    "credential",
];

pub(crate) fn contains_credential_like_term(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    CREDENTIAL_LIKE_TERMS
        .iter()
        .any(|needle| lower.contains(needle))
}

pub(crate) fn reject_credential_like_endpoint(
    value: &str,
) -> Result<(), ProviderRunnerConfigError> {
    if contains_credential_like_term(value)
        || value
            .chars()
            .any(|character| matches!(character, '@' | '?' | '#'))
    {
        return Err(ProviderRunnerConfigError::CredentialLikeEndpoint);
    }
    Ok(())
}
