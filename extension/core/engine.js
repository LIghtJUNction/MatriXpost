import { AppError,assert,publicError } from './errors.js';
import { VERSION,MAX_BATCH,ACTIVE_STATES,plain,text,id,strings,httpsUrl,uuid,now,normalizeDraft,safeAccount,safeJob,safeAsset,jobKey,parseSchedule } from './model.js';
import { PLATFORMS } from './catalog.js';
import { validateTarget,requiredOrigins,executeStep,canResumeAfterInterruption,promoteDraftCheckpoint } from '../adapters/index.js';
import { Vault } from './vault.js';

export const AGENT_METHODS=new Set(['platforms.list','accounts.list','drafts.list','drafts.save','publish.preview','jobs.enqueue','jobs.list','jobs.retry','jobs.cancel']);
export const METHODS=[
  ['platforms.list','列出平台接入能力'],['accounts.list','列出账号，不返回凭证'],['drafts.list','列出本地草稿'],
  ['drafts.save','创建或更新草稿；更新传 revision'],['publish.preview','检查草稿、账号、权限和平台字段'],
  ['jobs.enqueue','按账号创建任务；AI 公开发布等待本地确认'],['jobs.list','查看任务与真实远端结果'],
  ['jobs.retry','重试已明确失败的任务'],['jobs.cancel','取消尚未开始的任务']
].map(([name,description])=>({name,description}));

export class Engine {
  constructor({store,session,platforms=PLATFORMS,adapter={validateTarget,requiredOrigins,executeStep,canResumeAfterInterruption,promoteDraftCheckpoint},permissions=async()=>true,openTab=async()=>({}),preparePage=null}) {
    this.store=store; this.vault=new Vault(store,session); this.platforms=platforms; this.adapter=adapter;
    this.permissions=permissions; this.openTab=openTab; this.preparePage=preparePage;
    this.tail=Promise.resolve(); this.ticking=false; this.instance=uuid();
  }
  platform(value) {
    const p=this.platforms.find(p=>p.id===value);
    assert(p,'PLATFORM_NOT_FOUND','平台不存在。'); return p;
  }
  exclusive(fn) {
    const next=this.tail.then(fn,fn);
    this.tail=next.catch(()=>{}); return next;
  }
  async need(store,value) {
    const record=await this.store.get(store,id(value));
    assert(record,'NOT_FOUND','记录不存在或已经删除。'); return record;
  }
  async dispatch(method,params={},source='ui') {
    assert(typeof method==='string','INVALID_METHOD','缺少操作名称。');
    plain(params);
    assert(JSON.stringify(params).length<=1048576,'PAYLOAD_TOO_LARGE','请求超过 1 MiB。');
    if(source==='agent') assert(AGENT_METHODS.has(method),'METHOD_DENIED','AI 接口未开放此操作。');
    if(method==='jobs.run') return this.tick();
    return this.exclusive(()=>this.handle(method,params,source));
  }
  async handle(method,p,source) {
    switch(method) {
      case 'snapshot': return this.snapshot();
      case 'methods.list': return METHODS;
      case 'platforms.list': return this.platforms;
      case 'accounts.list': return (await this.store.all('accounts')).map(safeAccount);
      case 'drafts.list': return (await this.store.all('drafts')).sort((a,b)=>b.updatedAt.localeCompare(a.updatedAt));
      case 'jobs.list': return (await this.store.all('jobs')).map(safeJob).sort((a,b)=>b.createdAt.localeCompare(a.createdAt));
      case 'drafts.save': return this.saveDraft(p.draft);
      case 'drafts.delete': return this.deleteDraft(p.id);
      case 'accounts.save': return this.saveAccount(p.account,p.credentials);
      case 'accounts.remove': return this.removeAccount(p.id);
      case 'vault.setup': return this.vault.setup(p.passphrase);
      case 'vault.unlock': return this.vault.unlock(p.passphrase);
      case 'vault.lock': return this.vault.lock();
      case 'publish.preview': return this.preview(p);
      case 'jobs.enqueue': return this.enqueue(p,source);
      case 'jobs.approve': return this.approve(p.id);
      case 'jobs.retry': return this.retry(p.id,source);
      case 'jobs.cancel': return this.cancel(p.id);
      case 'platforms.open': return this.openPlatform(p);
      case 'assisted.prepare': return this.assisted(p);
      case 'assisted.fill': return this.fillAssisted(p);
      case 'assets.delete': return this.deleteAsset(p.id);
      case 'assets.prune': return this.pruneAssets();
      case 'settings.save': return this.saveSettings(p.settings);
      case 'backup.export': return this.export();
      case 'backup.import': return this.import(p.data);
      default: throw new AppError('METHOD_NOT_FOUND','未知操作。');
    }
  }
  async snapshot() {
    const [drafts,accounts,jobs,assets,settings,vault]=await Promise.all([
      this.store.all('drafts'),this.store.all('accounts'),this.store.all('jobs'),this.store.all('assets'),
      this.store.get('settings','preferences'),this.vault.status()
    ]);
    return {version:VERSION,platforms:this.platforms,drafts:drafts.sort((a,b)=>b.updatedAt.localeCompare(a.updatedAt)),
      accounts:accounts.map(safeAccount),jobs:jobs.map(safeJob).sort((a,b)=>b.createdAt.localeCompare(a.createdAt)),
      assets:assets.map(safeAsset),settings:settings?.value??{theme:'light'},vault};
  }
  async saveDraft(input) {
    plain(input,'草稿');
    const prev=input.id?await this.need('drafts',input.id):null;
    if(prev) assert(input.revision===prev.revision,'REVISION_CONFLICT','草稿已在另一个窗口更新，请重新载入。');
    const draft=normalizeDraft(input,prev);
    for(const aid of draft.assetIds) await this.need('assets',aid);
    await this.store.put('drafts',draft); return draft;
  }
  async deleteDraft(draftId) {
    await this.need('drafts',draftId);
    const jobs=await this.store.all('jobs');
    assert(!jobs.some(j=>j.draftId===draftId&&ACTIVE_STATES.includes(j.state)),'DRAFT_IN_USE','请先取消该草稿的待执行任务。');
    await this.store.delete('drafts',draftId); return {deleted:true};
  }
  async saveAccount(input,credentials) {
    plain(input,'账号');
    const platform=this.platform(input.platform);
    const prev=input.id?await this.need('accounts',input.id):null;
    const mode=input.mode==='browser'?'browser':platform.mode==='api'?'api':'browser';
    assert(!(platform.mode==='planned'&&mode==='api'),'PLATFORM_UNAVAILABLE','该平台直连仍在接入中。');
    let endpoint=text(input.endpoint,'店铺地址',2048);
    if(endpoint) endpoint=httpsUrl(endpoint,'店铺地址').replace(/\/$/,'');
    const account={id:prev?.id??uuid(),platform:platform.id,label:text(input.label,'账号名称',120,true),
      endpoint,remoteAccountId:text(input.remoteAccountId,'平台账号 ID',200),mode,
      hasCredentials:prev?.hasCredentials??false,revision:prev?.revision??1,createdAt:prev?.createdAt??now(),updatedAt:now()};
    if(prev) {
      assert(prev.platform===account.platform&&prev.endpoint===account.endpoint&&prev.mode===account.mode&&prev.remoteAccountId===account.remoteAccountId,
        'ACCOUNT_IDENTITY_IMMUTABLE','更换平台、店铺地址或接入方式时，请新增账号。');
      const active=(await this.store.all('jobs')).some(j=>j.accountId===prev.id&&j.state==='running');
      assert(!active,'ACCOUNT_IN_USE','当前账号正在发送请求，请稍后再更新凭证。');
    }
    if(mode==='api') {
      const origins=this.adapter.requiredOrigins(account); // Endpoint validation is centralized in adapters.
      assert(await this.permissions(origins),'HOST_PERMISSION_REQUIRED','请允许访问该账号的 API 地址。',{origins});
    }
    const ops=[];
    if(credentials!==undefined && Object.values(plain(credentials,'凭证')).some(v=>v!==''&&v!=null)) {
      assert(mode==='api','CREDENTIALS_NOT_NEEDED','浏览器会话账号不需要保存 API 凭证。');
      const filtered={};
      for(const field of platform.credentialFields??[]) {
        const key=field.key;
        filtered[key]=text(credentials[key],field.label??key,16384,true);
      }
      assert(Object.keys(filtered).length>0,'CREDENTIALS_NOT_SUPPORTED','平台没有可配置的 API 凭证。');
      const envelope=await this.vault.seal(filtered,account.id);
      ops.push({store:'secrets',value:{id:account.id,envelope}}); account.hasCredentials=true;
    }
    ops.push({store:'accounts',value:account});
    await this.store.batch(ops); return safeAccount(account);
  }
  async removeAccount(accountId) {
    await this.need('accounts',accountId);
    const jobs=await this.store.all('jobs');
    assert(!jobs.some(j=>j.accountId===accountId&&ACTIVE_STATES.includes(j.state)),'ACCOUNT_IN_USE','请先取消此账号的待执行任务。');
    await this.store.batch([{store:'accounts',delete:true,id:accountId},{store:'secrets',delete:true,id:accountId}]);
    return {deleted:true};
  }
  async preview({draftId,accountIds,action='draft'}) {
    assert(['draft','publish'].includes(action),'INVALID_ACTION','操作应为保存草稿或发布。');
    const draft=await this.need('drafts',draftId);
    const ids=strings(accountIds,'目标账号',MAX_BATCH,100);
    assert(ids.length>0,'NO_TARGETS','至少选择一个目标账号。');
    const vault=await this.vault.status(); const targets=[];
    for(const aid of ids) {
      const account=await this.need('accounts',aid); const platform=this.platform(account.platform);
      const errors=[];
      if(!draft.title) errors.push({field:'title',message:'填写标题。'});
      if(platform.kind!==draft.kind) errors.push({field:'kind',message:'账号平台与内容类型不匹配。'});
      const mode=account.mode==='api'&&platform.mode==='api'?'api':'assisted';
      if(platform.mode==='planned') errors.push({field:'platform',message:'该平台尚在接入中。'});
      if(mode==='api') {
        errors.push(...this.adapter.validateTarget(draft,account,action));
        if(!account.hasCredentials) errors.push({field:'credentials',message:'该账号尚未保存授权凭证。'});
        if(!vault.unlocked) errors.push({field:'vault',message:'请解锁账号保险库。'});
        const origins=this.adapter.requiredOrigins(account);
        if(!await this.permissions(origins)) errors.push({field:'permission',message:'需要授予平台访问权限。'});
        if(draft.kind==='video') {
          const asset=draft.assetIds.length?await this.store.get('assets',draft.assetIds[0]):null;
          if(!asset?.blob || !asset.size) errors.push({field:'assetIds',message:'选择非空本地视频文件。'});
          else if(!asset.type.startsWith('video/')) errors.push({field:'assetIds',message:'选择视频格式的素材。'});
        }
      }
      targets.push({accountId:account.id,accountRevision:account.revision,platform:platform.id,label:account.label,
        mode,valid:errors.length===0,errors,note:mode==='assisted'?'准备内容后前往平台完成发布。':'通过该账号已授权的 API 执行。'});
    }
    return {draftId:draft.id,revision:draft.revision,action,targets,valid:targets.every(t=>t.valid)};
  }
  async enqueue(params,source) {
    const preview=await this.preview(params);
    assert(preview.valid,'VALIDATION_FAILED','部分目标尚未准备好。',preview.targets);
    const draft=await this.need('drafts',params.draftId);
    if(params.revision!==undefined) assert(params.revision===draft.revision,'REVISION_CONFLICT','草稿已变化，请重新预览。');
    const scheduledAt=parseSchedule(params.scheduledAt); const previous=await this.store.all('jobs');
    const jobs=[]; const ops=[];
    for(const target of preview.targets) {
      const account=await this.need('accounts',target.accountId);
      const key=jobKey(draft,account,preview.action);
      const duplicate=previous.find(j=>j.key===key&&j.state!=='cancelled');
      if(duplicate) { jobs.push(safeJob(duplicate)); continue; }
      const other=previous.find(j=>j.draftId===draft.id&&j.draftRevision===draft.revision&&j.accountId===account.id&&
        j.action!==preview.action&&['queued','running','blocked','awaiting_approval','failed','uncertain'].includes(j.state));
      assert(!other,'TARGET_BUSY','该内容在此账号还有未结束的任务，请先完成或取消原任务。');
      assert(!(preview.action==='draft'&&previous.some(j=>j.draftId===draft.id&&j.draftRevision===draft.revision&&
        j.accountId===account.id&&j.action==='publish'&&j.state==='succeeded')),'ALREADY_PUBLISHED','该版本已经提交发布。修改内容后可创建新版本草稿。');
      const state=target.mode==='assisted'?'prepared':source==='agent'&&preview.action==='publish'?'awaiting_approval':'queued';
      const savedRemoteDraft=preview.action==='publish'&&target.mode==='api'?previous.find(j=>j.draftId===draft.id&&
        j.draftRevision===draft.revision&&j.accountId===account.id&&j.action==='draft'&&j.state==='succeeded'):null;
      const checkpoint=savedRemoteDraft?this.adapter.promoteDraftCheckpoint(account.platform,savedRemoteDraft.checkpoint):{};
      const job={id:uuid(),key,draftId:draft.id,draftRevision:draft.revision,accountId:account.id,
        accountRevision:account.revision,accountLabel:account.label,platform:account.platform,title:draft.title,kind:draft.kind,
        action:preview.action,mode:target.mode,state,source,scheduledAt,createdAt:now(),updatedAt:now(),attempts:0,
        content:structuredClone(draft),accountSnapshot:safeAccount(account),checkpoint,error:null,result:null};
      if(state==='prepared') job.result={status:'prepared',note:'内容已准备，请在平台页面完成上传和提交。'};
      ops.push({store:'jobs',value:job}); jobs.push(safeJob(job));
    }
    await this.store.batch(ops); return jobs;
  }
  async approve(jobId) {
    const job=await this.need('jobs',jobId);
    assert(job.state==='awaiting_approval','INVALID_STATE','此任务无需确认。');
    job.state='queued'; job.approvedAt=now(); job.updatedAt=now();
    await this.store.put('jobs',job); return safeJob(job);
  }
  async retry(jobId,source) {
    const job=await this.need('jobs',jobId);
    assert(['failed','blocked','uncertain'].includes(job.state),'INVALID_STATE','仅可重试失败或受阻的任务。');
    assert(job.state!=='uncertain' || this.adapter.canResumeAfterInterruption(job.platform,job.checkpoint),
      'OUTCOME_UNCERTAIN','平台可能已经接收该请求。请先到平台核对，避免重复发布。');
    assert(job.error?.code!=='ACCOUNT_CHANGED','ACCOUNT_CHANGED','目标账号已修改，请为当前草稿创建新任务。');
    if(source==='agent'&&job.action==='publish') {
      job.state='awaiting_approval'; job.approvedAt=null;
    } else job.state='queued';
    job.error=null; job.updatedAt=now(); job.scheduledAt=now(); job.inFlight=null;
    await this.store.put('jobs',job); return safeJob(job);
  }
  async cancel(jobId) {
    const job=await this.need('jobs',jobId);
    assert(['queued','blocked','awaiting_approval','prepared','failed'].includes(job.state),'INVALID_STATE',
      '当前请求已发出或已完成，请先查看平台结果。');
    job.state='cancelled'; job.updatedAt=now();
    await this.store.put('jobs',job); return safeJob(job);
  }
  async recover() {
    return this.exclusive(async()=>{
      for(const job of await this.store.all('jobs')) if(job.state==='running' && job.inFlight?.instance!==this.instance) {
        const resumable=this.adapter.canResumeAfterInterruption(job.platform,job.checkpoint);
        job.state=resumable?'queued':'uncertain'; job.updatedAt=now();
        job.error=resumable?null:{code:'INTERRUPTED',message:'浏览器在请求期间关闭，请到平台核对结果。'};
        job.inFlight=null; await this.store.put('jobs',job);
      }
    });
  }
  async tick() {
    if(this.ticking) return {busy:true};
    this.ticking=true;
    let job,account,credentials,asset;
    try {
      const ready=await this.exclusive(async()=>{
        const all=await this.store.all('jobs');
        job=all.filter(j=>j.state==='queued'&&Date.parse(j.scheduledAt)<=Date.now())
          .sort((a,b)=>a.createdAt.localeCompare(b.createdAt))[0];
        if(!job) return false;
        try {
          account=await this.need('accounts',job.accountId);
          assert(account.revision===job.accountRevision,'ACCOUNT_CHANGED','目标账号已修改，请重新创建任务。');
          const encrypted=await this.need('secrets',account.id);
          credentials=await this.vault.open(encrypted.envelope,account.id);
          assert(await this.permissions(this.adapter.requiredOrigins(account)),'HOST_PERMISSION_REQUIRED','平台访问权限已撤回。');
          asset=!job.checkpoint.promote&&job.content.assetIds[0]?(await this.need('assets',job.content.assetIds[0])).blob:null;
          job.state='running';job.inFlight={at:now(),instance:this.instance};job.attempts++;job.updatedAt=now();
          await this.store.put('jobs',job);return true;
        } catch(e) {
          job.state='blocked';job.error=publicError(e);job.updatedAt=now();await this.store.put('jobs',job);return false;
        }
      });
      if(!ready) return {idle:!job,...(job?{job:safeJob(job)}:{})};
      let result,error;
      try {
        result=await this.adapter.executeStep({draft:job.content,account,credentials,checkpoint:job.checkpoint,asset,
          action:job.action,signal:AbortSignal.timeout(25000)});
        assert(result && typeof result.done==='boolean' && result.checkpoint,'ADAPTER_INVALID_RESULT','平台返回了无法识别的任务状态。');
      } catch(e) {error=e;}
      credentials=null;
      return await this.exclusive(async()=>{
        const current=await this.need('jobs',job.id);
        if(current.state!=='running') return {job:safeJob(current)};
        current.inFlight=null;current.updatedAt=now();
        if(error) {
          current.state=error.uncertain?'uncertain':'failed';
          current.error=publicError(error);
          current.error.retryable=error.retryable===true;
        } else {
          current.checkpoint=result.checkpoint;current.error=null;
          current.state=result.done?'succeeded':'queued';
          if(result.done) { current.result=result.result??{status:'accepted'};current.completedAt=now(); }
        }
        await this.store.put('jobs',current);return {job:safeJob(current)};
      });
    } finally { credentials=null; this.ticking=false; }
  }
  async openPlatform({platform,accountId}) {
    const account=accountId?await this.need('accounts',accountId):null;
    const p=this.platform(account?.platform??platform);
    let url=p.composeUrl||p.home;
    if(account?.endpoint && p.id==='woocommerce') url=account.endpoint+'/wp-admin/post-new.php?post_type=product';
    if(account?.endpoint && p.id==='shopify') url=account.endpoint+'/admin/products/new';
    assert(url,'PLATFORM_URL_MISSING','平台入口尚未配置。');
    return this.openTab(httpsUrl(url));
  }
  async assisted({draftId,accountId}) {
    const draft=await this.need('drafts',draftId),account=await this.need('accounts',accountId);
    assert(this.platform(account.platform).kind===draft.kind,'INVALID_KIND','平台与内容类型不匹配。');
    const text=[draft.title,'',draft.description,'',draft.tags.map(x=>'#'+x).join(' '),
      ...(draft.kind==='product'?[`SKU: ${draft.product.sku}`,`价格: ${draft.product.price} ${draft.product.currency}`,`库存: ${draft.product.quantity}`]:[])].join('\n').trim();
    const opened=await this.openPlatform({accountId});
    return {...opened,text,status:'prepared',platform:account.platform,accountLabel:account.label,
      note:'使用该页面当前登录的账号。请核对账号、素材和必填项后提交。'};
  }
  async fillAssisted({draftId,accountId,tabId}) {
    assert(this.preparePage,'BROWSER_UNAVAILABLE','当前环境无法填表。');
    const draft=await this.need('drafts',draftId),account=await this.need('accounts',accountId);
    const platform=this.platform(account.platform);
    assert(platform.kind===draft.kind,'INVALID_KIND','平台与内容类型不匹配。');
    return this.preparePage({draft,account,platform,tabId});
  }
  async deleteAsset(assetId) {
    await this.need('assets',assetId);
    const drafts=await this.store.all('drafts'),jobs=await this.store.all('jobs');
    assert(!drafts.some(d=>d.assetIds.includes(assetId))&&!jobs.some(j=>!['succeeded','cancelled','prepared'].includes(j.state)&&j.content.assetIds.includes(assetId)),
      'ASSET_IN_USE','请先从草稿和待执行任务中移除此素材。');
    await this.store.delete('assets',assetId);return {deleted:true};
  }
  async pruneAssets() {
    const [drafts,jobs,assets]=await Promise.all([this.store.all('drafts'),this.store.all('jobs'),this.store.all('assets')]);
    const referenced=new Set(drafts.flatMap(d=>d.assetIds));
    for(const job of jobs) if(!['succeeded','cancelled','prepared'].includes(job.state)) {
      for(const aid of job.content.assetIds) referenced.add(aid);
    }
    const unused=assets.filter(a=>!referenced.has(a.id));
    await this.store.batch(unused.map(a=>({store:'assets',delete:true,id:a.id})));
    return {removed:unused.length,bytes:unused.reduce((n,a)=>n+a.size,0)};
  }
  async saveSettings(settings) {
    plain(settings,'设置');
    const theme=settings.theme??'light';assert(['light','dark','system'].includes(theme),'INVALID_THEME','主题无效。');
    const value={theme};await this.store.put('settings',{id:'preferences',value});return value;
  }
  async export() {
    const s=await this.snapshot();
    return {format:'matrixpost-backup',schema:1,version:VERSION,exportedAt:now(),drafts:s.drafts,
      accounts:s.accounts.map(a=>({...a,hasCredentials:false})),history:s.jobs,
      settings:s.settings,notice:'仅元数据；账号凭证与视频文件不包含在备份内。导入不会恢复执行任务。'};
  }
  async import(data) {
    plain(data,'备份');
    assert(data.format==='matrixpost-backup'&&data.schema===1,'INVALID_BACKUP','备份格式不受支持。');
    assert(Array.isArray(data.drafts)&&data.drafts.length<=500&&Array.isArray(data.accounts)&&data.accounts.length<=200,
      'INVALID_BACKUP','备份记录格式或数量无效。');
    const operations=[];
    for(const item of data.drafts) {
      const draft=normalizeDraft({...item,id:uuid(),assetIds:[]});
      operations.push({store:'drafts',value:draft});
    }
    for(const item of data.accounts) {
      plain(item);const p=this.platform(item.platform);
      const account={id:uuid(),platform:p.id,label:text(item.label,'账号名称',120,true),endpoint:item.endpoint?httpsUrl(item.endpoint).replace(/\/$/,''):'',
        remoteAccountId:text(item.remoteAccountId,'平台账号 ID',200),mode:item.mode==='api'&&p.mode==='api'?'api':'browser',
        hasCredentials:false,revision:1,createdAt:now(),updatedAt:now()};
      if(account.mode==='api') this.adapter.requiredOrigins(account);
      operations.push({store:'accounts',value:account});
    }
    await this.store.batch(operations);
    return {drafts:data.drafts.length,accounts:data.accounts.length,note:'已新增导入草稿和账号；请重新选择视频素材并授权账号。'};
  }
}
