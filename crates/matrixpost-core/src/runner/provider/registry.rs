use super::{
    ProviderRunner, ProviderRunnerConfigError, ProviderRunnerTransport, PublishProvider,
    TcpRunnerProvider,
};
use crate::{
    error::DomainError,
    runner::{DispatchOutcome, ProviderAvailability},
    types::{Platform, PublishRequest},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Deterministic failure returned when a platform is registered more than once.
///
/// A registry never replaces an existing provider implicitly: replacing an
/// adapter can change the side-effecting backend used for a publication.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProviderRegistrationError {
    #[error("provider already registered for platform: {platform:?}")]
    Duplicate { platform: Platform },
}

/// Per-platform results from one multi-target provider dispatch.
///
/// The map is ordered by [`Platform`], rather than registration or request
/// order, so callers get stable output across runs.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderDispatchReport {
    pub outcomes: BTreeMap<Platform, DispatchOutcome>,
}

/// Explicit registry for installed publication providers.
///
/// The registry contains no browser sessions or credentials. It only owns
/// provider implementations supplied by the embedding application. An absent
/// platform is a normal, explicit unavailable result, never an implied
/// automation capability.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<Platform, Box<dyn PublishProvider>>,
    runners: BTreeMap<Platform, ProviderRunner>,
}

impl ProviderRegistry {
    /// Creates an empty provider registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a registry with validated local runner declarations.
    ///
    /// Loopback-TCP declarations install the stable local runner adapter.
    /// Unix sockets and Windows named pipes remain declared-but-unavailable
    /// until those transports receive an audited implementation.
    pub fn from_runners(
        runners: impl IntoIterator<Item = ProviderRunner>,
    ) -> Result<Self, ProviderRunnerConfigError> {
        let mut registry = Self::new();
        for runner in runners {
            runner.validate()?;
            let platform = runner.platform;
            if registry.runners.insert(platform, runner).is_some() {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
        }
        for (platform, address) in
            registry
                .runners
                .values()
                .filter_map(|runner| match runner.transport {
                    ProviderRunnerTransport::Tcp { address } => Some((runner.platform, address)),
                    _ => None,
                })
        {
            if registry.providers.contains_key(&platform) {
                return Err(ProviderRunnerConfigError::DuplicatePlatform { platform });
            }
            registry
                .providers
                .insert(platform, Box::new(TcpRunnerProvider { platform, address }));
        }
        Ok(registry)
    }

    /// Registers a provider without allowing an implicit replacement.
    pub fn register(
        &mut self,
        provider: Box<dyn PublishProvider>,
    ) -> Result<(), ProviderRegistrationError> {
        let platform = provider.platform();
        if self.providers.contains_key(&platform) {
            return Err(ProviderRegistrationError::Duplicate { platform });
        }
        self.providers.insert(platform, provider);
        Ok(())
    }

    /// Returns an installed provider's declared availability.
    pub fn availability(&self, platform: Platform) -> ProviderAvailability {
        self.providers
            .get(&platform)
            .map(|provider| provider.availability())
            .unwrap_or_else(|| ProviderAvailability::Unavailable {
                reason: self.unregistered_reason(platform),
            })
    }

    /// Returns every known platform's availability in canonical platform order.
    pub fn availability_report(&self) -> BTreeMap<Platform, ProviderAvailability> {
        Platform::ALL
            .iter()
            .copied()
            .map(|platform| (platform, self.availability(platform)))
            .collect()
    }

    /// Dispatches one target after proving that target belongs to the request.
    ///
    /// Provider errors are retained as errors. Missing providers and providers
    /// that declare themselves unavailable are ordinary dispatch outcomes, so
    /// callers can safely aggregate partial multi-target results.
    pub fn dispatch(
        &self,
        platform: Platform,
        request: &PublishRequest,
    ) -> Result<DispatchOutcome, DomainError> {
        request.validate()?;
        if !request.targets.contains(&platform) {
            return Err(DomainError::ProviderPlatformNotTarget { platform });
        }

        let Some(provider) = self.providers.get(&platform) else {
            return Ok(DispatchOutcome::Unavailable {
                reason: self.unregistered_reason(platform),
            });
        };

        match provider.availability() {
            ProviderAvailability::Available => provider.enqueue(request),
            ProviderAvailability::Unavailable { reason } => {
                Ok(DispatchOutcome::Unavailable { reason })
            }
        }
    }

    /// Dispatches every requested target and preserves one outcome per platform.
    ///
    /// A malformed request is rejected before any provider is touched. Once the
    /// request is valid, an individual provider failure becomes that target's
    /// rejected outcome and cannot prevent the remaining targets from running.
    pub fn dispatch_all(
        &self,
        request: &PublishRequest,
    ) -> Result<ProviderDispatchReport, DomainError> {
        request.validate()?;
        let mut outcomes = BTreeMap::new();
        for platform in request.targets.iter().copied() {
            let outcome = match self.dispatch(platform, request) {
                Ok(outcome) => outcome,
                Err(error) => DispatchOutcome::Rejected {
                    reason: error.to_string(),
                },
            };
            outcomes.insert(platform, outcome);
        }
        Ok(ProviderDispatchReport { outcomes })
    }

    fn unregistered_reason(&self, platform: Platform) -> String {
        self.runners
            .get(&platform)
            .map(ProviderRunner::unavailable_reason)
            .unwrap_or_else(|| format!("no provider registered for {}", platform.as_str()))
    }
}
