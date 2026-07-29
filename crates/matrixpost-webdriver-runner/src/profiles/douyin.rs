use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};

/// Autonomous-statement interactions are bounded independently from generic
/// publish acknowledgement. A visible but incomplete declaration must never
/// be followed by a submit action.
pub(crate) const DOUYIN_STATEMENT_POLL_ATTEMPTS: usize = 30;
pub(crate) const DOUYIN_STATEMENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Upload processing and save-permission controls are independent finite
/// states. The runner must prove both before it opens an autonomous statement
/// or reaches a publish control. MatrixMedia gives both controls 30 seconds.
pub(crate) const DOUYIN_READY_DEADLINE: Duration = Duration::from_secs(30);
pub(crate) const DOUYIN_READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const DOUYIN_READY_POLL_ATTEMPTS: usize =
    (DOUYIN_READY_DEADLINE.as_millis() / DOUYIN_READY_POLL_INTERVAL.as_millis()) as usize;
/// These scripts return only booleans and accept one of the finite labels
/// resolved by the Rust runner. They intentionally do not return page text.
pub(crate) const DOUYIN_STATEMENT_OPEN_SCRIPT: &str = r#"const n=v=>String(v||'').replace(/\s+/g,'').trim();const keys=['请选择自主声明','添加自主声明','自主声明'];for(const el of document.querySelectorAll('[class]')){const classes=typeof el.className==='string'?el.className.split(/\s+/):[];if(!classes.some(item=>item.startsWith('selectText-')))continue;if(keys.some(key=>n(el.textContent).includes(n(key)))){el.click();return true;}}for(const el of document.querySelectorAll('span,div,label,[role="button"],.semi-select')){const text=n(el.textContent);if(text.length<=24&&keys.some(key=>text.includes(n(key)))){el.click();return true;}}return false;"#;
pub(crate) const DOUYIN_STATEMENT_DIALOG_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},matches=Array.from(document.querySelectorAll('.semi-modal-body')).filter(body=>visible(body)&&Array.from(body.querySelectorAll('.semi-radio-addon')).some(item=>String(item.textContent||'').trim()===expected));return matches.length===1;"#;
pub(crate) const DOUYIN_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},matches=Array.from(document.querySelectorAll('.semi-modal-body')).filter(body=>visible(body)&&Array.from(body.querySelectorAll('.semi-radio-addon')).some(item=>String(item.textContent||'').trim()===expected));if(matches.length!==1)return false;const option=Array.from(matches[0].querySelectorAll('.semi-radio-addon')).find(item=>String(item.textContent||'').trim()===expected);const label=option?.closest('label.semi-radio');if(!label)return false;label.click();return true;"#;
pub(crate) const DOUYIN_STATEMENT_CONFIRM_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},matches=Array.from(document.querySelectorAll('.semi-modal-body')).filter(body=>visible(body)&&Array.from(body.querySelectorAll('.semi-radio-addon')).some(item=>String(item.textContent||'').trim()===expected));if(matches.length!==1)return false;const root=matches[0].closest('.semi-modal')||matches[0],buttons=Array.from(root.querySelectorAll('.semi-button.semi-button-primary')).filter(item=>!item.disabled&&item.getAttribute('aria-disabled')!=='true');if(buttons.length!==1)return false;buttons[0].click();return true;"#;
pub(crate) const DOUYIN_STATEMENT_DIALOG_GONE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},matches=Array.from(document.querySelectorAll('.semi-modal-body')).filter(body=>visible(body)&&Array.from(body.querySelectorAll('.semi-radio-addon')).some(item=>String(item.textContent||'').trim()===expected));return matches.length===0;"#;
/// Douyin's post-upload preview is ready only when exactly one visible
/// douyin.com video has exactly one visible horizontal progress bar in its
/// immediate container. The classifier never returns DOM content.
pub(crate) const DOUYIN_PREVIEW_STATE_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},videos=Array.from(document.querySelectorAll('video')).filter(video=>visible(video)&&String(video.currentSrc||video.getAttribute('src')||'').includes('douyin.com'));if(videos.length===0)return'pending';if(videos.length!==1)return'ambiguous';const root=videos[0].parentElement,bars=Array.from(root?.querySelectorAll('.rc-slider.rc-slider-horizontal')||[]).filter(visible);return bars.length===1?'ready':bars.length===0?'pending':'ambiguous';"#;
/// The permission classifier intentionally derives its scope from the
/// "不允许" option, then deduplicates its nearest visible 保存权限 ancestor.
/// This avoids broad page-wide input selection while still tolerating nested
/// layout wrappers introduced by the publisher.
pub(crate) const DOUYIN_SAVE_PERMISSION_STATE_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),scope=label=>{let node=label;for(let i=0;i<28&&node;i++,node=node.parentElement){if(visible(node)&&Array.from(node.querySelectorAll('span')).some(span=>norm(span.textContent).includes('保存权限')))return node;}return null;},labels=Array.from(document.querySelectorAll('label')).filter(label=>visible(label)&&norm(label.textContent).includes('不允许')),candidates=labels.map(label=>({label,input:label.querySelector('input[value="0"]'),root:scope(label)})).filter(item=>item.input&&(['checkbox','radio'].includes(item.input.type))&&item.root),roots=Array.from(new Set(candidates.map(item=>item.root)));if(candidates.length===0)return'pending';if(roots.length!==1)return'ambiguous';const items=candidates.filter(item=>item.root===roots[0]);if(items.length!==1)return'ambiguous';const item=items[0],disabled=item.input.disabled||item.input.getAttribute('aria-disabled')==='true'||item.label.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(item.label.className||''));if(disabled)return'disabled';return item.input.checked?'selected':'ready';"#;
/// The action repeats the whole scope invariant immediately before its only
/// click. It succeeds idempotently when the selected input is already checked.
pub(crate) const DOUYIN_SAVE_PERMISSION_ACTION_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),scope=label=>{let node=label;for(let i=0;i<28&&node;i++,node=node.parentElement){if(visible(node)&&Array.from(node.querySelectorAll('span')).some(span=>norm(span.textContent).includes('保存权限')))return node;}return null;},labels=Array.from(document.querySelectorAll('label')).filter(label=>visible(label)&&norm(label.textContent).includes('不允许')),candidates=labels.map(label=>({label,input:label.querySelector('input[value="0"]'),root:scope(label)})).filter(item=>item.input&&(['checkbox','radio'].includes(item.input.type))&&item.root),roots=Array.from(new Set(candidates.map(item=>item.root)));if(candidates.length===0)return'pending';if(roots.length!==1)return'ambiguous';const items=candidates.filter(item=>item.root===roots[0]);if(items.length!==1)return'ambiguous';const item=items[0],disabled=item.input.disabled||item.input.getAttribute('aria-disabled')==='true'||item.label.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(item.label.className||''));if(disabled)return'disabled';if(item.input.checked)return'selected';item.label.click();return item.input.checked?'clicked':'invalid';"#;
/// Match MatrixMedia's target-specific Douyin resolver. An explicitly
/// supplied unsupported value follows the upstream `none` fallback and is
/// still selected on the page; an absent Douyin override remains absent.
pub(crate) fn douyin_autonomous_statement_label(request: &PublishRequest) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Douyin)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    Some(match value {
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => "内容由AI生成",
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历" => "虚构演绎，仅供娱乐",
        "marketing" | "营销推广" | "内容含营销信息" | "内容含营销推广信息" => {
            "内容含营销推广信息"
        }
        "personal_opinion" | "个人观点" | "个人观点，仅供参考" | "内容为个人观点或见解" => {
            "内容为个人观点或见解"
        }
        "repost" | "转载" | "内容为转载" | "内容为转载信息" | "取自站外" | "素材来源于网络" => {
            "内容为转载信息"
        }
        _ => "无需添加自主声明",
    })
}
