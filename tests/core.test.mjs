import test from 'node:test';
import assert from 'node:assert/strict';
import { Engine } from '../extension/core/engine.js';
import { MemoryStore } from '../extension/core/store.js';
import { MemorySession,Vault } from '../extension/core/vault.js';
import { normalizeDraft,safeJob } from '../extension/core/model.js';

const password='local-test-vault-passphrase';
const platforms=[{id:'testshop',name:'Test shop',kind:'product',mode:'api',home:'https://shop.example.com',composeUrl:'https://shop.example.com/new',credentialFields:[{key:'accessToken',label:'Token'}]},
  {id:'testvideo',name:'Test video',kind:'video',mode:'assisted',home:'https://creator.example.com',composeUrl:'https://creator.example.com/upload',credentialFields:[]}];
const draftInput={kind:'product',title:'演示商品',description:'A useful item',product:{price:'12.50',currency:'USD',quantity:2,sku:'MX-1',images:[]}};
async function fixture(executeStep) {
  const store=new MemoryStore(),session=new MemorySession();let calls=0;
  const adapter={validateTarget:()=>[],requiredOrigins:()=>['https://shop.example.com/*'],
    canResumeAfterInterruption:(p,cp)=>cp.resumable===true,
    executeStep:async options=>{calls++;return executeStep?executeStep(options):{done:true,checkpoint:{remoteId:'123'},result:{remoteId:'123',url:'https://shop.example.com/item/123',status:'published'}};}};
  const engine=new Engine({store,session,platforms,adapter,permissions:async()=>true,openTab:async url=>({tabId:99,url})});
  await engine.dispatch('vault.setup',{passphrase:password});
  const account=await engine.dispatch('accounts.save',{account:{platform:'testshop',label:'Example shop'},credentials:{accessToken:'fixture-token-never-export'}});
  const draft=await engine.dispatch('drafts.save',{draft:draftInput});
  return {engine,store,session,account,draft,adapter,calls:()=>calls};
}

test('vault encrypts credentials at rest and binds ciphertext to its account',async()=>{
  const store=new MemoryStore(),session=new MemorySession(),vault=new Vault(store,session);
  await vault.setup(password);
  const envelope=await vault.seal({accessToken:'hidden-value'},'a');
  assert(!JSON.stringify(envelope).includes('hidden-value'));
  assert.deepEqual(await vault.open(envelope,'a'),{accessToken:'hidden-value'});
  await assert.rejects(vault.open(envelope,'b'),{code:'VAULT_DECRYPT_FAILED'});
  await vault.lock();await assert.rejects(vault.open(envelope,'a'),{code:'VAULT_LOCKED'});
  await assert.rejects(vault.unlock('wrong-but-long-password'),{code:'VAULT_UNLOCK_FAILED'});
  await vault.unlock(password);assert.equal((await vault.open(envelope,'a')).accessToken,'hidden-value');
  const restarted=new Vault(store,new MemorySession());assert.deepEqual(await restarted.status(),{configured:true,unlocked:false});
  assert(!JSON.stringify(await store.all('settings')).includes(password));
});

test('concurrent draft updates reject stale revisions and unchanged saves preserve identity',async()=>{
  const {engine,draft}=await fixture();
  const same=await engine.dispatch('drafts.save',{draft});assert.equal(same.revision,draft.revision);
  const changed=await engine.dispatch('drafts.save',{draft:{...draft,title:'Changed'}});assert.equal(changed.revision,2);
  await assert.rejects(engine.dispatch('drafts.save',{draft:{...draft,title:'Stale'}}),{code:'REVISION_CONFLICT'});
});

test('repeated and concurrent enqueue create one immutable job per account and draft version',async()=>{
  const {engine,draft,account,store}=await fixture();
  const params={draftId:draft.id,accountIds:[account.id],action:'publish',revision:1};
  const [a,b]=await Promise.all([engine.dispatch('jobs.enqueue',params),engine.dispatch('jobs.enqueue',params)]);
  assert.equal(a[0].id,b[0].id);assert.equal((await store.all('jobs')).length,1);
  await engine.dispatch('drafts.save',{draft:{...draft,title:'Second version'}});
  assert.equal((await store.all('jobs'))[0].content.title,draft.title);
  await assert.rejects(engine.dispatch('jobs.enqueue',params),{code:'REVISION_CONFLICT'});
});

test('AI public publish is held for UI approval; AI cannot self approve or access vault',async()=>{
  const {engine,draft,account,calls}=await fixture();
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'publish'},'agent');
  assert.equal(job.state,'awaiting_approval');await engine.tick();assert.equal(calls(),0);
  await assert.rejects(engine.dispatch('jobs.approve',{id:job.id},'agent'),{code:'METHOD_DENIED'});
  await assert.rejects(engine.dispatch('vault.unlock',{passphrase:password},'agent'),{code:'METHOD_DENIED'});
  await engine.dispatch('jobs.approve',{id:job.id});await engine.tick();assert.equal(calls(),1);
  const [done]=await engine.dispatch('jobs.list');assert.equal(done.state,'succeeded');assert.equal(done.result.remoteId,'123');
});

test('AI draft action stays non-public, public retries require new local approval',async()=>{
  let fail=true;
  const {engine,account,draft}=await fixture(async o=>{assert.equal(o.action,'publish');if(fail) throw Object.assign(new Error('Temporary failure'),{code:'HTTP_429',retryable:true});return {done:true,checkpoint:{},result:{status:'published'}};});
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'publish'});
  await engine.tick();fail=false;
  assert.equal((await engine.dispatch('jobs.retry',{id:job.id},'agent')).state,'awaiting_approval');
});

test('API credentials and upload capabilities never appear in snapshot, export, or AI jobs',async()=>{
  const {engine,store,account,draft}=await fixture();
  const [j]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'draft'});
  const job=await store.get('jobs',j.id);job.checkpoint={sessionUrl:'https://upload.example.com/?secret=upload-capability',remoteId:'known-remote'};await store.put('jobs',job);
  for(const result of [await engine.dispatch('snapshot'),await engine.dispatch('backup.export'),await engine.dispatch('jobs.list',{},'agent')]) {
    const data=JSON.stringify(result);assert(!data.includes('fixture-token-never-export'));assert(!data.includes('upload-capability'));assert(!data.includes('matrixpostKey'));
  }
  assert.equal(safeJob(job).remoteId,'known-remote');
});

test('unknown remote outcome is preserved and cannot be blindly retried',async()=>{
  const {engine,account,draft,calls}=await fixture(async()=>{throw Object.assign(new Error('Check remote platform'),{code:'NETWORK_UNKNOWN',uncertain:true});});
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'publish'});
  await engine.tick();assert.equal((await engine.dispatch('jobs.list'))[0].state,'uncertain');
  await assert.rejects(engine.dispatch('jobs.retry',{id:job.id}),{code:'OUTCOME_UNCERTAIN'});await engine.tick();assert.equal(calls(),1);
});

test('worker interruption recovers only explicitly resumable sessions',async()=>{
  const {engine,store,session,adapter,account,draft}=await fixture();
  const [j]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'draft'});
  let job=await store.get('jobs',j.id);job.state='running';job.inFlight={instance:'dead-worker'};await store.put('jobs',job);
  const restored=new Engine({store,session,platforms,adapter});await restored.recover();
  assert.equal((await store.get('jobs',j.id)).state,'uncertain');
  job=await store.get('jobs',j.id);job.state='running';job.checkpoint={resumable:true,offset:123};job.inFlight={instance:'dead-worker'};await store.put('jobs',job);
  await restored.recover();assert.equal((await store.get('jobs',j.id)).state,'queued');
});

test('file asset references survive tasks and cannot be deleted while referenced',async()=>{
  const {engine,store}=await fixture();
  await store.put('assets',{id:'video-file',name:'a.mp4',type:'video/mp4',size:3,blob:new Blob(['abc'],{type:'video/mp4'})});
  const draft=await engine.dispatch('drafts.save',{draft:{kind:'video',title:'Video',assetIds:['video-file']}});
  await assert.rejects(engine.dispatch('assets.delete',{id:'video-file'}),{code:'ASSET_IN_USE'});
  await engine.dispatch('drafts.delete',{id:draft.id});assert.deepEqual(await engine.dispatch('assets.delete',{id:'video-file'}),{deleted:true});
});

test('assisted platforms prepare content without reporting remote publication',async()=>{
  const {engine,calls}=await fixture();
  const account=await engine.dispatch('accounts.save',{account:{platform:'testvideo',label:'Current profile'}});
  const draft=await engine.dispatch('drafts.save',{draft:{kind:'video',title:'Hello',description:'World'}});
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'publish'});
  assert.equal(job.state,'prepared');assert.equal(job.result.status,'prepared');await engine.tick();assert.equal(calls(),0);
  const prepared=await engine.dispatch('assisted.prepare',{draftId:draft.id,accountId:account.id});assert.equal(prepared.tabId,99);assert(prepared.text.includes('Hello'));
});

test('locking vault blocks dispatch and credential renewal preserves queued identity',async()=>{
  const {engine,account,draft,calls}=await fixture();
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'draft'});
  await engine.dispatch('vault.lock');await engine.tick();assert.equal(calls(),0);assert.equal((await engine.dispatch('jobs.list'))[0].state,'blocked');
  await engine.dispatch('vault.unlock',{passphrase:password});
  const updated=await engine.dispatch('accounts.save',{account:{...account,label:'Renewed'},credentials:{accessToken:'renewed-fixture'}});
  assert.equal(updated.revision,account.revision);await engine.dispatch('jobs.retry',{id:job.id});await engine.tick();assert.equal(calls(),1);
});

test('scheduler waits until due and cancellation prevents dispatch',async()=>{
  const {engine,account,draft,calls}=await fixture();
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'draft',scheduledAt:new Date(Date.now()+3600000).toISOString()});
  await engine.tick();assert.equal(calls(),0);await engine.dispatch('jobs.cancel',{id:job.id});await engine.tick();assert.equal(calls(),0);
});

test('a multi-step task checkpoints between calls and only completes after remote result',async()=>{
  const {engine,store,account,draft}=await fixture(async({checkpoint})=>checkpoint.stage?{done:true,checkpoint:{...checkpoint,remoteId:'product'},result:{status:'draft',remoteId:'product'}}:{done:false,checkpoint:{stage:'created',remoteId:'product'}});
  const [job]=await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'draft'});
  await engine.tick();assert.equal((await store.get('jobs',job.id)).state,'queued');assert.equal((await engine.dispatch('jobs.list'))[0].remoteId,'product');
  await engine.tick();assert.equal((await store.get('jobs',job.id)).state,'succeeded');
});

test('backup import is additive, resets media/credentials and never restores execution',async()=>{
  const {engine,account,draft,store}=await fixture();await engine.dispatch('jobs.enqueue',{draftId:draft.id,accountIds:[account.id],action:'publish'});
  const backup=await engine.dispatch('backup.export');await engine.dispatch('backup.import',{data:backup});
  assert.equal((await store.all('drafts')).length,2);assert.equal((await store.all('jobs')).length,1);
  const imported=(await store.all('accounts')).find(a=>a.id!==account.id);assert.equal(imported.hasCredentials,false);
  const invalid={...backup,drafts:[draft,{...draft,kind:'invalid'}]};await assert.rejects(engine.dispatch('backup.import',{data:invalid}));
  assert.equal((await store.all('drafts')).length,2);
});

test('model validates money, links, quantities, and preserves data as inert text',()=>{
  const d=normalizeDraft({...draftInput,title:'<script>bad()</script>'});assert.equal(d.title,'<script>bad()</script>');
  assert.throws(()=>normalizeDraft({...draftInput,product:{price:'NaN'}}),{code:'INVALID_PRICE'});
  assert.throws(()=>normalizeDraft({...draftInput,product:{quantity:-1}}),{code:'INVALID_QUANTITY'});
  assert.throws(()=>normalizeDraft({...draftInput,product:{images:['javascript:bad()']}}),{code:'INVALID_URL'});
});
