//! Local-only bridge between MatriXpost and a separately managed WebDriver.
//!
//! This process neither reads browser profiles nor accepts credentials. A user
//! starts ChromeDriver (or another compatible WebDriver) separately with their
//! own local browser state, then explicitly points this runner at its loopback
//! endpoint.

mod config;
mod profiles;
mod service;
mod webdriver;

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    config::run().await
}
