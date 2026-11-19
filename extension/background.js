import { Engine } from './core/engine.js';
import { IndexedStore } from './core/store.js';
import { publicError } from './core/errors.js';
import { preparePage } from './core/assisted.js';

const engine=new Engine({store:new IndexedStore(),session:chrome.storage.session,
  permissions:origins=>chrome.permissions.contains({origins}),
  openTab:async url=>{const tab=await chrome.tabs.create({url});return {tabId:tab.id,url};},preparePage});
const ready=(async()=>{
  await chrome.storage.session.setAccessLevel({accessLevel:'TRUSTED_CONTEXTS'});
  await chrome.storage.local.setAccessLevel({accessLevel:'TRUSTED_CONTEXTS'});
  await engine.recover();
  if(!await chrome.alarms.get('matrixpost-queue')) await chrome.alarms.create('matrixpost-queue',{periodInMinutes:0.5});
})();
let pumping=false;
async function pump() {
  if(pumping) return;pumping=true;
  try {
    await ready;
    const until=Date.now()+20000;
    for(let i=0;i<6&&Date.now()<until;i++) {
      const result=await engine.tick();
      if(result.idle||result.busy) break;
    }
  } catch { /* Individual jobs persist a bounded diagnostic; no credentials are logged. */ }
  finally {pumping=false;}
}

chrome.runtime.onMessage.addListener((request,sender,respond)=>{
  if(request?.channel!=='matrixpost') return false;
  let senderUrl;try {senderUrl=new URL(sender.url);}catch {return false;}
  if(sender.id!==chrome.runtime.id||senderUrl.protocol!=='chrome-extension:'||senderUrl.hostname!==chrome.runtime.id||senderUrl.pathname!=='/app.html') {
    respond({ok:false,error:{code:'UNTRUSTED_SENDER',message:'该页面无权操作 MatriXpost。'}});return false;
  }
  (async()=>{
    try {
      await ready;
      const result=await engine.dispatch(request.method,request.params??{},request.source==='agent'?'agent':'ui');
      respond({ok:true,result});
      if(['snapshot','jobs.enqueue','jobs.retry','jobs.approve','vault.unlock'].includes(request.method)) void pump();
    } catch(error) {respond({ok:false,error:publicError(error)});}
  })();
  return true;
});
chrome.alarms.onAlarm.addListener(alarm=>{if(alarm.name==='matrixpost-queue') void pump();});
chrome.runtime.onStartup.addListener(()=>void pump());
chrome.runtime.onInstalled.addListener(()=>void ready);
chrome.action.onClicked.addListener(async()=>{
  const url=chrome.runtime.getURL('app.html');
  const tabs=await chrome.tabs.query({url});
  if(tabs[0]) {await chrome.tabs.update(tabs[0].id,{active:true});if(tabs[0].windowId) await chrome.windows.update(tabs[0].windowId,{focused:true});}
  else await chrome.tabs.create({url});
});
