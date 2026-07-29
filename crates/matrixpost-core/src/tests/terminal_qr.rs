#[test]
fn terminal_qr_login_protocol_is_versioned_strict_and_platform_limited() {
    let request = TerminalQrLoginRunnerRequest {
        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        platform: Platform::WechatChannels,
    };
    assert!(request.validate());
    assert_eq!(
        serde_json::to_string(&request).unwrap(),
        r#"{"version":1,"platform":"sph"}"#
    );
    assert!(serde_json::from_str::<TerminalQrLoginRunnerRequest>(
        r#"{"version":1,"platform":"dy","cookie":"forbidden"}"#
    )
    .is_err());
    assert!(!TerminalQrLoginRunnerRequest {
        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        platform: Platform::Bilibili,
    }
    .validate());
    assert!(serde_json::from_str::<TerminalQrLoginRunnerResponse>(
        r#"{"outcome":"unavailable","version":1,"platform":"dy","diagnostic":"forbidden"}"#
    )
    .is_err());
    let refresh = TerminalQrLoginRefreshRequest {
        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        platform: Platform::Douyin,
        attempt_token: "attempt-1".into(),
    };
    assert!(refresh.validate());
    assert_eq!(
        serde_json::to_string(&refresh).unwrap(),
        r#"{"version":1,"platform":"dy","attempt_token":"attempt-1"}"#
    );
    assert!(serde_json::from_str::<TerminalQrLoginRefreshRequest>(
        r#"{"version":1,"platform":"dy","attempt_token":"attempt-1","profile":"forbidden"}"#
    )
    .is_err());
    assert!(!TerminalQrLoginCancelRequest {
        version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
        platform: Platform::WechatChannels,
        attempt_token: "not valid".into(),
    }
    .validate());
}

#[test]
fn terminal_qr_login_uses_only_loopback_endpoint_and_safe_payload() {
    struct Transport {
        captured: Mutex<Option<(String, String)>>,
    }

    impl TerminalQrLoginHttpTransport for Transport {
        fn post_json(
            &self,
            endpoint: &str,
            body: &str,
        ) -> Result<(u16, String), TerminalQrLoginTransportError> {
            *self.captured.lock().unwrap() = Some((endpoint.into(), body.into()));
            Ok((
                    200,
                    r#"{"outcome":"qr_available","version":1,"platform":"dy","attempt_token":"attempt-1","png_base64":"iVBORw0KGgo="}"#
                        .into(),
                ))
        }
    }

    let runner = ProviderRunner::parse_cli("dy=tcp:127.0.0.1:39001").unwrap();
    let transport = Transport {
        captured: Mutex::new(None),
    };
    assert!(matches!(
        runner.request_terminal_qr_login_with(&transport).unwrap(),
        TerminalQrLoginOutcome::QrAvailable {
            attempt_token,
            png_base64
        } if attempt_token == "attempt-1" && png_base64 == "iVBORw0KGgo="
    ));
    assert_eq!(
        transport.captured.lock().unwrap().take(),
        Some((
            "http://127.0.0.1:39001/v1/login/terminal-qr".into(),
            r#"{"version":1,"platform":"dy"}"#.into(),
        ))
    );
    assert!(matches!(
        runner
            .refresh_terminal_qr_login_with("attempt-1", &transport)
            .unwrap(),
        TerminalQrLoginOutcome::QrAvailable { .. }
    ));
    assert_eq!(
        transport.captured.lock().unwrap().take(),
        Some((
            "http://127.0.0.1:39001/v1/login/terminal-qr/refresh".into(),
            r#"{"version":1,"platform":"dy","attempt_token":"attempt-1"}"#.into(),
        ))
    );
    assert!(matches!(
        runner
            .cancel_terminal_qr_login_with("attempt-1", &transport)
            .unwrap(),
        TerminalQrLoginOutcome::QrAvailable { .. }
    ));
    assert_eq!(
        transport.captured.lock().unwrap().take(),
        Some((
            "http://127.0.0.1:39001/v1/login/terminal-qr/cancel".into(),
            r#"{"version":1,"platform":"dy","attempt_token":"attempt-1"}"#.into(),
        ))
    );
    assert_eq!(
        runner
            .refresh_terminal_qr_login_with("bad token", &transport)
            .unwrap(),
        TerminalQrLoginOutcome::Rejected
    );
    assert!(transport.captured.lock().unwrap().is_none());

    let remote = ProviderRunner {
        platform: Platform::Douyin,
        transport: ProviderRunnerTransport::Tcp {
            address: "192.0.2.1:39001".parse().unwrap(),
        },
    };
    assert_eq!(
        remote.request_terminal_qr_login_with(&transport).unwrap(),
        TerminalQrLoginOutcome::Unavailable
    );
}

#[test]
fn terminal_qr_login_rejects_malformed_oversize_and_invalid_state_responses() {
    struct Transport {
        response: (u16, String),
    }

    impl TerminalQrLoginHttpTransport for Transport {
        fn post_json(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(u16, String), TerminalQrLoginTransportError> {
            Ok(self.response.clone())
        }
    }

    let runner = ProviderRunner::parse_cli("sph=tcp:127.0.0.1:39001").unwrap();
    for response in [
            (200, "not-json".into()),
            (
                200,
                r#"{"outcome":"pending","version":1,"platform":"sph","attempt_token":"bad token"}"#.into(),
            ),
            (
                200,
                r#"{"outcome":"qr_available","version":1,"platform":"sph","attempt_token":"attempt","png_base64":"not-png"}"#.into(),
            ),
            (
                200,
                r#"{"outcome":"qr_available","version":1,"platform":"sph","attempt_token":"attempt","png_base64":"iVBORw0KGgoAAQ=A"}"#.into(),
            ),
            (200, "x".repeat(TERMINAL_QR_LOGIN_RESPONSE_MAX_BYTES + 1)),
            (503, String::new()),
        ] {
            assert_eq!(
                runner
                    .request_terminal_qr_login_with(&Transport { response })
                    .unwrap(),
                TerminalQrLoginOutcome::Rejected
            );
        }
    assert_eq!(
        TerminalQrLoginRunnerResponse::TimedOut {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: Platform::WechatChannels,
        }
        .into_terminal_qr_login(Platform::WechatChannels),
        Some(TerminalQrLoginOutcome::TimedOut)
    );
    assert_eq!(
        TerminalQrLoginRunnerResponse::Cancelled {
            version: TERMINAL_QR_LOGIN_RUNNER_PROTOCOL_VERSION,
            platform: Platform::WechatChannels,
        }
        .into_terminal_qr_login(Platform::WechatChannels),
        Some(TerminalQrLoginOutcome::Cancelled)
    );
}
