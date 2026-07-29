use matrixpost_core::{Platform, PublishRequest};

/// Toutiao video-source declarations are independent visible checkboxes. The
/// scripts find exactly one enabled label and return finite action states, so page
/// text and unrelated checkbox state never leave the attached browser.
pub(crate) const TOUTIAO_STATEMENT_SELECTED_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),matches=Array.from(document.querySelectorAll('.byte-checkbox.checkbot-item')).filter(item=>visible(item)&&norm(item.querySelector('.byte-checkbox-inner-text')?.textContent)===norm(expected));return matches.length===1&&matches[0].querySelector('input[type="checkbox"]')?.checked===true;"#;
pub(crate) const TOUTIAO_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),matches=Array.from(document.querySelectorAll('.byte-checkbox.checkbot-item')).filter(item=>visible(item)&&norm(item.querySelector('.byte-checkbox-inner-text')?.textContent)===norm(expected));if(matches.length===0)return'missing';if(matches.length!==1)return'ambiguous';const wrap=matches[0],input=wrap.querySelector('input[type="checkbox"]'),action=wrap.querySelector('.byte-checkbox-wrapper')||wrap,disabled=!input||input.disabled||input.getAttribute('aria-disabled')==='true'||action.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(wrap.className||''))||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(action.className||''));if(disabled)return'disabled';if(input.checked)return'selected';input.click();if(!input.checked)action.click();return input.checked?'clicked':'unverified';"#;
/// After processing completes, MatrixMedia uses the visible draft button as
/// the sole horizontal signal.  This classifier intentionally refuses to
/// infer a vertical layout while the footer is missing, duplicated, or
/// disabled: a publish click on an unfinished page is not recoverable.
pub(crate) const TOUTIAO_FOOTER_STATE_SCRIPT: &str = r#"const visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},disabled=e=>e.disabled||e.getAttribute('aria-disabled')==='true'||e.classList.contains('cannot-click'),ready=e=>visible(e)&&!disabled(e),footers=Array.from(document.querySelectorAll('.video-batch-footer')).filter(visible);if(footers.length===0)return'pending';if(footers.length!==1)return'ambiguous';const footer=footers[0],drafts=Array.from(footer.querySelectorAll('.draft')).filter(visible);if(drafts.length>1)return'ambiguous';if(drafts.length===1)return ready(drafts[0])?'horizontal_ready':'disabled';const submits=Array.from(footer.querySelectorAll('.submit')).filter(visible);if(submits.length===0)return'pending';if(submits.length!==1)return'ambiguous';return ready(submits[0])?'vertical_ready':'disabled';"#;
/// The action script repeats the finite-state invariant immediately before
/// clicking.  A changed footer therefore cannot turn a previously safe probe
/// into an unchecked publish action.
pub(crate) const TOUTIAO_FOOTER_ACTION_SCRIPT: &str = r#"const expected=arguments[0],visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;},disabled=e=>e.disabled||e.getAttribute('aria-disabled')==='true'||e.classList.contains('cannot-click'),ready=e=>visible(e)&&!disabled(e),footers=Array.from(document.querySelectorAll('.video-batch-footer')).filter(visible);if(footers.length===0)return'pending';if(footers.length!==1)return'ambiguous';const footer=footers[0],drafts=Array.from(footer.querySelectorAll('.draft')).filter(visible);if(drafts.length>1)return'ambiguous';if(drafts.length===1){if(!ready(drafts[0]))return'disabled';const submits=Array.from(footer.querySelectorAll('.submit')).filter(visible);if(expected==='draft'){drafts[0].click();return'clicked';}if(expected==='submit'&&submits.length===1&&ready(submits[0])){submits[0].click();return'clicked';}return expected==='submit'&&submits.length===1?'disabled':'invalid';}const submits=Array.from(footer.querySelectorAll('.submit')).filter(visible);if(submits.length===0)return'pending';if(submits.length!==1)return'ambiguous';if(!ready(submits[0]))return'disabled';if(expected!=='submit')return'invalid';submits[0].click();return'clicked';"#;
/// Toutiao only checks the three video-source labels offered by MatrixMedia.
/// Missing, `none`, and all unsupported values deliberately preserve the
/// page default rather than selecting or serializing an invented statement.
pub(crate) fn toutiao_creative_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Toutiao)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    match value {
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => Some("AI生成"),
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历"
        | "内容为虚构剧情，仅供娱乐" => Some("虚构演绎，故事经历"),
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            Some("取自站外")
        }
        _ => None,
    }
}
