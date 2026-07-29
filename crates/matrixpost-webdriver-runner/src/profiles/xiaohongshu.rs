use matrixpost_core::{Platform, PublishRequest};

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
