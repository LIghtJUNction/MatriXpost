import { assert } from './errors.js';

// Conservative hints, evaluated in the isolated extension world. Every matched
// field must be unique and visible; a changed platform form is reported, not guessed.
export function fillVisibleFields(fields) {
  const patterns={
    title:/^(标题|作品标题|商品名称|商品标题|视频标题|title|product name|video title)(（.*?）|\(.*?\))?$/i,
    description:/^(描述|简介|作品描述|商品描述|商品详情|视频简介|description|product description)$/i,
    price:/^(价格|销售价|售价|price|regular price)$/i,
    sku:/^(sku|商家编码|商品编码)$/i,
    quantity:/^(库存|库存数量|quantity|stock quantity)$/i
  };
  const controls=[...document.querySelectorAll('input:not([type=password]):not([type=hidden]):not([type=file]),textarea,[contenteditable=true]')]
    .filter(e=>!e.disabled&&!e.readOnly&&e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden');
  function label(el) {
    const labelledby=(el.getAttribute('aria-labelledby')??'').split(/\s+/).filter(Boolean).map(x=>document.getElementById(x)?.textContent??'').join(' ').trim();
    return [el.getAttribute('aria-label'),labelledby,...[...(el.labels??[])].map(l=>l.textContent),el.getAttribute('placeholder'),el.getAttribute('name')]
      .filter(Boolean).map(s=>s.trim().replace(/\s*[:：*]\s*$/,''));
  }
  const filled=[],missing=[],ambiguous=[],occupied=[];
  for(const [key,value] of Object.entries(fields)) {
    if(value===''||value==null) continue;
    const candidates=controls.filter(el=>label(el).some(s=>patterns[key]?.test(s)));
    if(!candidates.length) {missing.push(key);continue;}
    if(candidates.length!==1) {ambiguous.push(key);continue;}
    const el=candidates[0];
    if(String(el.isContentEditable?el.textContent:el.value).trim()) {occupied.push(key);continue;}
    if(el.isContentEditable) el.textContent=String(value);
    else {
      const proto=el instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto,'value').set.call(el,String(value));
    }
    el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));
    filled.push(key);
  }
  return {filled,missing,ambiguous,occupied,status:filled.length?'filled':'no_matching_fields',submitted:false};
}

export async function preparePage({draft,account,platform,tabId}) {
  assert(Number.isInteger(tabId),'INVALID_TAB','请先打开平台发布页面。');
  const tab=await chrome.tabs.get(tabId);
  let url;try {url=new URL(tab.url);}catch {assert(false,'INVALID_TAB','平台页面不可用。');}
  const endpoints=[platform.home,platform.composeUrl,account.endpoint].filter(Boolean).map(x=>new URL(x).hostname);
  assert(url.protocol==='https:'&&endpoints.some(host=>url.hostname===host ||
    (platform.id==='shopify'&&url.hostname==='admin.shopify.com')),
  'TAB_ORIGIN_MISMATCH','此标签页与所选账号的平台不匹配。');
  assert(await chrome.permissions.contains({origins:[url.origin+'/*']}),'HOST_PERMISSION_REQUIRED','请先允许访问该平台页面。');
  const fields={title:draft.title,description:draft.description,...(draft.kind==='product'?{
    price:draft.product.price,sku:draft.product.sku,quantity:String(draft.product.quantity)}:{})};
  const result=await chrome.scripting.executeScript({target:{tabId},func:fillVisibleFields,args:[fields]});
  assert(result[0]?.result,'FORM_UNAVAILABLE','页面尚未准备好。');return result[0].result;
}
