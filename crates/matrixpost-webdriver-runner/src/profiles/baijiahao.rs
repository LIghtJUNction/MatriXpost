use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};

/// Baijiahao declarations use ordinary DOM modals instead of a shadow root.
/// Scripts accept only known resolved labels and return booleans, keeping page
/// content and identifiers inside the attached browser.
pub(crate) const BAIJIAHAO_STATEMENT_OPEN_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},inputs=Array.from(document.querySelectorAll('input[placeholder=\"请选择创作声明\"]')).filter(visible);if(inputs.length!==1)return false;const input=inputs[0],wrap=input.closest('.form-inner-wrap')||input.parentElement;if(!wrap)return false;input.focus();input.click();if(wrap!==input)wrap.click();return true;"#;
pub(crate) const BAIJIAHAO_STATEMENT_DIALOG_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),roots=Array.from(document.querySelectorAll('.cheetah-modal')).filter(visible),dialogs=roots.length?roots:Array.from(document.querySelectorAll('.cheetah-modal-content')).filter(visible),matches=dialogs.filter(dialog=>Array.from(dialog.querySelectorAll('.cheetah-radio-wrapper')).some(item=>norm((item.closest('.flex.items-center')||item.parentElement||item).textContent)===norm(expected)));if(matches.length!==1)return false;return true;"#;
pub(crate) const BAIJIAHAO_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),roots=Array.from(document.querySelectorAll('.cheetah-modal')).filter(visible),dialogs=roots.length?roots:Array.from(document.querySelectorAll('.cheetah-modal-content')).filter(visible),matches=dialogs.filter(dialog=>Array.from(dialog.querySelectorAll('.cheetah-radio-wrapper')).some(item=>norm((item.closest('.flex.items-center')||item.parentElement||item).textContent)===norm(expected)));if(matches.length!==1)return false;const options=Array.from(matches[0].querySelectorAll('.cheetah-radio-wrapper')).filter(item=>norm((item.closest('.flex.items-center')||item.parentElement||item).textContent)===norm(expected));if(options.length!==1)return false;options[0].click();return true;"#;
pub(crate) const BAIJIAHAO_STATEMENT_CONFIRM_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),roots=Array.from(document.querySelectorAll('.cheetah-modal')).filter(visible),dialogs=roots.length?roots:Array.from(document.querySelectorAll('.cheetah-modal-content')).filter(visible),matches=dialogs.filter(dialog=>Array.from(dialog.querySelectorAll('.cheetah-radio-wrapper')).some(item=>norm((item.closest('.flex.items-center')||item.parentElement||item).textContent)===norm(expected)));if(matches.length!==1)return false;const buttons=Array.from(matches[0].querySelectorAll('.cheetah-modal-footer button.cheetah-btn-primary')).filter(item=>!item.disabled&&item.getAttribute('aria-disabled')!=='true');if(buttons.length!==1)return false;buttons[0].click();return true;"#;
pub(crate) const BAIJIAHAO_STATEMENT_DIALOG_GONE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),roots=Array.from(document.querySelectorAll('.cheetah-modal')).filter(visible),dialogs=roots.length?roots:Array.from(document.querySelectorAll('.cheetah-modal-content')).filter(visible),matches=dialogs.filter(dialog=>Array.from(dialog.querySelectorAll('.cheetah-radio-wrapper')).some(item=>norm((item.closest('.flex.items-center')||item.parentElement||item).textContent)===norm(expected)));return matches.length===0;"#;
/// Baijiahao does not expose a stable completion event after upload.  Keep the
/// decision local and finite: visible upload progress always wins, then one
/// visible operator root and one exact visible target button are required.
pub(crate) const BAIJIAHAO_ACTION_STATE_SCRIPT: &str = r#"const expected=arguments[0],visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},norm=v=>String(v||'').replace(/\s+/g,'').trim(),progress=Array.from(document.querySelectorAll('.upload-step-progress .progress-container.uploading,#cover-tabs-container .cheetah-progress-inner')).some(visible);if(progress)return'pending';const roots=Array.from(document.querySelectorAll('#new-operator-content .op-list-right')).filter(visible);if(roots.length===0)return'missing';if(roots.length!==1)return'ambiguous';const label=expected==='draft'?'存草稿':'发布',buttons=Array.from(roots[0].querySelectorAll('button')).filter(button=>visible(button)&&norm(button.textContent)===label);if(buttons.length===0)return'missing';if(buttons.length!==1)return'ambiguous';const button=buttons[0],disabled=button.disabled||button.hasAttribute('disabled')||button.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(button.className||''));return disabled?'disabled':'ready';"#;
/// Revalidate the same state immediately before the only allowed action; a
/// changed root, upload, target, or disabled state cannot fall through to a
/// generic profile click.
pub(crate) const BAIJIAHAO_ACTION_SCRIPT: &str = r#"const expected=arguments[0],visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},norm=v=>String(v||'').replace(/\s+/g,'').trim(),progress=Array.from(document.querySelectorAll('.upload-step-progress .progress-container.uploading,#cover-tabs-container .cheetah-progress-inner')).some(visible);if(progress)return'pending';const roots=Array.from(document.querySelectorAll('#new-operator-content .op-list-right')).filter(visible);if(roots.length===0)return'missing';if(roots.length!==1)return'ambiguous';const label=expected==='draft'?'存草稿':'发布',buttons=Array.from(roots[0].querySelectorAll('button')).filter(button=>visible(button)&&norm(button.textContent)===label);if(buttons.length===0)return'missing';if(buttons.length!==1)return'ambiguous';const button=buttons[0],disabled=button.disabled||button.hasAttribute('disabled')||button.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(button.className||''));if(disabled)return'disabled';button.click();return'clicked';"#;
pub(crate) const BAIJIAHAO_ACTION_POLL_ATTEMPTS: usize = 30;
pub(crate) const BAIJIAHAO_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// MatrixMedia resolves a declaration per target platform. Unsupported and
/// unknown Baijiahao values explicitly select the upstream `none` option.
pub(crate) fn baijiahao_creative_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Baijiahao)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    Some(match value {
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
        | "虚构演绎，故事经历" => "含虚构演绎内容",
        "marketing" | "营销推广" | "内容含营销信息" | "内容含营销推广信息" => {
            "内容含营销信息"
        }
        "personal_opinion" | "个人观点" | "个人观点，仅供参考" | "内容为个人观点或见解" => {
            "个人观点，仅供参考"
        }
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            "内容为转载"
        }
        _ => "无需声明",
    })
}
