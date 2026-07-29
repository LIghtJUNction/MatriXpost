use super::super::{AccountSelection, Platform};
use super::{LocalSchedule, MediaSource, PlatformOverride, WechatLink};
use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A complete upstream-compatible publication command. Validation has no side effects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublishRequest {
    pub source: MediaSource,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub draft: bool,
    /// Upstream `bt2` compatibility toggle, retained losslessly for adapters.
    #[serde(default)]
    pub bt2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<LocalSchedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(default, skip_serializing_if = "AccountSelection::is_empty")]
    pub account: AccountSelection,
    #[serde(default)]
    pub wechat_link: WechatLink,
    #[serde(default)]
    pub overrides: Vec<PlatformOverride>,
    pub targets: Vec<Platform>,
}

impl PublishRequest {
    /// Returns a copy safe to cross a local runner boundary.
    ///
    /// Account routing is resolved by an embedding before dispatch and is
    /// deliberately never exposed to a runner or browser adapter.
    pub fn runner_safe(&self) -> Self {
        let mut safe = self.clone();
        safe.account = AccountSelection::default();
        for override_value in &mut safe.overrides {
            override_value.account = None;
        }
        safe
    }

    /// Returns true if this request still carries account routing.
    pub fn has_account_routing(&self) -> bool {
        !self.account.is_empty()
            || self
                .overrides
                .iter()
                .filter_map(|override_value| override_value.account.as_ref())
                .any(|account| !account.is_empty())
    }

    /// Rejects malformed data before repository, provider, or network interaction.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.title.trim().is_empty() {
            return Err(DomainError::EmptyTitle);
        }
        if self
            .short_title
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DomainError::EmptyShortTitle);
        }
        if self
            .task_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DomainError::EmptyTaskName);
        }
        if self.targets.is_empty() {
            return Err(DomainError::MissingTargets);
        }
        if self.targets.iter().copied().collect::<BTreeSet<_>>().len() != self.targets.len() {
            return Err(DomainError::DuplicateTargets);
        }
        if self
            .overrides
            .iter()
            .map(|item| item.platform)
            .collect::<BTreeSet<_>>()
            .len()
            != self.overrides.len()
        {
            return Err(DomainError::DuplicateOverrides);
        }
        if self
            .overrides
            .iter()
            .any(|item| !self.targets.contains(&item.platform))
        {
            return Err(DomainError::OverrideOutsideTargets);
        }
        if let Some(schedule) = &self.scheduled_at {
            schedule.as_naive()?;
        }
        match &self.source {
            MediaSource::LocalFile(path) if path.as_os_str().is_empty() => {
                Err(DomainError::EmptyLocalPath)
            }
            MediaSource::RemoteUrl(url) if !matches!(url.scheme(), "http" | "https") => Err(
                DomainError::UnsupportedRemoteScheme(url.scheme().to_owned()),
            ),
            _ => Ok(()),
        }
    }
}
