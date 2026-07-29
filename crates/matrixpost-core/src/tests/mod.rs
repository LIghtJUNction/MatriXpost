use crate::*;
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{Connection, params};
use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    fs::OpenOptions,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread,
};
use url::Url;

static STAGING_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestTransport(Mutex<Option<RemoteMediaResponse>>);

impl TestTransport {
    fn response(
        content_type: Option<&str>,
        content_length: Option<&str>,
        body: impl Read + 'static,
    ) -> Self {
        Self(Mutex::new(Some(RemoteMediaResponse {
            content_type: content_type.map(str::to_owned),
            content_length: content_length.map(str::to_owned),
            body: Box::new(body),
        })))
    }
}

impl RemoteMediaTransport for TestTransport {
    fn get(&self, _: &Url) -> Result<RemoteMediaResponse, DomainError> {
        self.0
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| DomainError::RemoteMedia("test response reused".into()))
    }
}

#[derive(Clone, Copy)]
enum TestOutput {
    File,
    FailWrite,
    FailFlush,
}

struct TestFilesystem {
    output: TestOutput,
    created: AtomicU64,
}

impl TestFilesystem {
    fn file() -> Self {
        Self {
            output: TestOutput::File,
            created: AtomicU64::new(0),
        }
    }

    fn failing(output: TestOutput) -> Self {
        Self {
            output,
            created: AtomicU64::new(0),
        }
    }

    fn created_count(&self) -> u64 {
        self.created.load(Ordering::Relaxed)
    }
}

impl StagingFilesystem for TestFilesystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn create_new(&self, path: &Path) -> io::Result<Box<dyn Write>> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        self.created.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(TestWriter {
            file,
            failure: self.output,
        }))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

struct TestWriter {
    file: fs::File,
    failure: TestOutput,
}

impl Write for TestWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if matches!(self.failure, TestOutput::FailWrite) {
            return Err(io::Error::other("injected write failure"));
        }
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        if matches!(self.failure, TestOutput::FailFlush) {
            return Err(io::Error::other("injected flush failure"));
        }
        self.file.flush()
    }
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected read failure"))
    }
}

struct TestNames(VecDeque<String>);

impl TestNames {
    fn one(name: &str) -> Self {
        Self(VecDeque::from([name.to_owned()]))
    }
}

impl StagingNameSource for TestNames {
    fn next_name(&mut self) -> String {
        self.0.pop_front().expect("test supplied enough names")
    }
}

fn staging_directory(label: &str) -> PathBuf {
    let sequence = STAGING_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "matrixpost-core-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn staging_policy(max_bytes: u64) -> MediaStagingPolicy {
    MediaStagingPolicy {
        max_bytes,
        allowed_content_types: vec!["video/".into()],
    }
}

fn staging_request(policy: &MediaStagingPolicy) -> RemoteMediaRequest {
    RemoteMediaRequest::new(
        Url::parse("https://example.invalid/movie.mp4").unwrap(),
        policy,
    )
    .unwrap()
}

fn assert_empty_directory(path: &Path) {
    assert!(path.exists());
    assert_eq!(fs::read_dir(path).unwrap().count(), 0);
}

fn assert_staging_error_leaves_no_file(
    label: &str,
    policy: MediaStagingPolicy,
    transport: TestTransport,
    filesystem: &TestFilesystem,
) {
    let directory = staging_directory(label);
    fs::create_dir_all(&directory).unwrap();
    let stager = HttpRemoteMediaStager::new(directory.clone());
    let mut names = TestNames::one("output");
    assert!(
        stager
            .stage_with(
                &staging_request(&policy),
                &policy,
                &transport,
                filesystem,
                &mut names,
            )
            .is_err()
    );
    assert_empty_directory(&directory);
    fs::remove_dir_all(directory).unwrap();
}
fn request() -> PublishRequest {
    PublishRequest {
        source: MediaSource::LocalFile("video.mp4".into()),
        title: "title".into(),
        short_title: Some("short".into()),
        tags: vec!["tag".into()],
        address: Some("address".into()),
        draft: false,
        bt2: None,
        scheduled_at: Some(LocalSchedule::parse("2026-01-02 03:04:05").unwrap()),
        task_name: Some("task".into()),
        account: AccountSelection {
            phone: Some("masked".into()),
            partition: Some("main".into()),
        },
        wechat_link: WechatLink {
            product_id: Some("product".into()),
            ..Default::default()
        },
        overrides: vec![PlatformOverride {
            platform: Platform::Douyin,
            title: None,
            short_title: None,
            tags: None,
            creative_statement: Some("original".into()),
            account: None,
            wechat_link: None,
        }],
        targets: vec![Platform::Douyin],
    }
}

struct TestProvider {
    platform: Platform,
    availability: ProviderAvailability,
    outcome: DispatchOutcome,
    error: Option<String>,
    calls: Arc<AtomicUsize>,
}

struct CapturingRunnerTransport(Mutex<Option<(String, String)>>);

impl RunnerHttpTransport for CapturingRunnerTransport {
    fn post_json(&self, endpoint: &str, body: &str) -> Result<(u16, String), ()> {
        *self.0.lock().unwrap() = Some((endpoint.into(), body.into()));
        Ok((
            200,
            r#"{"outcome":"queued","version":1,"platform":"dy","job_id":"safe-job"}"#.into(),
        ))
    }
}

struct CapturingManualLoginTransport {
    captured: Mutex<Option<(String, String)>>,
    response: Result<(u16, String), ManualLoginTransportError>,
}

impl ManualLoginHttpTransport for CapturingManualLoginTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        *self.captured.lock().unwrap() = Some((endpoint.into(), body.into()));
        self.response.clone()
    }
}

impl AccountStatusHttpTransport for CapturingManualLoginTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        ManualLoginHttpTransport::post_json(self, endpoint, body)
    }
}

impl ReviewStatusHttpTransport for CapturingManualLoginTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ManualLoginTransportError> {
        ManualLoginHttpTransport::post_json(self, endpoint, body)
    }
}

struct CapturingArticleRunnerTransport {
    captured: Mutex<Option<(String, String)>>,
    response: (u16, String),
}

struct FailingArticleRunnerTransport;

impl ArticleRunnerHttpTransport for FailingArticleRunnerTransport {
    fn post_json(&self, _: &str, _: &str) -> Result<(u16, String), ArticleRunnerTransportError> {
        Err(ArticleRunnerTransportError::RequestFailed)
    }
}

impl ArticleRunnerHttpTransport for CapturingArticleRunnerTransport {
    fn post_json(
        &self,
        endpoint: &str,
        body: &str,
    ) -> Result<(u16, String), ArticleRunnerTransportError> {
        *self.captured.lock().unwrap() = Some((endpoint.into(), body.into()));
        Ok(self.response.clone())
    }
}

impl PublishProvider for TestProvider {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn availability(&self) -> ProviderAvailability {
        self.availability.clone()
    }

    fn enqueue(&self, _: &PublishRequest) -> Result<DispatchOutcome, DomainError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.error
            .as_ref()
            .map(|reason| Err(DomainError::RemoteMedia(reason.clone())))
            .unwrap_or_else(|| Ok(self.outcome.clone()))
    }
}

fn test_provider(
    platform: Platform,
    availability: ProviderAvailability,
    outcome: DispatchOutcome,
    error: Option<&str>,
) -> (Box<dyn PublishProvider>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Box::new(TestProvider {
            platform,
            availability,
            outcome,
            error: error.map(str::to_owned),
            calls: Arc::clone(&calls),
        }),
        calls,
    )
}

include!("publication.rs");
include!("lifecycle.rs");
include!("runner.rs");
