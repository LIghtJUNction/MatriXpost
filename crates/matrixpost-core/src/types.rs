//! Platform, account, and publication data-transfer models.
//!
//! This facade keeps the stable `crate::types::*` and crate-root re-export
//! paths while the domain models remain in focused private modules.

mod account;
mod article;
mod video;

pub use account::*;
pub use article::*;
pub use video::*;
