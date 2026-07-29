use std::{
    net::SocketAddr,
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, atomic::AtomicU64},
};

use clap::Parser;
use url::Url;

use crate::{
    profiles::{AcknowledgementPolicy, local_webdriver_endpoint},
    service::*,
    webdriver::*,
};

#[derive(Parser)]
#[command(
    name = "matrixpost-webdriver-runner",
    version,
    about = "Loopback-only MatriXpost WebDriver runner"
)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:39001")]
    bind: SocketAddr,
    #[arg(long)]
    webdriver_endpoint: Option<String>,
    /// Loopback Chrome remote-debugging address to attach through ChromeDriver.
    #[arg(long)]
    browser_debugger_address: Option<SocketAddr>,
    /// Permit the irreversible article publish confirmation endpoint.
    #[arg(long)]
    allow_article_publish: bool,
    /// Permit navigating an already-attached browser to a platform page for a
    /// user to complete login manually. This never reads or exports login data.
    #[arg(long)]
    allow_login_navigation: bool,
    /// Permit temporary attached-browser navigation solely to infer upload-form
    /// readiness. No browser data is read or exported.
    #[arg(long)]
    allow_account_status_probe: bool,
    /// Permit bounded Fanqie video-list review-status probes in an already
    /// attached browser. The response never contains page data or identifiers.
    #[arg(long)]
    allow_review_status_probe: bool,
    /// Absolute directory used only for bounded HTTP(S) video staging before
    /// WebDriver upload. Remote media is rejected unless this is configured.
    #[arg(long, value_name = "ABSOLUTE_PATH")]
    remote_media_dir: Option<PathBuf>,
}

pub(crate) fn build_remote_media_support(
    directory: Option<PathBuf>,
) -> Result<Option<RemoteMediaSupport>, String> {
    match directory {
        None => Ok(None),
        Some(directory) if directory.is_absolute() => {
            Ok(Some(RemoteMediaSupport::configured(directory)))
        }
        Some(_) => Err("--remote-media-dir must be an absolute path".into()),
    }
}

pub(crate) fn build_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
) -> Result<Option<Arc<dyn PublicationExecutor>>, String> {
    Ok(build_attached_publisher(endpoint, debugger_address)?
        .map(|publisher| publisher as Arc<dyn PublicationExecutor>))
}

fn build_attached_publisher(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
) -> Result<Option<Arc<WebDriverPublisher<HttpWebDriver>>>, String> {
    match (endpoint, debugger_address) {
        (Some(endpoint), Some(address)) if address.ip().is_loopback() => {
            Ok(Some(Arc::new(WebDriverPublisher {
                transport: HttpWebDriver { endpoint },
                browser_debugger_address: address,
                acknowledgement: AcknowledgementPolicy::production(),
                next_job: AtomicU64::new(1),
            })))
        }
        (Some(_), Some(_)) => Err("browser debugger address must be loopback".into()),
        (None, Some(_)) => {
            Err("--webdriver-endpoint is required when --browser-debugger-address is set".into())
        }
        (_, None) => Ok(None),
    }
}

pub(crate) fn build_article_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_article_publish: bool,
) -> Result<Option<Arc<dyn ArticlePublicationExecutor>>, String> {
    Ok(
        build_opt_in_publisher(endpoint, debugger_address, allow_article_publish)?
            .map(|publisher| publisher as Arc<dyn ArticlePublicationExecutor>),
    )
}

pub(crate) fn build_login_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_login_navigation: bool,
) -> Result<Option<Arc<dyn LoginNavigationExecutor>>, String> {
    Ok(
        build_opt_in_publisher(endpoint, debugger_address, allow_login_navigation)?
            .map(|publisher| publisher as Arc<dyn LoginNavigationExecutor>),
    )
}

pub(crate) fn build_account_status_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_account_status_probe: bool,
) -> Result<Option<Arc<dyn AccountStatusExecutor>>, String> {
    Ok(
        build_opt_in_publisher(endpoint, debugger_address, allow_account_status_probe)?
            .map(|publisher| publisher as Arc<dyn AccountStatusExecutor>),
    )
}

pub(crate) fn build_review_status_executor(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    allow_review_status_probe: bool,
) -> Result<Option<Arc<dyn ReviewStatusExecutor>>, String> {
    Ok(
        build_opt_in_publisher(endpoint, debugger_address, allow_review_status_probe)?
            .map(|publisher| publisher as Arc<dyn ReviewStatusExecutor>),
    )
}

fn build_opt_in_publisher(
    endpoint: Option<Url>,
    debugger_address: Option<SocketAddr>,
    enabled: bool,
) -> Result<Option<Arc<WebDriverPublisher<HttpWebDriver>>>, String> {
    if enabled {
        build_attached_publisher(endpoint, debugger_address)
    } else {
        Ok(None)
    }
}

pub(crate) async fn run() -> ExitCode {
    let args = Args::parse();
    if !args.bind.ip().is_loopback() {
        eprintln!("matrixpost-webdriver-runner bind must be loopback");
        return ExitCode::from(2);
    }
    let endpoint = match args.webdriver_endpoint {
        Some(endpoint) => match local_webdriver_endpoint(&endpoint) {
            Ok(endpoint) => Some(endpoint),
            Err(error) => {
                eprintln!("matrixpost-webdriver-runner configuration error: {error}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let executor = match build_executor(endpoint.clone(), args.browser_debugger_address) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let article_executor = match build_article_executor(
        endpoint.clone(),
        args.browser_debugger_address,
        args.allow_article_publish,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let login_executor = match build_login_executor(
        endpoint.clone(),
        args.browser_debugger_address,
        args.allow_login_navigation,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let account_status_executor = match build_account_status_executor(
        endpoint.clone(),
        args.browser_debugger_address,
        args.allow_account_status_probe,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let review_status_executor = match build_review_status_executor(
        endpoint,
        args.browser_debugger_address,
        args.allow_review_status_probe,
    ) {
        Ok(executor) => executor,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let remote_media = match build_remote_media_support(args.remote_media_dir) {
        Ok(support) => support,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let service = Arc::new(RunnerService {
        executor,
        login_executor,
        account_status_executor,
        review_status_executor,
        article_executor,
        remote_media,
        browser_debugger_address: args.browser_debugger_address,
        debugger_probe: Arc::new(HttpBrowserDebuggerProbe),
    });
    let listener = match tokio::net::TcpListener::bind(args.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner failed to bind: {error}");
            return ExitCode::from(4);
        }
    };
    match axum::serve(listener, app(service)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("matrixpost-webdriver-runner stopped unexpectedly: {error}");
            ExitCode::from(4)
        }
    }
}
