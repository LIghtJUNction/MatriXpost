use std::{collections::BTreeMap, str::FromStr};

use chrono::{DateTime, Utc};
use matrixpost_core::{
    AccountSelection, ApprovalStatus, BusinessObjectStatus, HistoryFilter, LedgerDirection,
    MediaSource, Platform, PlatformOverride, PublishRequest, WechatLink,
};

use crate::args::{HistoryArgs, PublishArgs};

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

pub(crate) fn parse_history_filter(args: &HistoryArgs) -> Result<HistoryFilter, String> {
    HistoryFilter::from_query(args.days, args.all, args.platform, args.status, Utc::now())
        .map_err(|error| error.to_string())
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
