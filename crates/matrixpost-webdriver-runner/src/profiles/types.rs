use std::time::Duration;

use matrixpost_core::Platform;

use super::{ACKNOWLEDGEMENT_ATTEMPTS, ACKNOWLEDGEMENT_INTERVAL};

#[derive(Clone, Copy)]
pub(crate) struct AcknowledgementPolicy {
    pub(crate) attempts: usize,
    pub(crate) interval: Duration,
}

impl AcknowledgementPolicy {
    pub(crate) const fn production() -> Self {
        Self {
            attempts: ACKNOWLEDGEMENT_ATTEMPTS,
            interval: ACKNOWLEDGEMENT_INTERVAL,
        }
    }
}

/// The UI profile is deliberately data, not per-platform automation code.
pub(crate) struct PlatformProfile {
    pub(crate) platform: Platform,
    pub(crate) upload_url: &'static str,
    pub(crate) file: &'static [&'static str],
    pub(crate) title: &'static [&'static str],
    pub(crate) short_title: Option<&'static [&'static str]>,
    pub(crate) description: &'static [&'static str],
    pub(crate) submit: &'static [&'static str],
    pub(crate) draft: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}

pub(crate) struct ArticleProfile {
    pub(crate) editor_url: &'static str,
    pub(crate) title: &'static [&'static str],
    pub(crate) content: &'static [&'static str],
    pub(crate) cover: &'static [&'static str],
    pub(crate) category: &'static [&'static str],
    pub(crate) tags: &'static [&'static str],
    pub(crate) summary: &'static [&'static str],
    pub(crate) publish_panel: &'static [&'static str],
    pub(crate) confirm: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}
