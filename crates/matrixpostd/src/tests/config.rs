use super::support::*;
use crate::config::default_bind;

#[test]
fn config_defaults_are_secret_free() {
    let config = DaemonConfig::default();
    assert_eq!(config.bind, default_bind());
    assert_eq!(config.state_path, PathBuf::from("matrixpost.db"));
    assert!(config.provider_runners.is_empty());
}
#[test]
fn daemon_config_builds_tcp_runner_execution_registry() {
    let config: DaemonConfig = toml::from_str(
        r#"
        [[provider_runners]]
        platform = 'dy'
        transport = 'tcp'
        address = '127.0.0.1:39001'
        "#,
    )
    .unwrap();
    let registry = ProviderRegistry::from_runners(config.provider_runners)
        .expect("runner declaration must validate");
    assert_eq!(
        registry.availability(Platform::Douyin),
        matrixpost_core::ProviderAvailability::Available
    );
}

#[test]
fn daemon_config_rejects_invalid_scheduler_bounds() {
    for config in [
        "scheduler_interval_seconds = 0",
        "scheduler_batch_size = 0",
        "scheduler_batch_size = 65",
    ] {
        assert!(
            toml::from_str::<DaemonConfig>(config)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}

#[test]
fn daemon_config_accepts_loopback_article_runner_and_rejects_remote_one() {
    let valid: DaemonConfig = toml::from_str(
        r#"
        [article_runner]
        address = '127.0.0.1:39002'
        "#,
    )
    .unwrap();
    assert!(valid.validate().is_ok());

    let remote: DaemonConfig = toml::from_str(
        r#"
        [article_runner]
        address = '192.0.2.1:39002'
        "#,
    )
    .unwrap();
    assert!(remote.validate().is_err());
}
