//! WebDriver protocol facade and platform-specialist publication flows.

use crate::profiles::*;

mod article;
mod baijiahao;
mod bilibili;
mod douyin;
mod fanqie;
mod interaction;
mod kuaishou;
mod model;
mod primitives;
mod status;
mod terminal_qr;
mod toutiao;
mod transport;
mod video;
mod video_text;
mod wechat;
mod xiaohongshu;

pub(crate) use model::{
    AccountStatusExecutor, ArticleExecutionError, ArticlePublicationExecutor,
    LoginNavigationExecutor, PublicationExecutor, ReviewStatusExecutor, WebDriverPublisher,
    WebDriverTransport,
};
pub(crate) use terminal_qr::{TerminalQrLoginAttempt, TerminalQrLoginExecutor};
pub(crate) use transport::HttpWebDriver;
