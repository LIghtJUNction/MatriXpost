//! Portable, side-effect-free publication domain model and durable state ports.
//!
//! This crate deliberately stores account *metadata* only. Browser sessions and
//! passwords belong to provider implementations and are never serialised here.

mod error;
mod lifecycle;
mod media;
mod runner;
mod storage;
mod types;

pub use error::*;
pub use lifecycle::*;
pub use media::*;
pub use runner::*;
pub use storage::*;
pub use types::*;

#[cfg(test)]
mod tests;
