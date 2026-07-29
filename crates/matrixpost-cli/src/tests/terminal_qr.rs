use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    process::ExitCode,
};

use clap::Parser;
use matrixpost_core::{Platform, TerminalQrLoginOutcome};

use crate::{
    args::{Cli, Command},
    terminal_qr::{
        TerminalQrLoginClient, render_qr_frame, run_terminal_qr_login, terminal_qr_preflight,
    },
};

const STATIC_QR_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQAAAAA3bvkkAAAACklEQVQI12NgAAAAAgAB4iG8MwAAAABJRU5ErkJggg==";

struct FakeTerminalQrClient {
    start: RefCell<Option<Result<TerminalQrLoginOutcome, String>>>,
    refreshes: RefCell<VecDeque<Result<TerminalQrLoginOutcome, String>>>,
    cancelled: Cell<usize>,
}

impl FakeTerminalQrClient {
    fn with_outcomes(
        start: Result<TerminalQrLoginOutcome, String>,
        refreshes: impl IntoIterator<Item = Result<TerminalQrLoginOutcome, String>>,
    ) -> Self {
        Self {
            start: RefCell::new(Some(start)),
            refreshes: RefCell::new(refreshes.into_iter().collect()),
            cancelled: Cell::new(0),
        }
    }
}

impl TerminalQrLoginClient for FakeTerminalQrClient {
    fn start(&self) -> Result<TerminalQrLoginOutcome, String> {
        self.start
            .borrow_mut()
            .take()
            .expect("start may only be called once")
    }

    fn refresh(&self, _: &str) -> Result<TerminalQrLoginOutcome, String> {
        self.refreshes
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(TerminalQrLoginOutcome::TimedOut))
    }

    fn cancel(&self, _: &str) -> Result<TerminalQrLoginOutcome, String> {
        self.cancelled.set(self.cancelled.get() + 1);
        Ok(TerminalQrLoginOutcome::Cancelled)
    }
}

#[test]
fn terminal_qr_flag_is_explicit_and_manual_login_remains_default() {
    let manual = Cli::try_parse_from(["matrixpost", "login", "--platform", "dy"]).unwrap();
    assert!(matches!(
        manual.command,
        Command::Login {
            terminal_qr: false,
            ..
        }
    ));
    let terminal =
        Cli::try_parse_from(["matrixpost", "login", "--platform", "sph", "--terminal-qr"]).unwrap();
    assert!(matches!(
        terminal.command,
        Command::Login {
            terminal_qr: true,
            ..
        }
    ));
}

#[test]
fn terminal_qr_rejects_non_tty_before_any_runner_request() {
    assert!(terminal_qr_preflight(false, Platform::Douyin).is_err());
    assert!(terminal_qr_preflight(true, Platform::Kuaishou).is_err());
    assert!(terminal_qr_preflight(true, Platform::WechatChannels).is_ok());
}

#[test]
fn terminal_qr_rendering_decodes_static_png_without_exposing_source_payload() {
    let rendered = render_qr_frame(STATIC_QR_PNG_BASE64).unwrap();
    assert!(rendered.contains('█'));
    assert!(!rendered.contains(STATIC_QR_PNG_BASE64));
    assert!(render_qr_frame("not-base64").is_err());
    assert!(render_qr_frame(&"A".repeat(1_048_580)).is_err());
}

#[test]
fn terminal_qr_refreshes_then_cancels_after_terminal_outcome() {
    let client = FakeTerminalQrClient::with_outcomes(
        Ok(TerminalQrLoginOutcome::QrAvailable {
            attempt_token: "attempt-1".into(),
            png_base64: STATIC_QR_PNG_BASE64.into(),
        }),
        [Ok(TerminalQrLoginOutcome::TimedOut)],
    );
    let frames = RefCell::new(Vec::new());
    let waits = Cell::new(0);
    let status = run_terminal_qr_login(
        &client,
        |frame| {
            frames.borrow_mut().push(frame.to_owned());
            Ok(())
        },
        || waits.set(waits.get() + 1),
    );
    assert_eq!(status, ExitCode::from(4));
    assert_eq!(waits.get(), 1);
    assert_eq!(client.cancelled.get(), 1);
    assert_eq!(frames.borrow().len(), 1);
}

#[test]
fn terminal_qr_cancels_when_terminal_write_fails() {
    let client = FakeTerminalQrClient::with_outcomes(
        Ok(TerminalQrLoginOutcome::QrAvailable {
            attempt_token: "attempt-2".into(),
            png_base64: STATIC_QR_PNG_BASE64.into(),
        }),
        [],
    );
    let status = run_terminal_qr_login(&client, |_| Err(io::ErrorKind::BrokenPipe.into()), || {});
    assert_eq!(status, ExitCode::from(4));
    assert_eq!(client.cancelled.get(), 1);
}
