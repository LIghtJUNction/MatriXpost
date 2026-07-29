use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};

/// Bilibili uses a select list rather than a modal confirmation. The exact,
/// enabled option is the final confirmation; the runner verifies the list
/// closes before allowing the generic submit action.
pub(crate) const BILIBILI_STATEMENT_OPEN_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},inputs=Array.from(document.querySelectorAll('.statement-content .bcc-select-input-wrap input')).filter(visible);if(inputs.length!==1)return false;inputs[0].focus();inputs[0].click();return true;"#;
pub(crate) const BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);return matches.length===1;"#;
pub(crate) const BILIBILI_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);if(matches.length!==1)return false;const target=options(matches[0])[0],action=target.closest('li.bcc-option,.auth-content'),disabled=action?.disabled||action?.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(action?.className||''));if(!action||disabled)return false;action.click();return true;"#;
pub(crate) const BILIBILI_STATEMENT_LIST_GONE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);return matches.length===0;"#;
/// Submit exactly one Bilibili tag through the upstream tag input. The script
/// refuses a missing, ambiguous, or disabled input before it mutates the page.
pub(crate) const BILIBILI_TAG_SUBMIT_SCRIPT: &str = r#"const expected=String(arguments[0]??'').trim(),visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},inputs=Array.from(document.querySelectorAll('.tag-container .input-instance input')).filter(visible);if(!expected||inputs.length!==1)return false;const input=inputs[0],disabled=input.disabled||input.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(input.className||''));if(disabled)return false;const setter=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set;if(!setter)return false;input.focus();setter.call(input,expected);input.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:expected}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',code:'Enter',bubbles:true,cancelable:true}));input.dispatchEvent(new KeyboardEvent('keyup',{key:'Enter',code:'Enter',bubbles:true}));return input.value===expected||input.value==='';"#;
/// Bilibili accepts a tag only after the controlled input clears. Require the
/// same unique, enabled input while waiting so page changes cannot be mistaken
/// for a successful submission.
pub(crate) const BILIBILI_TAG_COMMITTED_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},inputs=Array.from(document.querySelectorAll('.tag-container .input-instance input')).filter(visible);if(inputs.length!==1)return false;const input=inputs[0],disabled=input.disabled||input.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(input.className||''));return !disabled&&input.value==='';"#;
/// MatrixMedia's Bilibili flow treats the visible success badge as the only
/// upload-complete signal.  Classify it before *any* form mutation so an
/// unfinished upload cannot acquire metadata, declarations, or a final action.
pub(crate) const BILIBILI_UPLOAD_READY_STATE_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},states=Array.from(document.querySelectorAll('.file-item-content-status .success')).filter(visible);if(states.length===0)return'pending';return states.length===1?'ready':'ambiguous';"#;
pub(crate) const BILIBILI_UPLOAD_READY_POLL_ATTEMPTS: usize = 150;
pub(crate) const BILIBILI_UPLOAD_READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Match `creativeStatement.js` normalization before resolving Bilibili's
/// page label. This deliberately retains the unified self-shot fallback label;
/// a Bilibili page that does not offer it fails closed instead of mislabeling.
pub(crate) fn bilibili_creative_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Bilibili)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    Some(match value {
        "none" | "无标注" | "内容无需标注" | "无需添加自主声明" | "无需标注" | "无需声明" => {
            "内容无需标注"
        }
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => "含AI生成内容",
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历"
        | "内容为虚构剧情，仅供娱乐" => "含虚构演绎内容",
        "marketing" | "营销推广" | "内容含营销信息" | "内容含营销推广信息" | "内容包含营销广告" => {
            "内容含营销信息"
        }
        "personal_opinion" | "个人观点" | "个人观点，仅供参考" | "内容为个人观点或见解" => {
            "个人观点，仅供参考"
        }
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            "内容为转载"
        }
        "self_shot" | "自行拍摄" | "内容为自行拍摄" => "自行拍摄",
        "self_made_no_repost" | "自制禁转载" | "内容为自制：未经作者允许，禁止转载" => {
            "内容为自制：未经作者允许，禁止转载"
        }
        _ => "内容无需标注",
    })
}
