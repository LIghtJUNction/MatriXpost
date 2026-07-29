use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};

/// Every shadow-root metadata phase has a fixed deadline. The runner never
/// waits for unbounded UI activity in an attached user browser.
pub(crate) const WECHAT_SHADOW_ACTION_POLL_ATTEMPTS: usize = 30;
pub(crate) const WECHAT_SHADOW_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Video Channels may transcode after the form fields are filled. Keep the
/// upstream thirty-second upload-processing window separate from metadata UI.
pub(crate) const WECHAT_UPLOAD_READY_POLL_ATTEMPTS: usize = 150;
pub(crate) const WECHAT_UPLOAD_READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) fn wechat_creative_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::WechatChannels)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    match value {
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => Some("含AI生成内容"),
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历" => Some("内容为虚构剧情，仅供娱乐"),
        "marketing" | "营销推广" | "内容含营销信息" | "内容含营销推广信息" => {
            Some("内容包含营销广告")
        }
        "personal_opinion" | "个人观点" | "个人观点，仅供参考" | "内容为个人观点或见解" => {
            Some("个人观点，仅供参考")
        }
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            Some("内容为转载")
        }
        "self_shot" | "自行拍摄" | "内容为自行拍摄" => Some("内容为自行拍摄"),
        _ => None,
    }
}
/// These scripts return only booleans. Product names, identifiers, and DOM
/// content deliberately never leave the attached page through WebDriver.
pub(crate) const WECHAT_PRODUCT_TYPE_READY_SCRIPT: &str = r#"const app=document.querySelector('wujie-app.wujie_iframe');const root=app?.shadowRoot;if(!root)return false;const link=root.querySelector('.post-with-link');if(!link)return false;const selected=String(link.querySelector('.choosen-link-wrap span')?.textContent||'').trim();const chooser=link.querySelector('.post-component-choose-wrap .content-wrap');if(selected==='商品'&&chooser)return true;const menu=link.querySelector('.link-list-options');const visible=menu&&getComputedStyle(menu).display!=='none';if(!visible){link.querySelector('.link-display-wrap')?.click();return false;}const product=Array.from(menu.querySelectorAll('.link-option-item')).find(item=>String(item.textContent||'').replace(/\s+/g,'')==='商品');product?.click();return false;"#;
pub(crate) const WECHAT_PRODUCT_OPEN_CHOOSER_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const chooser=root?.querySelector('.post-with-link .post-component-choose-wrap .content-wrap');if(!chooser)return false;chooser.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>String(dialog.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(dialog).display!=='none'));"#;
pub(crate) const WECHAT_PRODUCT_SEARCH_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const input=dialog?.querySelector('input[placeholder="请输入商品名称/编码搜索"]');const button=dialog?.querySelector('.search-btn button');if(!input||!button)return false;const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set;if(!set)return false;set.call(input,id);input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));button.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_EXACT_ROW_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Array.from(root?.querySelectorAll('.weui-desktop-dialog tr[data-row-key]')||[]).some(row=>row.getAttribute('data-row-key')===id);"#;
pub(crate) const WECHAT_PRODUCT_SELECT_EXACT_SCRIPT: &str = r#"const id=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const row=Array.from(root?.querySelectorAll('.weui-desktop-dialog tr[data-row-key]')||[]).find(item=>item.getAttribute('data-row-key')===id);const radio=Array.from(row?.querySelectorAll('input.ant-radio-input')||[]).find(item=>item.value===id);if(!radio)return false;radio.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_ADD_READY_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const button=Array.from(dialog?.querySelectorAll('.weui-desktop-btn_primary')||[]).find(item=>/^添加(?:\(\d+\))?$/.test(String(item.textContent||'').replace(/\s+/g,'')));return Boolean(button&&!button.disabled&&!button.classList.contains('weui-desktop-btn_disabled'));"#;
pub(crate) const WECHAT_PRODUCT_ADD_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>String(item.textContent||'').includes('从橱窗添加商品')&&getComputedStyle(item).display!=='none');const button=Array.from(dialog?.querySelectorAll('.weui-desktop-btn_primary')||[]).find(item=>/^添加(?:\(\d+\))?$/.test(String(item.textContent||'').replace(/\s+/g,'')));if(!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;button.click();return true;"#;
pub(crate) const WECHAT_PRODUCT_ATTACHED_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const name=root?.querySelector('.post-with-link .post-component-choose-wrap .choose-content .name');return Boolean(String(name?.textContent||'').trim());"#;
/// These scripts accept only a known creative-statement label and return a
/// boolean. They never expose page content to the runner.
pub(crate) const WECHAT_CREATIVE_STATEMENT_OPEN_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const trigger=root?.querySelector('.post-with-mark-tag .select-display');if(!trigger)return false;trigger.click();return true;"#;
pub(crate) const WECHAT_CREATIVE_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0];const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const option=Array.from(root?.querySelectorAll('.post-with-mark-tag .mark-tag-option')||[]).find(item=>String(item.querySelector('.option-main')?.textContent||'').trim()===expected);if(!option)return false;option.click();return true;"#;
/// Classify only the upload-processing control inside Video Channels' shadow
/// root. It intentionally returns a finite state token, never page text.
pub(crate) const WECHAT_UPLOAD_READY_STATE_SCRIPT: &str = r#"const app=document.querySelector('wujie-app.wujie_iframe'),root=app?.shadowRoot;if(!root)return'invalid';const visible=item=>{const style=getComputedStyle(item),rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},tags=Array.from(root.querySelectorAll('.tag-inner')).filter(visible);if(tags.length>1)return'ambiguous';return tags.length===1&&String(tags[0].textContent||'').replace(/\s+/g,'').trim()==='删除'?'ready':'pending';"#;
/// Original-declaration scripts deliberately return only whether each local
/// page action is available or completed. The declaration itself is attempted
/// only for an explicit WeChat publication request.
pub(crate) const WECHAT_ORIGINAL_ENTRY_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const entry=root?.querySelector('.declare-original-checkbox .ant-checkbox-wrapper');if(!entry)return false;entry.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_ANY_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>getComputedStyle(dialog).display!=='none'&&(dialog.querySelector('.weui-desktop-dialog__bd .protocol-text')||dialog.matches('.declare-original-dialog .weui-desktop-dialog'))));"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return Boolean(Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(dialog=>getComputedStyle(dialog).display!=='none'&&dialog.querySelector('.weui-desktop-dialog__bd .protocol-text')));"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_CONFIRM_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).find(item=>getComputedStyle(item).display!=='none'&&item.querySelector('.weui-desktop-dialog__bd .protocol-text'));const protocol=dialog?.querySelector('.weui-desktop-dialog__bd .protocol-text');const button=Array.from(dialog?.querySelectorAll('button.weui-desktop-btn_primary')||[]).find(item=>String(item.textContent||'').trim().includes('声明原创'));if(!protocol||!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;protocol.click();button.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_PROTOCOL_DIALOG_GONE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;return !Array.from(root?.querySelectorAll('.weui-desktop-dialog')||[]).some(dialog=>getComputedStyle(dialog).display!=='none'&&dialog.querySelector('.weui-desktop-dialog__bd .protocol-text'));"#;
pub(crate) const WECHAT_ORIGINAL_DECLARATION_DIALOG_VISIBLE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');return Boolean(dialog&&getComputedStyle(dialog).display!=='none');"#;
pub(crate) const WECHAT_ORIGINAL_CONFIRM_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');const check=dialog?.querySelector('label.ant-checkbox-wrapper');const button=Array.from(dialog?.querySelectorAll('button.weui-desktop-btn_primary')||[]).find(item=>String(item.textContent||'').trim().includes('声明原创'));if(!check||!button||button.disabled||button.classList.contains('weui-desktop-btn_disabled'))return false;check.click();button.click();return true;"#;
pub(crate) const WECHAT_ORIGINAL_DECLARATION_DIALOG_GONE_SCRIPT: &str = r#"const root=document.querySelector('wujie-app.wujie_iframe')?.shadowRoot;const dialog=root?.querySelector('.declare-original-dialog .weui-desktop-dialog');return !dialog||getComputedStyle(dialog).display==='none';"#;
