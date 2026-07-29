//! Credential-free terminal rendering for an explicitly requested local QR attempt.

use std::{
    io::{Cursor, IsTerminal, Write},
    process::ExitCode,
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use matrixpost_core::{Platform, ProviderRunner, TerminalQrLoginOutcome};

use crate::runners::login_runner;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const MAX_REFRESHES: usize = 20;
const MAX_QR_DIMENSION: u32 = 1024;
const MAX_QR_PIXELS: usize = 256 * 1024;
const MAX_QR_PNG_BYTES: usize = 768 * 1024;
const MAX_QR_BASE64_BYTES: usize = MAX_QR_PNG_BYTES.div_ceil(3) * 4;

pub(crate) trait TerminalQrLoginClient {
    fn start(&self) -> Result<TerminalQrLoginOutcome, String>;
    fn refresh(&self, attempt_token: &str) -> Result<TerminalQrLoginOutcome, String>;
    fn cancel(&self, attempt_token: &str) -> Result<TerminalQrLoginOutcome, String>;
}

impl TerminalQrLoginClient for ProviderRunner {
    fn start(&self) -> Result<TerminalQrLoginOutcome, String> {
        self.request_terminal_qr_login()
            .map_err(|error| error.to_string())
    }

    fn refresh(&self, attempt_token: &str) -> Result<TerminalQrLoginOutcome, String> {
        self.refresh_terminal_qr_login(attempt_token)
            .map_err(|error| error.to_string())
    }

    fn cancel(&self, attempt_token: &str) -> Result<TerminalQrLoginOutcome, String> {
        self.cancel_terminal_qr_login(attempt_token)
            .map_err(|error| error.to_string())
    }
}

/// Starts a terminal-only QR attempt after proving that its pixels have a TTY.
pub(crate) fn dispatch_terminal_qr_login(
    runners: &[ProviderRunner],
    platform: Platform,
) -> ExitCode {
    if let Err((code, message)) = terminal_qr_preflight(std::io::stdout().is_terminal(), platform) {
        return terminal_error(code, message);
    }
    let Some(runner) = login_runner(runners, platform) else {
        return terminal_error(
            3,
            "no local runner is configured for this platform; no QR login was attempted",
        );
    };

    let mut stdout = std::io::stdout().lock();
    run_terminal_qr_login(
        runner,
        |frame| stdout.write_all(frame.as_bytes()),
        || thread::sleep(REFRESH_INTERVAL),
    )
}

pub(crate) fn terminal_qr_preflight(
    stdout_is_terminal: bool,
    platform: Platform,
) -> Result<(), (u8, &'static str)> {
    if !stdout_is_terminal {
        return Err((
            2,
            "terminal QR login requires an interactive stdout terminal; no QR pixels were requested",
        ));
    }
    if !matches!(platform, Platform::Douyin | Platform::WechatChannels) {
        return Err((
            2,
            "terminal QR login supports only Douyin and WeChat Channels; no QR pixels were requested",
        ));
    }
    Ok(())
}

fn terminal_error(code: u8, message: &str) -> ExitCode {
    eprintln!("matrixpost: {message}");
    ExitCode::from(code)
}

fn cancel_attempt<C: TerminalQrLoginClient>(client: &C, attempt_token: &str) {
    let _ = client.cancel(attempt_token);
}

fn terminal_outcome_error<C: TerminalQrLoginClient>(
    client: &C,
    attempt_token: &str,
    code: u8,
    message: &str,
) -> ExitCode {
    cancel_attempt(client, attempt_token);
    terminal_error(code, message)
}

pub(crate) fn run_terminal_qr_login<C, W, S>(
    client: &C,
    mut write_frame: W,
    mut wait: S,
) -> ExitCode
where
    C: TerminalQrLoginClient,
    W: FnMut(&str) -> std::io::Result<()>,
    S: FnMut(),
{
    let initial = match client.start() {
        Ok(outcome) => outcome,
        Err(_) => {
            return terminal_error(
                4,
                "local runner QR request failed; no login success is asserted",
            );
        }
    };
    let (mut attempt_token, mut outcome) = match initial {
        TerminalQrLoginOutcome::QrAvailable {
            attempt_token,
            png_base64,
        } => (attempt_token, Some(png_base64)),
        TerminalQrLoginOutcome::Pending { attempt_token } => (attempt_token, None),
        TerminalQrLoginOutcome::Unavailable => {
            return terminal_error(3, "local runner is unavailable; no QR login was attempted");
        }
        TerminalQrLoginOutcome::Rejected => {
            return terminal_error(
                4,
                "local runner QR request was rejected; no login success is asserted",
            );
        }
        TerminalQrLoginOutcome::TimedOut | TerminalQrLoginOutcome::Cancelled => {
            return terminal_error(4, "local QR attempt ended before pixels were available");
        }
    };

    for refresh_index in 0..=MAX_REFRESHES {
        if let Some(png_base64) = outcome.take() {
            let frame = match render_qr_frame(&png_base64) {
                Ok(frame) => frame,
                Err(_) => {
                    return terminal_outcome_error(
                        client,
                        &attempt_token,
                        4,
                        "local runner returned invalid QR pixels; the attempt was cancelled",
                    );
                }
            };
            if write_frame(&frame).is_err() {
                return terminal_outcome_error(
                    client,
                    &attempt_token,
                    4,
                    "terminal QR rendering failed; the attempt was cancelled",
                );
            }
        }

        if refresh_index == MAX_REFRESHES {
            return terminal_outcome_error(
                client,
                &attempt_token,
                4,
                "terminal QR attempt timed out; no login success is asserted",
            );
        }

        wait();
        match client.refresh(&attempt_token) {
            Ok(TerminalQrLoginOutcome::QrAvailable {
                attempt_token: refreshed_token,
                png_base64,
            }) => {
                attempt_token = refreshed_token;
                outcome = Some(png_base64);
            }
            Ok(TerminalQrLoginOutcome::Pending {
                attempt_token: refreshed_token,
            }) => attempt_token = refreshed_token,
            Ok(TerminalQrLoginOutcome::Unavailable) => {
                return terminal_outcome_error(
                    client,
                    &attempt_token,
                    3,
                    "local runner became unavailable; the QR attempt was cancelled",
                );
            }
            Ok(TerminalQrLoginOutcome::Rejected) | Err(_) => {
                return terminal_outcome_error(
                    client,
                    &attempt_token,
                    4,
                    "local runner QR refresh failed; the attempt was cancelled",
                );
            }
            Ok(TerminalQrLoginOutcome::TimedOut) | Ok(TerminalQrLoginOutcome::Cancelled) => {
                return terminal_outcome_error(
                    client,
                    &attempt_token,
                    4,
                    "terminal QR attempt ended; no login success is asserted",
                );
            }
        }
    }
    unreachable!("the bounded refresh loop always returns")
}

pub(crate) fn render_qr_frame(png_base64: &str) -> Result<String, ()> {
    if png_base64.len() > MAX_QR_BASE64_BYTES {
        return Err(());
    }
    let png_bytes = STANDARD.decode(png_base64).map_err(|_| ())?;
    if png_bytes.len() > MAX_QR_PNG_BYTES {
        return Err(());
    }

    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_limits(png::Limits {
        bytes: MAX_QR_PNG_BYTES,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| ())?;
    let info = reader.info();
    let pixel_count = usize::try_from(info.width)
        .ok()
        .and_then(|width| {
            usize::try_from(info.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(())?;
    if info.width == 0
        || info.height == 0
        || info.width > MAX_QR_DIMENSION
        || info.height > MAX_QR_DIMENSION
        || pixel_count > MAX_QR_PIXELS
    {
        return Err(());
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).map_err(|_| ())?;
    pixels.truncate(frame.buffer_size());
    render_pixels(frame.width, frame.height, frame.color_type, &pixels)
}

fn render_pixels(
    width: u32,
    height: u32,
    color_type: png::ColorType,
    pixels: &[u8],
) -> Result<String, ()> {
    let samples = color_type.samples();
    let width = usize::try_from(width).map_err(|_| ())?;
    let height = usize::try_from(height).map_err(|_| ())?;
    if pixels.len()
        != width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(samples))
            .ok_or(())?
    {
        return Err(());
    }

    let mut rendered = String::with_capacity(width.saturating_mul(height).saturating_mul(2));
    for row in pixels.chunks_exact(width * samples) {
        for pixel in row.chunks_exact(samples) {
            rendered.push_str(if is_dark(pixel, color_type) {
                "██"
            } else {
                "  "
            });
        }
        rendered.push('\n');
    }
    Ok(rendered)
}

fn is_dark(pixel: &[u8], color_type: png::ColorType) -> bool {
    match color_type {
        png::ColorType::Grayscale => pixel[0] < 128,
        png::ColorType::GrayscaleAlpha => pixel[1] >= 128 && pixel[0] < 128,
        png::ColorType::Rgb => {
            u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]) < 384
        }
        png::ColorType::Rgba => {
            pixel[3] >= 128 && u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]) < 384
        }
        png::ColorType::Indexed => false,
    }
}
