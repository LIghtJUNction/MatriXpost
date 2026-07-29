//! Minimal terminal QR capture over the W3C WebDriver element-screenshot API.
//!
//! This module deliberately has no cookie, storage, profile, or full-page
//! screenshot operations. A temporary session is kept solely while a runner
//! attempt is active and is closed by the service on every terminal path.

use std::sync::Arc;

use matrixpost_core::{Platform, TERMINAL_QR_LOGIN_PNG_BASE64_MAX_BYTES};
use serde_json::{Value, json};

use super::{ELEMENT_KEY, WebDriverPublisher, WebDriverTransport};

const DOUYIN_LOGIN_URL: &str =
    "https://creator.douyin.com/creator-micro/content/post/video?enter_from=publish_page";
const WECHAT_LOGIN_URL: &str = "https://channels.weixin.qq.com/platform/post/create";
const DOUYIN_QR_SELECTOR: &str = "#animate_qrcode_container";
const WECHAT_LOGIN_FRAME_SELECTOR: &str = "iframe[src*='login-for-iframe']";
const WECHAT_QR_SELECTOR: &str = "img.qrcode";

/// Creates a bounded, runner-owned terminal QR attempt in an existing
/// attached browser. It never exposes a session ID or inspects login state.
pub(crate) trait TerminalQrLoginExecutor: Send + Sync {
    fn start_terminal_qr_login(
        self: Arc<Self>,
        platform: Platform,
    ) -> Result<Box<dyn TerminalQrLoginAttempt>, String>;
}

/// Runner-private state for one terminal QR attempt.
pub(crate) trait TerminalQrLoginAttempt: Send {
    fn platform(&self) -> Platform;
    fn capture_qr_png_base64(&mut self) -> Result<String, String>;
    fn close(&mut self) -> Result<(), String>;
}

pub(crate) struct WebDriverTerminalQrAttempt<T> {
    publisher: Arc<WebDriverPublisher<T>>,
    session: Option<String>,
    platform: Platform,
}

impl<T: WebDriverTransport> WebDriverTerminalQrAttempt<T> {
    fn session(&self) -> Result<&str, String> {
        self.session
            .as_deref()
            .ok_or_else(|| "terminal QR attempt is already closed".to_owned())
    }

    fn find_unique(&self, selector: &str) -> Result<String, String> {
        let session = self.session()?;
        let reply = self.publisher.transport.request(
            "POST",
            &format!("/session/{session}/elements"),
            json!({"using":"css selector","value":selector}),
        )?;
        let values = WebDriverPublisher::<T>::webdriver_value(reply)?;
        let elements = values
            .as_array()
            .ok_or_else(|| "WebDriver selector response was not an element array".to_owned())?;
        if elements.len() != 1 {
            return Err("terminal QR selector did not identify exactly one element".into());
        }
        elements[0]
            .get(ELEMENT_KEY)
            .and_then(Value::as_str)
            .or_else(|| elements[0].get("ELEMENT").and_then(Value::as_str))
            .map(str::to_owned)
            .ok_or_else(|| "terminal QR selector returned an invalid element".into())
    }

    fn switch_to_wechat_login_frame(&self) -> Result<(), String> {
        let session = self.session()?;
        WebDriverPublisher::<T>::webdriver_value(self.publisher.transport.request(
            "POST",
            &format!("/session/{session}/frame"),
            json!({"id": null}),
        )?)?;
        let frame = self.find_unique(WECHAT_LOGIN_FRAME_SELECTOR)?;
        WebDriverPublisher::<T>::webdriver_value(self.publisher.transport.request(
            "POST",
            &format!("/session/{session}/frame"),
            json!({"id": {ELEMENT_KEY: frame}}),
        )?)
        .map(|_| ())
    }

    fn screenshot_element(&self, element: &str) -> Result<String, String> {
        let session = self.session()?;
        let value = WebDriverPublisher::<T>::webdriver_value(self.publisher.transport.request(
            "GET",
            &format!("/session/{session}/element/{element}/screenshot"),
            json!({}),
        )?)?;
        let png_base64 = value
            .as_str()
            .ok_or_else(|| "WebDriver element screenshot was not base64 text".to_owned())?;
        validate_png_base64(png_base64)?;
        Ok(png_base64.to_owned())
    }
}

impl<T: WebDriverTransport + 'static> TerminalQrLoginExecutor for WebDriverPublisher<T> {
    fn start_terminal_qr_login(
        self: Arc<Self>,
        platform: Platform,
    ) -> Result<Box<dyn TerminalQrLoginAttempt>, String> {
        let login_url = match platform {
            Platform::Douyin => DOUYIN_LOGIN_URL,
            Platform::WechatChannels => WECHAT_LOGIN_URL,
            _ => {
                return Err(
                    "terminal QR login is supported only for Douyin and WeChat Channels".into(),
                );
            }
        };
        let session = self.session()?;
        let outcome = self.navigate(&session, login_url);
        if outcome.is_err() {
            let _ = self.delete_session(&session);
            return outcome.map(|_| unreachable!());
        }
        let mut attempt = WebDriverTerminalQrAttempt {
            publisher: self,
            session: Some(session),
            platform,
        };
        if attempt.capture_qr_png_base64().is_err() {
            let _ = attempt.close();
            return Err("terminal QR element could not be captured".into());
        }
        Ok(Box::new(attempt))
    }
}

impl<T: WebDriverTransport> TerminalQrLoginAttempt for WebDriverTerminalQrAttempt<T> {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn capture_qr_png_base64(&mut self) -> Result<String, String> {
        let selector = match self.platform {
            Platform::Douyin => DOUYIN_QR_SELECTOR,
            Platform::WechatChannels => {
                self.switch_to_wechat_login_frame()?;
                WECHAT_QR_SELECTOR
            }
            _ => return Err("unsupported terminal QR platform".into()),
        };
        let element = self.find_unique(selector)?;
        self.screenshot_element(&element)
    }

    fn close(&mut self) -> Result<(), String> {
        let Some(session) = self.session.take() else {
            return Ok(());
        };
        self.publisher.delete_session(&session)
    }
}

fn validate_png_base64(value: &str) -> Result<(), String> {
    // The caller must never echo arbitrary WebDriver response data. This small
    // structural check is enough to reject non-PNG or oversized screenshots
    // without decoding or persisting the image.
    const PNG_PREFIX_BASE64: &str = "iVBORw0KGgo=";
    if value.len() < PNG_PREFIX_BASE64.len()
        || value.len() > TERMINAL_QR_LOGIN_PNG_BASE64_MAX_BYTES
        || !value.len().is_multiple_of(4)
        || !value.starts_with(PNG_PREFIX_BASE64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
    {
        return Err("WebDriver element screenshot was not a bounded PNG".into());
    }
    Ok(())
}
