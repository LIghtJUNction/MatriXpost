pub(crate) use std::time::Duration;

use url::Url;

pub(crate) const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";
pub(crate) const ELEMENT_POLL_ATTEMPTS: usize = 3;
pub(crate) const ELEMENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const ACKNOWLEDGEMENT_ATTEMPTS: usize = 60;
pub(crate) const ACKNOWLEDGEMENT_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const DEBUGGER_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
pub(crate) const VISIBLE_SCRIPT: &str = r#"const e=arguments[0];const s=getComputedStyle(e);const r=e.getBoundingClientRect();return !(e.getAttribute('aria-hidden')==='true'||s.display==='none'||s.visibility==='hidden'||Number(s.opacity)===0||r.width===0||r.height===0);"#;
pub(crate) const CODEMIRROR_WRITE_SCRIPT: &str = r#"const root=arguments[0],text=arguments[1];const editor=root.closest('.cm-editor')||root;const view=editor.cmView?.view||root.cmView?.view;if(view){view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});return view.state.doc.toString()===text;}root.focus();root.textContent=text;root.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:text}));return root.textContent===text;"#;
pub(crate) const MAX_ARTICLE_BODY_BYTES: usize = 1_000_000;
pub(crate) const MAX_ARTICLE_TITLE_BYTES: usize = 200;
pub(crate) const MAX_ARTICLE_CATEGORY_BYTES: usize = 64;
pub(crate) const MAX_ARTICLE_TAGS: usize = 10;
pub(crate) const MAX_ARTICLE_TAG_BYTES: usize = 32;
pub(crate) const MAX_ARTICLE_SUMMARY_BYTES: usize = 500;
pub(crate) const MAX_ARTICLE_COVER_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const ARTICLE_TEXT_EXTENSIONS: &[&str] = &["md", "txt"];
pub(crate) const ARTICLE_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
/// Remote videos are staged only into the user-selected directory before a
/// browser session is created.  Two GiB is large enough for ordinary platform
/// uploads while keeping the local runner's disk and network exposure bounded.
pub(crate) const MAX_REMOTE_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Accepted MIME types are deliberately finite: the runner stages videos, not
/// arbitrary remote objects. Parameters such as `charset` remain accepted by
/// the core policy's prefix comparison.
pub(crate) const REMOTE_VIDEO_CONTENT_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];
/// A remote source is untrusted input. Keep every execution failure at the
/// provider boundary intentionally opaque so neither its URL nor its staged
/// local path can be reflected to callers.
pub(crate) const REMOTE_MEDIA_EXECUTION_REJECTION: &str = "remote media publication failed";

pub(crate) fn normalize_review_title_query(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

pub(crate) fn sensitive(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
        "profile",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn local_webdriver_endpoint(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "WebDriver endpoint must be an absolute URL".to_owned())?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("WebDriver endpoint must be credential-free loopback HTTP".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if loopback && !sensitive(url.path()) {
        Ok(url)
    } else {
        Err("WebDriver endpoint must be credential-free loopback HTTP".into())
    }
}
