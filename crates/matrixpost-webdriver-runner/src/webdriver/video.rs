use super::*;
use matrixpost_core::{MediaSource, Platform, PublishRequest};
use std::sync::atomic::Ordering;

impl<T: WebDriverTransport> PublicationExecutor for WebDriverPublisher<T> {
    fn publish(&self, platform: Platform, request: &PublishRequest) -> Result<String, String> {
        let profile = profile(platform)
            .ok_or_else(|| "no WebDriver profile is installed for platform".to_owned())?;
        let MediaSource::LocalFile(file) = &request.source else {
            return Err("WebDriver runner accepts only local media files".into());
        };
        let file = file
            .to_str()
            .ok_or_else(|| "local media path is not valid Unicode".to_owned())?;
        let wechat_product = (platform == Platform::WechatChannels)
            .then(|| Self::wechat_product_id(request))
            .transpose()?
            .flatten();
        if platform == Platform::FanqieVideo && request.draft {
            return Err(
                "Fanqie Video does not support draft publication in the local runner".into(),
            );
        }
        if platform == Platform::Kuaishou && request.draft {
            return Err("Kuaishou does not support draft publication in the local runner".into());
        }
        let creative_statement = match platform {
            Platform::WechatChannels => wechat_creative_statement_label(request),
            Platform::Douyin => douyin_autonomous_statement_label(request),
            Platform::Baijiahao => baijiahao_creative_statement_label(request),
            Platform::Bilibili => bilibili_creative_statement_label(request),
            Platform::Kuaishou => kuaishou_creative_statement_label(request),
            Platform::Toutiao => toutiao_creative_statement_label(request),
            Platform::Xiaohongshu => xiaohongshu_creative_statement_label(request),
            _ => None,
        };
        let session = self.session()?;
        let outcome: Result<(), String> = (|| {
            self.navigate(&session, profile.upload_url)?;
            self.input(&session, profile.file, file)?;
            if platform == Platform::Bilibili {
                self.wait_for_bilibili_upload_ready(&session)?;
            }
            if platform == Platform::Kuaishou {
                self.input_kuaishou_metadata(&session, request)?;
            } else {
                self.input(&session, profile.title, Self::title(platform, request))?;
                if platform == Platform::WechatChannels
                    && let (Some(selectors), Some(short_title)) =
                        (profile.short_title, Self::short_title(platform, request))
                {
                    self.input(&session, selectors, short_title)?;
                }
                let description = Self::description(platform, request);
                if !description.is_empty() {
                    self.input(&session, profile.description, &description)?;
                }
            }
            if let Some(product_id) = wechat_product.as_deref() {
                self.attach_wechat_product(&session, product_id)?;
            }
            if platform == Platform::Douyin {
                self.prepare_douyin_video(&session)?;
            }
            if let Some(label) = creative_statement {
                match platform {
                    Platform::WechatChannels => {
                        self.apply_wechat_creative_statement(&session, label)?
                    }
                    Platform::Douyin => self.apply_douyin_autonomous_statement(&session, label)?,
                    Platform::Baijiahao => {
                        self.apply_baijiahao_creative_statement(&session, label)?
                    }
                    Platform::Bilibili => {
                        self.apply_bilibili_creative_statement(&session, label)?
                    }
                    Platform::Kuaishou => {
                        self.apply_kuaishou_creative_statement(&session, label)?
                    }
                    Platform::Toutiao => self.apply_toutiao_creative_statement(&session, label)?,
                    Platform::Xiaohongshu => {
                        self.apply_xiaohongshu_creative_statement(&session, label)?
                    }
                    _ => unreachable!("only declaration-capable platforms resolve a statement"),
                }
            }
            if platform == Platform::WechatChannels {
                self.try_declare_wechat_original(&session)?;
                self.wait_for_wechat_upload_ready(&session)?;
            }
            if platform == Platform::Xiaohongshu {
                self.normalize_xiaohongshu_pk_cover(&session)?;
            }
            if platform == Platform::FanqieVideo {
                self.publish_fanqie_video(&session)?;
            } else {
                if self.success_marker_visible(&session, profile)? {
                    return Err(
                        "a success marker was already visibly present before the publish action"
                            .into(),
                    );
                }
                if platform == Platform::Baijiahao {
                    self.publish_baijiahao_action(&session, request.draft)?;
                } else if platform == Platform::Toutiao {
                    self.publish_toutiao_footer(&session, request.draft)?;
                } else if platform == Platform::Kuaishou {
                    self.publish_kuaishou_action(&session)?;
                } else {
                    let action = if request.draft {
                        profile.draft
                    } else {
                        profile.submit
                    };
                    self.click(&session, action)?;
                }
                self.wait_for_success_transition(&session, profile)?;
            }
            Ok(())
        })();
        let cleanup = self.delete_session(&session);
        outcome?;
        cleanup?;
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        Ok(format!("webdriver-{}-{job}", platform.as_str()))
    }
}
