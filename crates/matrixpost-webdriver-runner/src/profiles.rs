use std::time::Duration;

use matrixpost_core::{Platform, PublishRequest};
use url::Url;

pub(crate) const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";
pub(crate) const ELEMENT_POLL_ATTEMPTS: usize = 3;
pub(crate) const ELEMENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const ACKNOWLEDGEMENT_ATTEMPTS: usize = 60;
pub(crate) const ACKNOWLEDGEMENT_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const DEBUGGER_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
pub(crate) const VISIBLE_SCRIPT: &str = r#"const e=arguments[0];const s=getComputedStyle(e);const r=e.getBoundingClientRect();return !(e.getAttribute('aria-hidden')==='true'||s.display==='none'||s.visibility==='hidden'||Number(s.opacity)===0||r.width===0||r.height===0);"#;
pub(crate) const CODEMIRROR_WRITE_SCRIPT: &str = r#"const root=arguments[0],text=arguments[1];const editor=root.closest('.cm-editor')||root;const view=editor.cmView?.view||root.cmView?.view;if(view){view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});return view.state.doc.toString()===text;}root.focus();root.textContent=text;root.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:text}));return root.textContent===text;"#;
pub(crate) const MAX_ARTICLE_BODY_BYTES: usize = 1_000_000;
pub(crate) const MAX_ARTICLE_TITLE_BYTES: usize = 200;
pub(crate) const MAX_ARTICLE_CATEGORY_BYTES: usize = 64;
pub(crate) const MAX_ARTICLE_TAGS: usize = 10;
pub(crate) const MAX_ARTICLE_TAG_BYTES: usize = 32;
pub(crate) const MAX_ARTICLE_SUMMARY_BYTES: usize = 500;
pub(crate) const MAX_ARTICLE_COVER_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const ARTICLE_TEXT_EXTENSIONS: &[&str] = &["md", "txt"];
pub(crate) const ARTICLE_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
/// Remote videos are staged only into the user-selected directory before a
/// browser session is created.  Two GiB is large enough for ordinary platform
/// uploads while keeping the local runner's disk and network exposure bounded.
pub(crate) const MAX_REMOTE_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Accepted MIME types are deliberately finite: the runner stages videos, not
/// arbitrary remote objects. Parameters such as `charset` remain accepted by
/// the core policy's prefix comparison.
pub(crate) const REMOTE_VIDEO_CONTENT_TYPES: &[&str] = &[
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "video/x-matroska",
    "video/x-msvideo",
];
/// A remote source is untrusted input. Keep every execution failure at the
/// provider boundary intentionally opaque so neither its URL nor its staged
/// local path can be reflected to callers.
pub(crate) const REMOTE_MEDIA_EXECUTION_REJECTION: &str = "remote media publication failed";
pub(crate) const FANQIE_VIDEO_LIST_URL: &str =
    "https://pugc.yueduwuxian.com/fqvideo/home/video-list";
pub(crate) const FANQIE_REVIEW_SCROLL_ATTEMPTS: usize = 6;
pub(crate) const FANQIE_REVIEW_SCROLL_INTERVAL: Duration = Duration::from_millis(250);
/// This script is deliberately a one-way classifier. It receives a bounded
/// title query but returns only one fixed status token, never card/page text,
/// URLs, identifiers, or selectors.
pub(crate) const FANQIE_REVIEW_STATUS_SCRIPT: &str = r#"const n=v=>String(v||'').replace(/\s+/g,'').trim();const q=n(arguments[0]);for(const card of document.querySelectorAll('.video-card')){const t=n(card.querySelector('.video-card-title')?.textContent);if(!t||!t.includes(q))continue;const s=n(card.querySelector('.video-status')?.textContent);if(/审核未通过|未通过|驳回|违规|失败/.test(s))return'rejected';if(/审核中|待审核/.test(s))return'under_review';if(/已发布|发布成功|已上线|公开|正常/.test(s))return'published';return'under_review';}window.scrollBy(0,Math.max(window.innerHeight,800));return null;"#;
/// Every shadow-root metadata phase has a fixed deadline. The runner never
/// waits for unbounded UI activity in an attached user browser.
pub(crate) const WECHAT_SHADOW_ACTION_POLL_ATTEMPTS: usize = 30;
pub(crate) const WECHAT_SHADOW_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Video Channels may transcode after the form fields are filled. Keep the
/// upstream thirty-second upload-processing window separate from metadata UI.
pub(crate) const WECHAT_UPLOAD_READY_POLL_ATTEMPTS: usize = 150;
pub(crate) const WECHAT_UPLOAD_READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
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
/// Fanqie uses a separate publish surface from the generic video profiles.
/// Its state probes return only finite control tokens and never page content.
pub(crate) const FANQIE_PUBLISH_POLL_ATTEMPTS: usize = 30;
pub(crate) const FANQIE_PUBLISH_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const FANQIE_UPLOAD_READY_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const root=document.querySelector('[data-rbd-droppable-id="album-upload-list-a"]')||document.querySelector('[class*="upload-list"]');const states=Array.from(root?.querySelectorAll('.upload-status')||[]).filter(v).map(e=>String(e.textContent||'').replace(/\s+/g,''));return states.length>0&&states.every(s=>s.includes('上传完成')||s.includes('100%'));"#;
pub(crate) const FANQIE_CHANNEL_PANEL_OPEN_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const items=Array.from(document.querySelectorAll('.platform-panel-item')).filter(v);if(items.length)return'open';const candidates=Array.from(new Set(Array.from(document.querySelectorAll('.platform-panel-trigger,.publish-video-footer-left .platform-panel-trigger-text')).filter(e=>v(e)&&(e.classList.contains('platform-panel-trigger')||String(e.textContent||'').replace(/\s+/g,'').includes('发布至App'))).map(e=>e.closest('.platform-panel-trigger')||e)));if(candidates.length===0)return'missing';if(candidates.length!==1)return'ambiguous';candidates[0].click();return'opened';"#;
pub(crate) const FANQIE_CHANNEL_PANEL_VISIBLE_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const items=Array.from(document.querySelectorAll('.platform-panel-item')).filter(v);if(!items.length)return false;const panels=new Set(items.map(item=>item.closest('.platform-panel')||item.parentElement));return panels.size===1;"#;
pub(crate) const FANQIE_CHANNELS_ENABLE_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const items=Array.from(document.querySelectorAll('.platform-panel-item')).filter(v);const panels=new Set(items.map(item=>item.closest('.platform-panel')||item.parentElement));if(!items.length)return'missing';if(panels.size!==1)return'ambiguous';const switches=items.map(item=>item.querySelector('button[role="switch"]'));if(switches.some(sw=>!sw))return'missing';if(switches.some(sw=>sw.disabled||sw.getAttribute('aria-disabled')==='true'))return'disabled';let clicked=false;for(const sw of switches){const on=sw.getAttribute('aria-checked')==='true'||sw.classList.contains('arco-switch-checked');if(!on){sw.click();clicked=true;}}return clicked?'clicked':'selected';"#;
pub(crate) const FANQIE_CHANNELS_SELECTED_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const items=Array.from(document.querySelectorAll('.platform-panel-item')).filter(v);const panels=new Set(items.map(item=>item.closest('.platform-panel')||item.parentElement));if(!items.length||panels.size!==1)return false;const switches=items.map(item=>item.querySelector('button[role="switch"]'));return switches.length===items.length&&switches.every(sw=>sw&&(sw.getAttribute('aria-checked')==='true'||sw.classList.contains('arco-switch-checked')));"#;
pub(crate) const FANQIE_ONE_CLICK_PUBLISH_READY_SCRIPT: &str = r#"const text=b=>String(b.textContent||'').replace(/\s+/g,''),v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const all=Array.from(document.querySelectorAll('button')).filter(b=>text(b).includes('一键发布'));if(!all.length)return'missing';const buttons=all.filter(v);if(!buttons.length)return'pending';if(buttons.length!==1)return'ambiguous';const button=buttons[0];return button.disabled||button.getAttribute('aria-disabled')==='true'?'disabled':'ready';"#;
pub(crate) const FANQIE_ONE_CLICK_PUBLISH_SCRIPT: &str = r#"const text=b=>String(b.textContent||'').replace(/\s+/g,''),v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const buttons=Array.from(document.querySelectorAll('button')).filter(b=>v(b)&&text(b).includes('一键发布'));if(buttons.length===0)return'missing';if(buttons.length!==1)return'ambiguous';const button=buttons[0];if(button.disabled||button.getAttribute('aria-disabled')==='true')return'disabled';button.click();return'clicked';"#;
pub(crate) const FANQIE_PUBLISH_RESULT_SCRIPT: &str = r#"const v=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)!==0&&r.width>0&&r.height>0;};const notices=Array.from(document.querySelectorAll('.arco-message,.arco-notification,[class*="message"],[class*="toast"],[class*="Toast"]')).filter(v).map(e=>String(e.textContent||'').replace(/\s+/g,' '));if(notices.some(t=>/失败|错误|异常|请重试|未通过|不能为空/.test(t)))return'failure';if(notices.some(t=>/发布成功|提交成功|操作成功/.test(t)))return'success';return'pending';"#;
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

/// Bilibili uses a select list rather than a modal confirmation. The exact,
/// enabled option is the final confirmation; the runner verifies the list
/// closes before allowing the generic submit action.
pub(crate) const BILIBILI_STATEMENT_OPEN_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},inputs=Array.from(document.querySelectorAll('.statement-content .bcc-select-input-wrap input')).filter(visible);if(inputs.length!==1)return false;inputs[0].focus();inputs[0].click();return true;"#;
pub(crate) const BILIBILI_STATEMENT_LIST_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);return matches.length===1;"#;
pub(crate) const BILIBILI_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);if(matches.length!==1)return false;const target=options(matches[0])[0],action=target.closest('li.bcc-option,.auth-content'),disabled=action?.disabled||action?.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(action?.className||''));if(!action||disabled)return false;action.click();return true;"#;
pub(crate) const BILIBILI_STATEMENT_LIST_GONE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=list=>Array.from(list.querySelectorAll('li.bcc-option span,.auth-content .option-text')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),lists=Array.from(document.querySelectorAll('.statement-content .bcc-select-list-wrap')).filter(visible),matches=lists.filter(list=>options(list).length===1);return matches.length===0;"#;

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

/// Xiaohongshu exposes declarations through a single visible d-select. The
/// bounded scripts act only on an exact resolved label and retain all page
/// content inside the attached browser.
pub(crate) const XIAOHONGSHU_STATEMENT_OPEN_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),matches=Array.from(document.querySelectorAll('.d-select-placeholder')).filter(item=>visible(item)&&norm(item.textContent)==='添加内容类型声明').map(item=>item.closest('.d-select')||item.parentElement).filter(Boolean);if(matches.length!==1)return false;const trigger=matches[0];trigger.setAttribute('data-matrixpost-xhs-statement','true');trigger.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true,view:window}));trigger.click();return true;"#;
pub(crate) const XIAOHONGSHU_STATEMENT_LIST_VISIBLE_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=root=>Array.from(root.querySelectorAll('.d-option-name')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),roots=Array.from(document.querySelectorAll('.d-options-wrapper')).filter(visible),matches=roots.filter(root=>options(root).length===1);return matches.length===1;"#;
pub(crate) const XIAOHONGSHU_STATEMENT_SELECT_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),options=root=>Array.from(root.querySelectorAll('.d-option-name')).filter(item=>visible(item)&&norm(item.textContent)===norm(expected)),roots=Array.from(document.querySelectorAll('.d-options-wrapper')).filter(visible),matches=roots.filter(root=>options(root).length===1);if(matches.length!==1)return false;const option=options(matches[0])[0],row=option.closest('.d-grid-item'),grid=row?.parentElement,area=row?.getAttribute('style')||'',number=area.match(/grid-area:\s*(\d+)/)?.[1],handlers=number&&grid?Array.from(grid.querySelectorAll('.d-grid-item')).filter(item=>(item.getAttribute('style')||'').match(/grid-area:\s*(\d+)/)?.[1]===number).map(item=>item.querySelector('.d-option-handler')).filter(Boolean):[],action=handlers.length===1?handlers[0]:option.closest('.d-option')||row,disabled=item=>item?.disabled||item?.getAttribute('aria-disabled')==='true'||/(^|\s)(?:is-)?disabled(?:\s|$)/.test(String(item?.className||''));if(!action||disabled(action)||disabled(row))return false;action.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true,view:window}));action.click();return true;"#;
pub(crate) const XIAOHONGSHU_STATEMENT_APPLIED_SCRIPT: &str = r#"const expected=arguments[0],visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},norm=value=>String(value||'').replace(/\s+/g,'').trim(),matches=Array.from(document.querySelectorAll('[data-matrixpost-xhs-statement="true"]')).filter(visible);if(matches.length!==1)return'pending';const root=matches[0],description=norm(root.querySelector('.d-select-description')?.textContent),placeholder=norm(root.querySelector('.d-select-placeholder')?.textContent),open=Array.from(document.querySelectorAll('.d-options-wrapper')).some(visible);if(open)return'open';if(description===norm(expected))return'description';if(placeholder==='添加内容类型声明')return'prompt';return placeholder===norm(expected)?'placeholder':'pending';"#;
/// MatrixMedia closes the optional PK-cover switch before publishing. The
/// runner normalizes only one visible checkbox and returns finite states, so
/// unrelated page state never leaves the attached browser.
pub(crate) const XIAOHONGSHU_PK_COVER_STATE_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},wrappers=Array.from(document.querySelectorAll('.pk-cover-title-wrapper')).filter(visible);if(wrappers.length===0)return'absent';if(wrappers.length!==1)return'ambiguous';const inputs=Array.from(wrappers[0].querySelectorAll('input[type="checkbox"]'));if(inputs.length===0)return'invalid';if(inputs.length!==1)return'ambiguous';return inputs[0].checked?'checked':'unchecked';"#;
pub(crate) const XIAOHONGSHU_PK_COVER_CLOSE_SCRIPT: &str = r#"const visible=item=>{const style=getComputedStyle(item);const rect=item.getBoundingClientRect();return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0&&rect.width>0&&rect.height>0;},wrappers=Array.from(document.querySelectorAll('.pk-cover-title-wrapper')).filter(visible);if(wrappers.length!==1)return false;const inputs=Array.from(wrappers[0].querySelectorAll('input[type="checkbox"]'));if(inputs.length!==1||!inputs[0].checked)return false;const input=inputs[0];input.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,cancelable:true,view:window}));input.dispatchEvent(new MouseEvent('mouseup',{bubbles:true,cancelable:true,view:window}));input.click();return true;"#;

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

/// Xiaohongshu accepts only AI, fictional, and marketing declarations. The
/// upstream deliberately leaves `none`, unsupported, and unknown values
/// unchanged, so there is no page action for them here.
pub(crate) fn xiaohongshu_creative_statement_label(
    request: &PublishRequest,
) -> Option<&'static str> {
    let value = request
        .overrides
        .iter()
        .find(|item| item.platform == Platform::Xiaohongshu)
        .and_then(|item| item.creative_statement.as_deref())?
        .trim();
    match value {
        "ai_generated"
        | "AI生成"
        | "含AI生成内容"
        | "内容由AI生成"
        | "内容为AI生成"
        | "笔记含AI合成内容" => Some("笔记含AI合成内容"),
        "fiction"
        | "虚构演绎"
        | "含虚构演绎内容"
        | "虚构演绎，仅供娱乐"
        | "演绎情节，仅供娱乐"
        | "虚构演绎，故事经历"
        | "内容为虚构剧情，仅供娱乐" => Some("虚构演绎，仅供娱乐"),
        "marketing" | "营销推广" | "内容含营销信息" | "内容含营销推广信息" | "内容包含营销广告" => {
            Some("内容包含营销广告")
        }
        _ => None,
    }
}

/// Resolve only an explicit WeChat target override; an unsupported value has
/// no matching local page action.
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

pub(crate) fn normalize_review_title_query(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct AcknowledgementPolicy {
    pub(crate) attempts: usize,
    pub(crate) interval: Duration,
}

impl AcknowledgementPolicy {
    pub(crate) const fn production() -> Self {
        Self {
            attempts: ACKNOWLEDGEMENT_ATTEMPTS,
            interval: ACKNOWLEDGEMENT_INTERVAL,
        }
    }
}

/// The UI profile is deliberately data, not per-platform automation code.
pub(crate) struct PlatformProfile {
    pub(crate) platform: Platform,
    pub(crate) upload_url: &'static str,
    pub(crate) file: &'static [&'static str],
    pub(crate) title: &'static [&'static str],
    pub(crate) short_title: Option<&'static [&'static str]>,
    pub(crate) description: &'static [&'static str],
    pub(crate) submit: &'static [&'static str],
    pub(crate) draft: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}

pub(crate) struct ArticleProfile {
    pub(crate) editor_url: &'static str,
    pub(crate) title: &'static [&'static str],
    pub(crate) content: &'static [&'static str],
    pub(crate) cover: &'static [&'static str],
    pub(crate) category: &'static [&'static str],
    pub(crate) tags: &'static [&'static str],
    pub(crate) summary: &'static [&'static str],
    pub(crate) publish_panel: &'static [&'static str],
    pub(crate) confirm: &'static [&'static str],
    pub(crate) success: &'static [&'static str],
}

pub(crate) const JUEJIN_PROFILE: ArticleProfile = ArticleProfile {
    editor_url: "https://juejin.cn/editor/drafts/new",
    title: &["input[placeholder*='标题']", "input[aria-label*='标题']"],
    content: &[
        "div.cm-content[contenteditable='true']",
        ".cm-editor div[contenteditable='true']",
    ],
    cover: &[
        "input[type='file'][accept*='image']",
        "input[data-testid='cover-upload']",
    ],
    category: &[
        "input[placeholder*='分类']",
        "button[data-testid='category']",
    ],
    tags: &[
        "input[placeholder*='标签']",
        "input[data-testid='tag-input']",
    ],
    summary: &[
        "textarea[placeholder*='摘要']",
        "input[placeholder*='摘要']",
    ],
    publish_panel: &[
        "button[data-testid='publish-article']",
        "button[class*='publish']",
    ],
    confirm: &[
        "button[data-testid='confirm-publish']",
        "button[class*='confirm']",
    ],
    success: &[
        "[data-testid='publish-success']",
        "[role='status'][data-status='success']",
    ],
};

pub(crate) const PROFILES: &[PlatformProfile] = &[
    PlatformProfile {
        platform: Platform::Douyin,
        upload_url: "https://creator.douyin.com/creator-micro/content/post/video?enter_from=publish_page",
        file: &["input[type='file']", "input.upload-input"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-e2e='publish-success']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::WechatChannels,
        upload_url: "https://channels.weixin.qq.com/platform/post/create",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: Some(&[
            "wujie-app.wujie_iframe input[placeholder='填写短标题有机会获得更多流量']",
        ]),
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-status='published']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Bilibili,
        upload_url: "https://member.bilibili.com/platform/upload/video/frame/",
        file: &["input[type='file']", "input[type='file'][accept*='video']"],
        title: &["input[placeholder*='标题']", "input[aria-label*='标题']"],
        short_title: None,
        description: &[
            "textarea[placeholder*='简介']",
            "div[contenteditable='true']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".success-wrap", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Baijiahao,
        upload_url: "https://baijiahao.baidu.com/builder/rc/edit?type=videoV2&is_from_cms=1",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
        short_title: None,
        description: &[
            "textarea[placeholder*='摘要']",
            "div[contenteditable='true']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &["[data-status='published']", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Toutiao,
        upload_url: "https://mp.toutiao.com/profile_v4/xigua/upload-video",
        file: &["input[type='file']", "input.upload-file"],
        title: &["input[placeholder*='标题']", "input[name='title']"],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='简介']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
    PlatformProfile {
        platform: Platform::Kuaishou,
        upload_url: "https://cp.kuaishou.com/article/publish/video?tabType=1",
        file: &["input[type='file']", "input.upload-file"],
        title: &["#work-description-edit", "input[placeholder*='标题']"],
        short_title: None,
        description: &["#work-description-edit", "textarea[placeholder*='描述']"],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-result-success", ".publish-success"],
    },
    PlatformProfile {
        platform: Platform::Xiaohongshu,
        upload_url: "https://creator.xiaohongshu.com/publish/publish?from=menu&target=video",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='填写标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='正文']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
    PlatformProfile {
        platform: Platform::FanqieVideo,
        upload_url: "https://pugc.yueduwuxian.com/fqvideo/home/publish-video",
        file: &["input[type='file']", "input.upload-file"],
        title: &[
            "input[placeholder*='标题']",
            "textarea[placeholder*='标题']",
        ],
        short_title: None,
        description: &[
            "div[contenteditable='true']",
            "textarea[placeholder*='描述']",
        ],
        submit: &["button[type='submit']", "button.publish-button"],
        draft: &["button[data-action='draft']", "button.draft-button"],
        success: &[".publish-success", "[data-status='published']"],
    },
];

pub(crate) fn profile(platform: Platform) -> Option<&'static PlatformProfile> {
    PROFILES.iter().find(|profile| profile.platform == platform)
}

pub(crate) fn sensitive(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "cookie",
        "token",
        "password",
        "secret",
        "session",
        "authorization",
        "credential",
        "profile",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn local_webdriver_endpoint(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "WebDriver endpoint must be an absolute URL".to_owned())?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("WebDriver endpoint must be credential-free loopback HTTP".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if loopback && !sensitive(url.path()) {
        Ok(url)
    } else {
        Err("WebDriver endpoint must be credential-free loopback HTTP".into())
    }
}
