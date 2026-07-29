//! JSON-first command-line adapter for the portable MatriXpost core.

mod app;
mod args;
mod batch;
mod lifecycle;
mod output;
mod query;
mod runners;
mod terminal_qr;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
