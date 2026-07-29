use super::*;
use matrixpost_core::{Platform, PublishRequest};

impl<T: WebDriverTransport> WebDriverPublisher<T> {
    pub(crate) fn wechat_product_id(request: &PublishRequest) -> Result<Option<String>, String> {
        let link = &request.wechat_link;
        let product_id = link.product_id.as_deref().map(str::trim);
        let link_type = link.link_type.as_deref().map(str::trim);
        let link_value = link.link_value.as_deref().map(str::trim);
        // MatrixMedia accepts the explicit sphProductId independently of the
        // optional `sphLink` object. Keep that precedence: a product ID is a
        // complete product attachment request even if old input carries an
        // unrelated link-type field beside it.
        let product_id = if let Some(product_id) = product_id.filter(|value| !value.is_empty()) {
            product_id
        } else {
            match link_type {
                None if link_value.is_none() => return Ok(None),
                Some(value) if value.eq_ignore_ascii_case("none") => return Ok(None),
                Some(value) if value.eq_ignore_ascii_case("product") => link_value
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        "WeChat product link requires a non-empty product identifier".to_owned()
                    })?,
                Some(_) => {
                    return Err("WeChat link type is not supported by the local runner".into());
                }
                None => return Err("WeChat link type is required for its link value".into()),
            }
        };
        if product_id.len() > 128 || product_id.chars().any(char::is_control) {
            return Err("WeChat product identifier is malformed".into());
        }
        Ok(Some(product_id.to_owned()))
    }

    pub(super) fn description(platform: Platform, request: &PublishRequest) -> String {
        let override_value = request
            .overrides
            .iter()
            .find(|item| item.platform == platform);
        let tags = override_value
            .and_then(|item| item.tags.as_ref())
            .unwrap_or(&request.tags);
        let mut fields = tags.iter().map(|tag| format!("#{tag}")).collect::<Vec<_>>();
        if let Some(address) = &request.address {
            fields.push(address.clone());
        }
        if !matches!(
            platform,
            Platform::WechatChannels
                | Platform::Douyin
                | Platform::Bilibili
                | Platform::Baijiahao
                | Platform::Kuaishou
                | Platform::Toutiao
                | Platform::Xiaohongshu
        ) && let Some(statement) =
            override_value.and_then(|item| item.creative_statement.as_ref())
        {
            fields.push(statement.clone());
        }
        fields.join(" ")
    }

    pub(super) fn title(platform: Platform, request: &PublishRequest) -> &str {
        request
            .overrides
            .iter()
            .find(|item| item.platform == platform)
            .and_then(|item| item.title.as_deref())
            .unwrap_or(&request.title)
    }

    pub(super) fn short_title(platform: Platform, request: &PublishRequest) -> Option<&str> {
        request
            .overrides
            .iter()
            .find(|item| item.platform == platform)
            .and_then(|item| item.short_title.as_deref())
            .or(request.short_title.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}
