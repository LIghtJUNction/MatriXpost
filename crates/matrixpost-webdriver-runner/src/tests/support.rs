use crate::{
    profiles::{AcknowledgementPolicy, ELEMENT_KEY},
    service::{BrowserDebuggerProbe, RunnerService, TerminalQrAttempts, app},
    webdriver::{
        ArticlePublicationExecutor, LoginNavigationExecutor, PublicationExecutor,
        TerminalQrLoginAttempt, TerminalQrLoginExecutor, WebDriverPublisher, WebDriverTransport,
    },
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use matrixpost_core::{MediaSource, Platform, PublishArticleRequest, PublishRequest};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::{
    net::SocketAddr,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

pub(crate) struct ProfileFixture {
    pub(crate) platform: Platform,
    pub(crate) upload_url: &'static str,
    pub(crate) success_selector: &'static str,
}
pub(crate) const PROFILE_FIXTURES: &[ProfileFixture] = &[
    ProfileFixture {
        platform: Platform::Douyin,
        upload_url: "https://creator.douyin.com/creator-micro/content/post/video?enter_from=publish_page",
        success_selector: "[data-e2e='publish-success']",
    },
    ProfileFixture {
        platform: Platform::WechatChannels,
        upload_url: "https://channels.weixin.qq.com/platform/post/create",
        success_selector: "[data-status='published']",
    },
    ProfileFixture {
        platform: Platform::Bilibili,
        upload_url: "https://member.bilibili.com/platform/upload/video/frame/",
        success_selector: ".success-wrap",
    },
    ProfileFixture {
        platform: Platform::Baijiahao,
        upload_url: "https://baijiahao.baidu.com/builder/rc/edit?type=videoV2&is_from_cms=1",
        success_selector: "[data-status='published']",
    },
    ProfileFixture {
        platform: Platform::Toutiao,
        upload_url: "https://mp.toutiao.com/profile_v4/xigua/upload-video",
        success_selector: ".publish-success",
    },
    ProfileFixture {
        platform: Platform::Kuaishou,
        upload_url: "https://cp.kuaishou.com/article/publish/video?tabType=1",
        success_selector: ".publish-result-success",
    },
    ProfileFixture {
        platform: Platform::Xiaohongshu,
        upload_url: "https://creator.xiaohongshu.com/publish/publish?from=menu&target=video",
        success_selector: ".publish-success",
    },
    ProfileFixture {
        platform: Platform::FanqieVideo,
        upload_url: "https://pugc.yueduwuxian.com/fqvideo/home/publish-video",
        success_selector: ".publish-success",
    },
];

pub(crate) struct MockWebDriver {
    pub(crate) replies: Mutex<VecDeque<Result<Value, String>>>,
    pub(crate) methods: Mutex<Vec<String>>,
    pub(crate) paths: Mutex<Vec<String>>,
    pub(crate) bodies: Mutex<Vec<Value>>,
}
impl MockWebDriver {
    pub(crate) fn new(replies: Vec<Result<Value, String>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            methods: Mutex::new(Vec::new()),
            paths: Mutex::new(Vec::new()),
            bodies: Mutex::new(Vec::new()),
        }
    }
}
impl WebDriverTransport for MockWebDriver {
    fn request(&self, method: &str, path: &str, body: Value) -> Result<Value, String> {
        self.methods.lock().unwrap().push(method.into());
        self.paths.lock().unwrap().push(path.into());
        self.bodies.lock().unwrap().push(body);
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("unexpected HTTP request".into()))
    }
}
pub(crate) fn value(value: Value) -> Result<Value, String> {
    Ok(json!({"value":value}))
}
pub(crate) fn request() -> PublishRequest {
    PublishRequest {
        source: MediaSource::LocalFile("movie.mp4".into()),
        title: "Title".into(),
        short_title: None,
        tags: vec!["tag".into()],
        address: None,
        draft: false,
        bt2: None,
        scheduled_at: None,
        task_name: None,
        account: Default::default(),
        wechat_link: Default::default(),
        overrides: Vec::new(),
        targets: vec![Platform::Douyin],
    }
}
pub(crate) fn article_request() -> PublishArticleRequest {
    PublishArticleRequest {
        platform: "juejin".into(),
        account: Default::default(),
        title: "Article title".into(),
        content: Some("# Article body".into()),
        file: None,
        cover: None,
        category: None,
        tags: Vec::new(),
        summary: None,
        scheduled_at: None,
    }
}
pub(crate) fn temporary_article_path(extension: &str) -> std::path::PathBuf {
    static NEXT_TEST_FILE: AtomicU64 = AtomicU64::new(1);
    std::env::temp_dir().join(format!(
        "matrixpost-article-runner-{}-{}.{}",
        std::process::id(),
        NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed),
        extension
    ))
}
pub(crate) fn element(id: &str) -> Result<Value, String> {
    value(json!({ELEMENT_KEY:id}))
}
pub(crate) fn debugger_address() -> SocketAddr {
    "127.0.0.1:9222".parse().unwrap()
}
pub(crate) fn test_publisher(mock: MockWebDriver) -> WebDriverPublisher<MockWebDriver> {
    WebDriverPublisher {
        transport: mock,
        browser_debugger_address: debugger_address(),
        acknowledgement: AcknowledgementPolicy {
            attempts: 2,
            interval: Duration::ZERO,
        },
        next_job: AtomicU64::new(1),
    }
}

pub(crate) struct StaticBrowserDebuggerProbe(pub(crate) bool);

impl BrowserDebuggerProbe for StaticBrowserDebuggerProbe {
    fn is_ready(&self, _: SocketAddr) -> bool {
        self.0
    }
}

pub(crate) fn runner_service(
    executor: Option<Arc<dyn PublicationExecutor>>,
    article_executor: Option<Arc<dyn ArticlePublicationExecutor>>,
) -> RunnerService {
    RunnerService {
        executor,
        login_executor: None,
        terminal_qr_login_executor: None,
        terminal_qr_attempts: Arc::new(TerminalQrAttempts::new()),
        account_status_executor: None,
        review_status_executor: None,
        article_executor,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    }
}

pub(crate) fn runner_service_with_login(
    login_executor: Option<Arc<dyn LoginNavigationExecutor>>,
) -> RunnerService {
    RunnerService {
        executor: None,
        login_executor,
        terminal_qr_login_executor: None,
        terminal_qr_attempts: Arc::new(TerminalQrAttempts::new()),
        account_status_executor: None,
        review_status_executor: None,
        article_executor: None,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    }
}

pub(crate) struct TestTerminalQrExecutor {
    pub(crate) closes: Arc<AtomicU64>,
    pub(crate) capture_fails: bool,
}

pub(crate) struct BlockingTerminalQrExecutor {
    pub(crate) starts: AtomicU64,
    pub(crate) closes: Arc<AtomicU64>,
    barrier: Barrier,
}

impl BlockingTerminalQrExecutor {
    pub(crate) fn new(expected_starts: usize) -> Self {
        Self {
            starts: AtomicU64::new(0),
            closes: Arc::new(AtomicU64::new(0)),
            barrier: Barrier::new(expected_starts),
        }
    }
}

impl TestTerminalQrExecutor {
    pub(crate) fn available() -> Self {
        Self {
            closes: Arc::new(AtomicU64::new(0)),
            capture_fails: false,
        }
    }
}

struct TestTerminalQrAttempt {
    platform: Platform,
    closes: Arc<AtomicU64>,
    capture_fails: bool,
}

impl TerminalQrLoginAttempt for TestTerminalQrAttempt {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn capture_qr_png_base64(&mut self) -> Result<String, String> {
        if self.capture_fails {
            Err("capture failed".into())
        } else {
            Ok("iVBORw0KGgo=".into())
        }
    }

    fn close(&mut self) -> Result<(), String> {
        self.closes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl TerminalQrLoginExecutor for TestTerminalQrExecutor {
    fn start_terminal_qr_login(
        self: Arc<Self>,
        platform: Platform,
    ) -> Result<Box<dyn TerminalQrLoginAttempt>, String> {
        Ok(Box::new(TestTerminalQrAttempt {
            platform,
            closes: Arc::clone(&self.closes),
            capture_fails: self.capture_fails,
        }))
    }
}

impl TerminalQrLoginExecutor for BlockingTerminalQrExecutor {
    fn start_terminal_qr_login(
        self: Arc<Self>,
        platform: Platform,
    ) -> Result<Box<dyn TerminalQrLoginAttempt>, String> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        self.barrier.wait();
        Ok(Box::new(TestTerminalQrAttempt {
            platform,
            closes: Arc::clone(&self.closes),
            capture_fails: false,
        }))
    }
}

pub(crate) fn runner_service_with_terminal_qr(
    terminal_qr_login_executor: Option<Arc<dyn TerminalQrLoginExecutor>>,
) -> RunnerService {
    RunnerService {
        executor: None,
        login_executor: None,
        terminal_qr_login_executor,
        terminal_qr_attempts: Arc::new(TerminalQrAttempts::new()),
        account_status_executor: None,
        review_status_executor: None,
        article_executor: None,
        remote_media: None,
        browser_debugger_address: None,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(false)),
    }
}

pub(crate) fn runner_service_with_terminal_qr_attempts(
    terminal_qr_login_executor: Option<Arc<dyn TerminalQrLoginExecutor>>,
    terminal_qr_attempts: Arc<TerminalQrAttempts>,
) -> RunnerService {
    let mut service = runner_service_with_terminal_qr(terminal_qr_login_executor);
    service.terminal_qr_attempts = terminal_qr_attempts;
    service
}

pub(crate) fn runner_service_with_probe(
    executor: Option<Arc<dyn PublicationExecutor>>,
    browser_debugger_address: Option<SocketAddr>,
    probe_ready: bool,
) -> RunnerService {
    RunnerService {
        executor,
        login_executor: None,
        terminal_qr_login_executor: None,
        terminal_qr_attempts: Arc::new(TerminalQrAttempts::new()),
        account_status_executor: None,
        review_status_executor: None,
        article_executor: None,
        remote_media: None,
        browser_debugger_address,
        debugger_probe: Arc::new(StaticBrowserDebuggerProbe(probe_ready)),
    }
}

pub(crate) async fn health_status(service: RunnerService) -> Value {
    let response = app(Arc::new(service))
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
