use std::{collections::BTreeMap, str::FromStr};

use chrono::{DateTime, Local, NaiveDate, Utc};
use matrixpost_core::{
    AccountSelection, ApprovalStatus, BusinessObjectStatus, HistoryFilter, HistoryRecord,
    LedgerDirection, MediaSource, Platform, PlatformOverride, PublishRequest, WechatLink,
};

use crate::args::{HistoryArgs, PublishArgs};

/// A CLI-only refinement around the core's durable history filter.
#[derive(Debug)]
pub(crate) struct HistoryQuery {
    filter: HistoryFilter,
    phone: Option<String>,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    limit: usize,
}

pub(crate) fn parse_video_platform(value: &str) -> Result<Platform, String> {
    Platform::from_str(value).map_err(|error| error.to_string())
}

pub(crate) fn parse_history_date(value: &str) -> Result<NaiveDate, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| !matches!(index, 4 | 7))
            .all(|(_, byte)| byte.is_ascii_digit())
    {
        return Err("date must use YYYY-MM-DD format".into());
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "date must use YYYY-MM-DD format".to_owned())
}

pub(crate) fn parse_history_platform(value: &str) -> Result<Platform, String> {
    let platform = Platform::from_str(value).map_err(|error| error.to_string())?;
    if platform == Platform::FanqieVideo {
        return Err("history platform must be dy, ks, blbl, bjh, tt, sph, or xhs".into());
    }
    Ok(platform)
}

pub(crate) fn parse_business_object_status(value: &str) -> Result<BusinessObjectStatus, String> {
    match value {
        "draft" => Ok(BusinessObjectStatus::Draft),
        "active" => Ok(BusinessObjectStatus::Active),
        "completed" => Ok(BusinessObjectStatus::Completed),
        "archived" => Ok(BusinessObjectStatus::Archived),
        _ => Err("lifecycle status must be draft, active, completed, or archived".into()),
    }
}

pub(crate) fn parse_approval_status(value: &str) -> Result<ApprovalStatus, String> {
    match value {
        "pending" => Ok(ApprovalStatus::Pending),
        "approved" => Ok(ApprovalStatus::Approved),
        "rejected" => Ok(ApprovalStatus::Rejected),
        _ => Err("approval status must be pending, approved, or rejected".into()),
    }
}

pub(crate) fn parse_ledger_direction(value: &str) -> Result<LedgerDirection, String> {
    match value {
        "expense" => Ok(LedgerDirection::Expense),
        "revenue" => Ok(LedgerDirection::Revenue),
        _ => Err("ledger direction must be expense or revenue".into()),
    }
}

pub(crate) fn parse_positive_minor_amount(value: &str) -> Result<i64, String> {
    let amount = value
        .parse::<i64>()
        .map_err(|_| "amount minor must be a positive integer".to_owned())?;
    if amount <= 0 {
        return Err("amount minor must be a positive integer".into());
    }
    Ok(amount)
}

pub(crate) fn parse_currency(value: &str) -> Result<String, String> {
    if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(value.to_owned())
    } else {
        Err("currency must be a three-letter uppercase ISO code".into())
    }
}

pub(crate) fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| "timestamp must use RFC3339 format".into())
}

pub(crate) fn parse_attributes(values: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut attributes = BTreeMap::new();
    for value in values {
        let Some((key, value)) = value.split_once('=') else {
            return Err("attribute must use KEY=VALUE".into());
        };
        if key.trim().is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(format!("attribute key is invalid: {key}"));
        }
        if value.trim().is_empty() {
            return Err(format!("attribute value must not be empty: {key}"));
        }
        if attributes
            .insert(key.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(format!("attribute key is repeated: {key}"));
        }
    }
    Ok(attributes)
}

pub(crate) fn parse_history_filter(args: &HistoryArgs) -> Result<HistoryQuery, String> {
    if args
        .since
        .is_some_and(|since| args.until.is_some_and(|until| since > until))
    {
        return Err("since must not be later than until".into());
    }
    let has_explicit_bounds = args.since.is_some() || args.until.is_some();
    let filter = HistoryFilter::from_query(
        if has_explicit_bounds { None } else { args.days },
        has_explicit_bounds || args.all,
        args.platform,
        args.status,
        Utc::now(),
    )
    .map_err(|error| error.to_string())?;
    Ok(HistoryQuery {
        filter,
        phone: args.phone.clone(),
        since: args.since,
        until: args.until,
        limit: args.limit.get(),
    })
}

impl HistoryQuery {
    pub(crate) fn filter(&self, history: Vec<HistoryRecord>) -> Vec<HistoryRecord> {
        let mut retained = self
            .filter
            .filter(history)
            .into_iter()
            .filter(|record| {
                self.phone
                    .as_ref()
                    .is_none_or(|phone| record.request.account.phone.as_ref() == Some(phone))
            })
            .filter(|record| {
                let date = record.recorded_at.with_timezone(&Local).date_naive();
                self.since.is_none_or(|since| date >= since)
                    && self.until.is_none_or(|until| date <= until)
            })
            .collect::<Vec<_>>();
        retained.sort_by(|left, right| {
            right
                .recorded_at
                .cmp(&left.recorded_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        retained.truncate(self.limit);
        retained
    }
}

pub(crate) fn parse_request(args: PublishArgs) -> Result<PublishRequest, String> {
    let targets = args
        .platforms
        .iter()
        .map(|value| Platform::from_str(value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut overrides = args
        .platform_overrides
        .into_iter()
        .map(|value| {
            serde_json::from_str::<PlatformOverride>(&value)
                .map_err(|error| format!("invalid --platform-override JSON: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(statement) = args.creative_statement {
        for platform in &targets {
            if let Some(override_value) =
                overrides.iter_mut().find(|item| item.platform == *platform)
            {
                override_value.creative_statement = Some(statement.clone());
            } else {
                overrides.push(PlatformOverride {
                    platform: *platform,
                    title: None,
                    short_title: None,
                    tags: None,
                    creative_statement: Some(statement.clone()),
                    account: None,
                    wechat_link: None,
                });
            }
        }
    }
    let source = match url::Url::parse(&args.file) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => MediaSource::RemoteUrl(url),
        Ok(url) => {
            return Err(format!(
                "unsupported remote source scheme: {}",
                url.scheme()
            ));
        }
        Err(_) => MediaSource::LocalFile(args.file.into()),
    };
    let request = PublishRequest {
        source,
        title: args.title,
        short_title: args.short_title,
        tags: args.tags,
        address: args.address,
        draft: args.draft,
        bt2: args.bt2,
        scheduled_at: args.publish_at,
        task_name: args.task_name,
        account: AccountSelection {
            phone: args.phone,
            partition: args.partition,
        },
        wechat_link: WechatLink {
            product_id: args.sph_product_id,
            link_type: args.sph_link_type,
            link_value: args.sph_link_value,
        },
        overrides,
        targets,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}
