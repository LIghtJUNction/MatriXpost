import { assert, AppError } from './errors.js';

export const VERSION='0.6.0';
export const MAX_BATCH=50;
export const ACTIVE_STATES=['queued','running','awaiting_approval','blocked'];
export const uuid=()=>crypto.randomUUID();
export const now=()=>new Date().toISOString();

export function plain(value,name='参数') {
  assert(value && typeof value==='object' && !Array.isArray(value) &&
    (Object.getPrototypeOf(value)===Object.prototype || Object.getPrototypeOf(value)===null),
  'INVALID_INPUT',`${name}必须是 JSON 对象。`);
  return value;
}
export function text(value,name,max=2000,required=false) {
  if(value===undefined || value===null) value='';
  assert(typeof value==='string','INVALID_INPUT',`${name}必须是文字。`);
  const result=value.trim();
  assert(result.length<=max && (!required || result.length>0),'INVALID_INPUT',`${name}需为${required?' 1–':'最多 '}${max}个字符。`);
  return result;
}
export function id(value) {
  assert(typeof value==='string' && /^[a-zA-Z0-9_-]{1,100}$/.test(value),'INVALID_ID','记录 ID 无效。');
  return value;
}
export function strings(value,name,limit=100,maxLength=500) {
  if(value===undefined) return [];
  assert(Array.isArray(value) && value.length<=limit,'INVALID_INPUT',`${name}超出数量限制。`);
  return [...new Set(value.map(x=>text(x,name,maxLength,true)))];
}
export function httpsUrl(value,name='URL') {
  try {
    const u=new URL(value);
    assert(u.protocol==='https:' && !u.username && !u.password && !u.hash,'INVALID_URL',`${name}需要不含密码的 HTTPS 地址。`);
    return u.href;
  } catch(e) { if(e.code) throw e; throw new AppError('INVALID_URL',`${name}地址无效。`); }
}
export function normalizeDraft(input,previous) {
  plain(input,'草稿');
  assert(['video','product'].includes(input.kind),'INVALID_KIND','选择视频或商品。');
  const video=plain(input.video??{},'视频设置');
  const product=plain(input.product??{},'商品设置');
  const privacy=video.privacy??'private';
  assert(['private','unlisted','public'].includes(privacy),'INVALID_PRIVACY','视频可见性无效。');
  const price=text(String(product.price??''),'价格',24);
  assert(!price || /^(0|[1-9]\d{0,9})(\.\d{1,2})?$/.test(price),'INVALID_PRICE','价格使用非负十进制数字，最多两位小数。');
  const quantity=Number(product.quantity??0);
  assert(Number.isSafeInteger(quantity)&&quantity>=0,'INVALID_QUANTITY','库存应为非负整数。');
  const currency=text(product.currency??'USD','币种',3).toUpperCase();
  assert(/^[A-Z]{3}$/.test(currency),'INVALID_CURRENCY','币种使用三个大写字母。');
  const result={
    id:previous?.id??(input.id?id(input.id):uuid()), revision:(previous?.revision??0)+1,
    kind:input.kind,title:text(input.title,'标题',1000),description:text(input.description,'描述',50000),
    tags:strings(input.tags,'标签',100,100),assetIds:strings(input.assetIds,'素材',20,100).map(id),
    video:{privacy,categoryId:text(video.categoryId??'22','视频分类',20),madeForKids:video.madeForKids===true},
    product:{price,currency,quantity,sku:text(product.sku,'SKU',200),images:strings(product.images,'图片',20,2048).map(v=>httpsUrl(v,'商品图片')),
      vendor:text(product.vendor,'品牌',200),productType:text(product.productType,'商品类型',200),
      publicationId:text(product.publicationId,'渠道 ID',200),locationId:text(product.locationId,'库存位置',200),trackInventory:product.trackInventory===true},
    createdAt:previous?.createdAt??now(),updatedAt:now()
  };
  if(previous) {
    const keys=['kind','title','description','tags','assetIds','video','product'];
    if(keys.every(key=>JSON.stringify(previous[key])===JSON.stringify(result[key]))) return structuredClone(previous);
  }
  return result;
}
export function safeAccount(account) {
  const {id,platform,label,endpoint,remoteAccountId,mode,hasCredentials,createdAt,updatedAt,revision}=account;
  return {id,platform,label,endpoint,remoteAccountId,mode,hasCredentials,createdAt,updatedAt,revision};
}
export function safeJob(job) {
  // Resumable upload URLs may contain signed session capabilities. Keep them internal.
  const {checkpoint,inFlight,accountSnapshot,content,...safe}=job;
  const cp=checkpoint??{};
  return {...safe,progress:cp.total?Math.min(100,Math.floor((cp.offset??0)*100/cp.total)):job.state==='succeeded'?100:0,
    stage:cp.stage??'',remoteId:job.result?.remoteId??cp.remoteId??cp.productId??null,
    canRetry:['failed','blocked'].includes(job.state)&&job.error?.code!=='ACCOUNT_CHANGED',
    ...(job.state==='awaiting_approval'&&content?{preview:{title:content.title,description:content.description,
      tags:content.tags,video:content.video,product:content.product,assetIds:content.assetIds}}:{})};
}
export function safeAsset(asset) {
  const {id,name,size,type,createdAt}=asset;
  return {id,name,size,type,createdAt};
}
export function jobKey(draft,account,action) { return `${draft.id}:${draft.revision}:${account.id}:${action}`; }
export function parseSchedule(value) {
  if(!value) return now();
  assert(typeof value==='string' && Number.isFinite(Date.parse(value)),'INVALID_SCHEDULE','定时时间无效。');
  const n=Date.parse(value);
  assert(n<=Date.now()+366*24*3600*1000,'INVALID_SCHEDULE','定时时间需在一年以内。');
  return new Date(Math.max(n,Date.now())).toISOString();
}
