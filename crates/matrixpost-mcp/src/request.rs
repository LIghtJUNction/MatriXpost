use std::{collections::BTreeMap, path::PathBuf};

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use matrixpost_core::{
    AccountSelection, ArticleDispatchOutcome, DispatchOutcome, LocalSchedule, MediaSource,
    Platform, PlatformOverride, ProviderDispatchReport, PublishArticleRequest, PublishRequest,
    ScheduledJob, WechatLink,
};

use crate::{
    PROVIDER_MESSAGE,
    model::{
        ArticlePlatformInput, JobResult, PublicationResult, SafeProviderOutcome, SphLinkInput,
        VideoPlatform,
    },
};

pub(crate) fn video_request(input: crate::PublishVideoInput) -> Result<PublishRequest, String> {
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let platform = video_platform(input.platform);
    let _ = input.show;
    let source = match url::Url::parse(&input.file) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
        Ok(url) => {
            return Err(format!(
                "unsupported remote source scheme: {}",
                url.scheme()
            ));
        }
        Err(_) => MediaSource::LocalFile(PathBuf::from(&input.file)),
    };
    let scheduled_at = input
        .publish_at
        .as_deref()
        .map(parse_video_schedule)
        .transpose()?;
    let wechat_link = if platform == Platform::WechatChannels {
        effective_sph_link(input.sph_product_id, input.sph_link)?
    } else {
        WechatLink::default()
    };
    let overrides = input.creative_statement.map(|creative_statement| {
        vec![PlatformOverride {
            platform,
            title: None,
            short_title: None,
            tags: None,
            creative_statement: Some(creative_statement),
            account: None,
            wechat_link: None,
        }]
    });
    let request = PublishRequest {
        source,
        title: input.title,
        short_title: None,
        tags: split_tags(input.tags),
        address: input.address,
        draft: input.draft.unwrap_or(false),
        bt2: input.bt2,
        scheduled_at,
        task_name: None,
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        wechat_link,
        overrides: overrides.unwrap_or_default(),
        targets: vec![platform],
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

pub(crate) fn article_request(
    input: crate::PublishArticleInput,
) -> Result<PublishArticleRequest, String> {
    let ArticlePlatformInput::Juejin = input.platform;
    if input.phone.trim().is_empty() {
        return Err("phone must not be empty".into());
    }
    let _ = input.show;
    let request = PublishArticleRequest {
        platform: "juejin".into(),
        account: AccountSelection {
            phone: Some(input.phone),
            partition: None,
        },
        title: input.title,
        content: input.content,
        file: input.file.map(PathBuf::from),
        cover: input.cover,
        category: input.category,
        tags: split_tags(input.tags),
        summary: input.summary,
        scheduled_at: input
            .publish_at
            .as_deref()
            .map(|value| parse_article_schedule(value, Local::now().date_naive()))
            .transpose()?,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

pub(crate) fn video_platform(value: VideoPlatform) -> Platform {
    match value {
        VideoPlatform::Dy => Platform::Douyin,
        VideoPlatform::Ks => Platform::Kuaishou,
        VideoPlatform::Blbl => Platform::Bilibili,
        VideoPlatform::Bjh => Platform::Baijiahao,
        VideoPlatform::Tt => Platform::Toutiao,
        VideoPlatform::Sph => Platform::WechatChannels,
    }
}

fn parse_video_schedule(value: &str) -> Result<LocalSchedule, String> {
    normalize_full_schedule(
        value,
        "publishAt must use YYYY-MM-DD HH:mm or YYYY-MM-DD HH:mm:ss",
    )
}

pub(crate) fn parse_article_schedule(
    value: &str,
    today: NaiveDate,
) -> Result<LocalSchedule, String> {
    if let Ok(time) = NaiveTime::parse_from_str(value, "%H:%M") {
        return LocalSchedule::parse(&format!("{} {}:00", today, time.format("%H:%M")))
            .map_err(|error| error.to_string());
    }
    normalize_full_schedule(
        value,
        "publishAt must use HH:mm, YYYY-MM-DD HH:mm, or YYYY-MM-DD HH:mm:ss",
    )
}

fn normalize_full_schedule(value: &str, message: &str) -> Result<LocalSchedule, String> {
    if let Ok(schedule) = LocalSchedule::parse(value) {
        return Ok(schedule);
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")
        .map(|time| LocalSchedule(time.format("%Y-%m-%d %H:%M:%S").to_string()))
        .map_err(|_| message.to_owned())
}

pub(crate) fn effective_sph_link(
    product_id: Option<String>,
    link: Option<SphLinkInput>,
) -> Result<WechatLink, String> {
    if let Some(product_id) = product_id {
        if product_id.trim().is_empty() {
            return Err("sphProductId must not be empty".into());
        }
        return Ok(WechatLink {
            link_type: Some("product".into()),
            link_value: Some(product_id.clone()),
            product_id: Some(product_id),
        });
    }
    match link {
        None => Ok(WechatLink::default()),
        Some(SphLinkInput::None {}) => Ok(WechatLink {
            product_id: None,
            link_type: Some("none".into()),
            link_value: None,
        }),
        Some(SphLinkInput::Product { value }) if !value.trim().is_empty() => Ok(WechatLink {
            product_id: None,
            link_type: Some("product".into()),
            link_value: Some(value),
        }),
        Some(SphLinkInput::Product { .. }) => {
            Err("sphLink.value must not be empty when sphLink.type is product".into())
        }
    }
}

fn split_tags(tags: Option<String>) -> Vec<String> {
    tags.unwrap_or_default()
        .split([' ', ','])
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn job_result(job: ScheduledJob) -> JobResult {
    JobResult {
        id: job.id,
        state: job.state,
        due_at: job.due_at,
        revision: job.revision,
    }
}

pub(crate) fn article_unavailable_result() -> PublicationResult {
    PublicationResult {
        outcome: "unavailable",
        provider_available: false,
        remote_publish_attempted: false,
        persisted: false,
        job: None,
        providers: None,
        message: "no article runner is configured; no remote publishing was attempted",
    }
}

pub(crate) fn article_dispatch_result(outcome: ArticleDispatchOutcome) -> PublicationResult {
    match outcome {
        ArticleDispatchOutcome::Queued { .. } => PublicationResult {
            outcome: "queued",
            provider_available: true,
            remote_publish_attempted: true,
            persisted: false,
            job: None,
            providers: None,
            message: "local article runner completed its WebDriver workflow; remote publication is not confirmed",
        },
        ArticleDispatchOutcome::Unavailable { .. } => PublicationResult {
            outcome: "unavailable",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: false,
            job: None,
            providers: None,
            message: "article runner was unavailable; no remote publishing was attempted",
        },
        ArticleDispatchOutcome::Rejected {
            automation_attempted,
            ..
        } => PublicationResult {
            outcome: "rejected",
            provider_available: false,
            remote_publish_attempted: automation_attempted,
            persisted: false,
            job: None,
            providers: None,
            message: "article runner rejected the request; no remote publication success is claimed",
        },
    }
}

pub(crate) fn video_dispatch_result(report: ProviderDispatchReport) -> PublicationResult {
    let providers = report
        .outcomes
        .iter()
        .map(|(platform, outcome)| (*platform, safe_provider_outcome(outcome)))
        .collect::<BTreeMap<_, _>>();
    let all_queued = report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. }));
    let all_unavailable = report
        .outcomes
        .values()
        .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. }));
    let remote_publish_attempted = report.outcomes.values().any(|outcome| {
        matches!(
            outcome,
            DispatchOutcome::Queued { .. } | DispatchOutcome::Rejected { .. }
        )
    });
    if all_queued {
        return PublicationResult {
            outcome: "queued",
            provider_available: true,
            remote_publish_attempted: true,
            persisted: false,
            job: None,
            providers: Some(providers),
            message: "local provider runner completed its WebDriver workflow; remote publication is not confirmed",
        };
    }
    if all_unavailable {
        return PublicationResult {
            outcome: "unavailable",
            provider_available: false,
            remote_publish_attempted: false,
            persisted: false,
            job: None,
            providers: Some(providers),
            message: PROVIDER_MESSAGE,
        };
    }
    PublicationResult {
        outcome: "rejected",
        provider_available: false,
        remote_publish_attempted,
        persisted: false,
        job: None,
        providers: Some(providers),
        message: "local provider runner dispatch was incomplete; no remote publication success is claimed",
    }
}

fn safe_provider_outcome(outcome: &DispatchOutcome) -> SafeProviderOutcome {
    match outcome {
        DispatchOutcome::Queued { .. } => SafeProviderOutcome::Queued,
        DispatchOutcome::Unavailable { .. } => SafeProviderOutcome::Unavailable,
        DispatchOutcome::Rejected { .. } => SafeProviderOutcome::Rejected,
    }
}
