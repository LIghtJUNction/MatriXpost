//! JSON-first command-line adapter for the portable MatriXpost core.

mod app;
mod args;
mod lifecycle;
mod output;
mod query;
mod runners;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
