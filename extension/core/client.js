import { IndexedStore } from './store.js';
import { AppError } from './errors.js';
import { safeAsset,uuid,now } from './model.js';

export async function rpc(method,params={},source='ui') {
  if(!globalThis.chrome?.runtime?.id) throw new AppError('EXTENSION_REQUIRED','请在浏览器扩展中打开 MatriXpost。');
  const response=await chrome.runtime.sendMessage({channel:'matrixpost',method,params,source});
  if(!response?.ok) throw Object.assign(new Error(response?.error?.message??'扩展服务未响应。'),response?.error??{code:'NO_RESPONSE'});
  return response.result;
}

const store=new IndexedStore();
export async function uploadAsset(file) {
  if(!(file instanceof Blob)||!file.size) throw new AppError('INVALID_FILE','请选择非空文件。');
  if(!file.type.startsWith('video/')&&!file.type.startsWith('image/')) throw new AppError('INVALID_FILE_TYPE','请选择视频或图片。');
  const estimate=await navigator.storage?.estimate?.();
  if(estimate?.quota&&estimate?.usage+file.size>estimate.quota*0.95) {
    throw new AppError('STORAGE_FULL','浏览器可用空间不足，请先清理未使用素材或允许更大的本地存储空间。');
  }
  const asset={id:uuid(),name:file.name??'media',size:file.size,type:file.type,createdAt:now(),blob:file};
  try { await store.put('assets',asset); }
  catch(e) { if(e.name==='QuotaExceededError') throw new AppError('STORAGE_FULL','本地素材空间不足。'); throw e; }
  return safeAsset(asset);
}

let bridge=null;
const update=detail=>globalThis.dispatchEvent(new CustomEvent('matrixpost:bridge',{detail}));
export function disconnectBridge() {
  const previous=bridge;bridge=null;
  if(previous) {clearTimeout(previous.timer);previous.controller.abort();previous.token='';}
  update({connected:false});return {connected:false};
}
export async function connectBridge({url='http://127.0.0.1:43176',token}) {
  const u=new URL(url);
  if(u.protocol!=='http:'||u.hostname!=='127.0.0.1'||u.username||u.password||u.search||u.hash||!['','/'].includes(u.pathname)) {
    throw new AppError('INVALID_BRIDGE_URL','AI 桥接仅支持本机 http://127.0.0.1:端口。');
  }
  if(typeof token!=='string'||token.length<32||token.length>256||/[\s\x00-\x1f]/.test(token)) {
    throw new AppError('INVALID_BRIDGE_TOKEN','配对码无效。');
  }
  // Called directly from the Connect button so the permission request retains the user gesture.
  const allowed=await chrome.permissions.request({origins:['http://127.0.0.1/*']});
  if(!allowed) throw new AppError('HOST_PERMISSION_REQUIRED','需要允许连接本机 AI 桥接。');
  disconnectBridge();
  const state={url:u.origin,token,controller:new AbortController(),timer:null};
  const headers={Authorization:`Bearer ${token}`,'Content-Type':'application/json'};
  const response=await fetch(state.url+'/health',{headers,signal:AbortSignal.timeout(5000),credentials:'omit',redirect:'error'});
  if(!response.ok) throw new AppError('BRIDGE_CONNECT_FAILED','桥接连接失败，请检查配对码和扩展 ID。');
  bridge=state;
  const poll=async()=>{
    if(bridge!==state) return;
    let next=700;
    try {
      const response=await fetch(state.url+'/v1/poll',{headers,credentials:'omit',redirect:'error',
        signal:AbortSignal.any([state.controller.signal,AbortSignal.timeout(5000)])});
      if(!response.ok) throw new Error('poll failed');
      const data=await response.json();
      for(const request of data.requests??[]) {
        if(bridge!==state) break;
        let result,error;
        try {
          const deadline=typeof request.expiresAt==='number'?request.expiresAt:Date.parse(request.expiresAt);
          if(!Number.isFinite(deadline)||deadline<=Date.now()) throw new AppError('REQUEST_EXPIRED','AI 请求已过期。');
          result=await rpc(request.method,request.params??{},'agent');
        } catch(e) { error={code:e.code??'REQUEST_FAILED',message:e.code?e.message:'AI 请求未完成。'}; }
        await fetch(state.url+'/v1/results',{method:'POST',headers,body:JSON.stringify({id:request.id,...(error?{error}:{result})}),
          credentials:'omit',redirect:'error',signal:AbortSignal.any([state.controller.signal,AbortSignal.timeout(5000)])});
      }
      update({connected:true,url:state.url});
    } catch {
      if(bridge!==state) return;
      next=5000;update({connected:false,url:state.url,error:'连接中断，正在重连。'});
    }
    if(bridge===state) state.timer=setTimeout(poll,next);
  };
  state.timer=setTimeout(poll,0);update({connected:true,url:state.url});
  return {connected:true,url:state.url};
}
globalThis.addEventListener?.('pagehide',()=>disconnectBridge());
