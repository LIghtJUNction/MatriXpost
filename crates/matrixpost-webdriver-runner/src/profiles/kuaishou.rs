use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};

/// Kuaishou offers a finite subset of the unified creative-statement values
/// through an Ant Design select. Every script returns only a boolean and uses
/// an exact resolved label, so no page content leaves the attached browser.
pub(crate) const KUAISHOU_STATEMENT_OPEN_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),keys=['作者声明','作品声明','创作声明','声明'],candidates=Array.from(document.querySelectorAll('[class*="edit-form-item"],[class*="form-item"]')).filter(item=>{const label=item.querySelector('label');const trigger=item.querySelector('.ant-select-selector');return label&&trigger&&visible(item)&&visible(trigger)&&keys.some(key=>norm(label.textContent).includes(key));});if(candidates.length!==1)return false;const trigger=candidates[0].querySelector('.ant-select-selector');trigger.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true,view:window}));trigger.click();return true;"#;
pub(crate) const KUAISHOU_STATEMENT_LIST_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('.ant-select-item.ant-select-item-option')).filter(item=>visible(item)&&norm(item.getAttribute('title')||item.querySelector('.ant-select-item-option-content')?.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.ant-select-dropdown')).filter(visible),matches=lists.filter(list=>options(list).length===1);return matches.length===1;"#;
pub(crate) const KUAISHOU_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('.ant-select-item.ant-select-item-option')).filter(item=>visible(item)&&norm(item.getAttribute('title')||item.querySelector('.ant-select-item-option-content')?.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.ant-select-dropdown')).filter(visible),matches=lists.filter(list=>options(list).length===1);if(matches.length!==1)return false;const option=options(matches[0])[0],disabled=option.disabled||option.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(option.className||''));if(disabled)return false;option.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true,view:window}));option.click();return true;"#;
pub(crate) const KUAISHOU_STATEMENT_APPLIED_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),keys=['作者声明','作品声明','创作声明','声明'],candidates=Array.from(document.querySelectorAll('[class*="edit-form-item"],[class*="form-item"]')).filter(item=>{const label=item.querySelector('label');const trigger=item.querySelector('.ant-select-selector');return label&&trigger&&visible(item)&&visible(trigger)&&keys.some(key=>norm(label.textContent).includes(key));});if(candidates.length!==1)return false;const trigger=candidates[0].querySelector('.ant-select-selector');const open=Array.from(document.querySelectorAll('.ant-select-dropdown')).some(visible);return norm(trigger.textContent)===norm(expected)&&!open;"#;
/// Kuaishou has its own upload-complete signal and publishing surface.  A
/// missing preview is the sole transient state; all other non-unique or
/// disabled controls are terminal so an unfinished page cannot fall back to a
/// generic profile action.
pub(crate) const KUAISHOU_ACTION_STATE_SCRIPT: &str = r#"const visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},norm=v=>String(v||'').replace(/\s+/g,'').trim(),previews=Array.from(document.querySelectorAll('#preview-tours video')).filter(visible);if(previews.length===0)return'pending';if(previews.length!==1)return'ambiguous';const bars=Array.from(document.querySelectorAll('#setting-tours + div')).filter(visible);if(bars.length===0)return'missing';if(bars.length!==1)return'ambiguous';const buttons=Array.from(bars[0].querySelectorAll('button')).filter(button=>visible(button)&&norm(button.textContent)==='发布');if(buttons.length===0)return'missing';if(buttons.length!==1)return'ambiguous';const button=buttons[0],disabled=button.disabled||button.hasAttribute('disabled')||button.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(button.className||''));return disabled?'disabled':'ready';"#;
/// This repeats the complete readiness invariant immediately before the one
/// allowed Kuaishou click, preventing a page race from changing the target.
pub(crate) const KUAISHOU_ACTION_SCRIPT: &str = r#"const visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},norm=v=>String(v||'').replace(/\s+/g,'').trim(),previews=Array.from(document.querySelectorAll('#preview-tours video')).filter(visible);if(previews.length===0)return'pending';if(previews.length!==1)return'ambiguous';const bars=Array.from(document.querySelectorAll('#setting-tours + div')).filter(visible);if(bars.length===0)return'missing';if(bars.length!==1)return'ambiguous';const buttons=Array.from(bars[0].querySelectorAll('button')).filter(button=>visible(button)&&norm(button.textContent)==='发布');if(buttons.length===0)return'missing';if(buttons.length!==1)return'ambiguous';const button=buttons[0],disabled=button.disabled||button.hasAttribute('disabled')||button.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(button.className||''));if(disabled)return'disabled';button.click();return'clicked';"#;
pub(crate) const KUAISHOU_ACTION_POLL_ATTEMPTS: usize = 30;
pub(crate) const KUAISHOU_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Match MatrixMedia's Kuaishou resolver. The upstream leaves `none`,
/// unsupported values, and absent target overrides at the page default, so
/// this returns `None` for all three rather than inventing a declaration.
pub(crate) fn kuaishou_creative_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Kuaishou)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    match value {
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => Some("内容为AI生成"),
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历"
        | "内容为虚构剧情，仅供娱乐" => Some("演绎情节，仅供娱乐"),
        "personal_opinion" | "个人观点" | "个人观点，仅供参考" | "内容为个人观点或见解" => {
            Some("个人观点，仅供参考")
        }
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            Some("素材来源于网络")
        }
        _ => None,
    }
}
