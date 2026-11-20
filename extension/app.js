import { rpc, uploadAsset, connectBridge, disconnectBridge } from './core/client.js';
import { PLATFORMS } from './core/catalog.js';

const $ = (id) => document.getElementById(id);
const state = { drafts: [], accounts: [], jobs: [], assets: [], settings: {}, vault: { configured: false, unlocked: false } };
const viewLabels = { compose: '开始创作', drafts: '草稿箱', queue: '发布任务', accounts: '账号管理', platforms: '平台目录', ai: 'AI 连接' };
const jobLabels = { queued: '等待执行', scheduled: '等待计划时间', running: '正在处理', pending: '等待执行', succeeded: '已完成', completed: '已完成', failed: '执行失败', blocked: '需要处理', cancelled: '已取消', uncertain: '结果待核对', awaiting_approval: '等待批准', prepared: '内容已准备', needs_attention: '需要处理', awaiting_user: '等待手动提交' };
const modeLabels = { api: 'API 直连', assisted: '创作页辅助', planned: '待接入' };
let currentView = 'compose';
let currentKind = 'video';
let editorId = null;
let editorRevision;
let editorDirty = false;
let editorAssets = [];
let selectedAccounts = new Set();
let accountFingerprint = '';
let queueFilter = 'all';
let refreshPromise;
let pendingPlan;
let lastOutput = '';
let currentTheme = 'light';
let currentBridge = { connected: false };
let lastSnapshotError = '';
let autosaveTimer;
let savePromise;
let editorEpoch = 0;
let editorChanges = 0;
let editorConflict = false;
let lastSavedDraft;
let accountOpenedFromCompose = false;
let editorReady = false;
const editorMemoryKey = 'matrixpost.editor';

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value == null || value === false) continue;
    if (name === 'class') node.className = value;
    else if (name.startsWith('on') && typeof value === 'function') node.addEventListener(name.slice(2), value);
    else if (name === 'text') node.textContent = value;
    else if (name in node && !name.startsWith('aria-') && !name.startsWith('data-')) node[name] = value;
    else node.setAttribute(name, value === true ? '' : String(value));
  }
  for (const child of children.flat(Infinity)) {
    if (child != null && child !== false) node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

function icon(name) {
  const node = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  node.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
  use.setAttribute('href', `#i-${name}`);
  node.append(use);
  return node;
}

function button(label, className, handler, glyph) {
  return el('button', { class: className, type: 'button', onclick: handler }, glyph ? icon(glyph) : null, label);
}

function platform(id) { return (state.platforms || PLATFORMS).find((item) => item.id === id) || PLATFORMS.find((item) => item.id === id); }
function account(id) { return state.accounts.find((item) => item.id === id); }
function platformName(id) { return platform(id)?.name || id || '未知平台'; }
function platformMode(id) { return platform(id)?.mode || 'planned'; }
function accountMode(item) { return item?.mode === 'browser' ? 'assisted' : platformMode(item?.platform); }
function statusOf(job) { return job.status || job.state || 'queued'; }
function prettyError(error) { return error?.message || String(error || '操作未完成，请重试。'); }
function cleanUrl(value) {
  try { const url = new URL(value); return url.protocol === 'https:' && !url.username && !url.password ? url.href : null; } catch { return null; }
}
function platformReviewUrl(id, target) {
  const endpoint = cleanUrl(target?.endpoint);
  if (endpoint && id === 'woocommerce') {
    const base = new URL(endpoint);
    if (!base.pathname.endsWith('/')) base.pathname += '/';
    base.search = '';
    base.hash = '';
    return new URL('wp-admin/edit.php?post_type=product', base).href;
  }
  if (endpoint && id === 'shopify') return new URL('/admin/products', endpoint).href;
  return cleanUrl(platform(id)?.composeUrl || platform(id)?.home);
}
function bytes(value = 0) {
  if (value < 1024) return `${value} B`;
  if (value < 1048576) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1073741824) return `${(value / 1048576).toFixed(1)} MB`;
  return `${(value / 1073741824).toFixed(2)} GB`;
}
function date(value) {
  if (!value) return '刚刚';
  const stamp = new Date(value);
  return Number.isNaN(stamp.getTime()) ? '—' : stamp.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false });
}
function statePill(label, type = '') { return el('span', { class: `state-pill ${type}` }, label); }
function avatar(id, large = false) {
  const initials = { youtube: 'YT', woocommerce: 'W', shopify: 'S', bilibili: 'b', douyin: '抖', tiktok: 'Tk', xiaohongshu: '红', taobao: '淘', tmall: '猫', instagram: 'IG', facebook: 'f', ebay: 'e', amazon: 'a', kuaishou: '快', vimeo: 'v', wechat: '微' };
  return el('span', { class: `platform-avatar ${id || ''} ${large ? 'large' : ''}`, 'aria-hidden': 'true' }, initials[id] || platformName(id).slice(0, 2));
}
function toast(message, isError = false) {
  const node = el('div', { class: `toast${isError ? ' error' : ''}` }, icon(isError ? 'close' : 'check'), el('span', {}, message));
  $('toast-region').append(node);
  setTimeout(() => node.remove(), isError ? 6500 : 3500);
}
function showError(id, message) { $(id).textContent = message || ''; $(id).hidden = !message; }
async function busy(node, operation, options = {}) {
  if (node?.disabled) return;
  if (node) { node.disabled = true; node.setAttribute('aria-busy', 'true'); }
  try { return await operation(); }
  catch (error) { if (options.errorId) showError(options.errorId, prettyError(error)); else toast(prettyError(error), true); }
  finally { if (node) { node.disabled = false; node.removeAttribute('aria-busy'); } }
}

function setTheme(theme) {
  currentTheme = ['light', 'dark', 'system'].includes(theme) ? theme : 'light';
  document.documentElement.dataset.theme = currentTheme === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : currentTheme;
  document.querySelectorAll('[data-theme]').forEach((node) => { if (node.tagName === 'BUTTON') node.classList.toggle('selected', node.dataset.theme === currentTheme); });
}

function navigate(view, updateHash = true) {
  currentView = viewLabels[view] ? view : 'compose';
  for (const name of Object.keys(viewLabels)) $(`view-${name}`).hidden = name !== currentView;
  document.querySelectorAll('[data-view]').forEach((node) => { node.classList.toggle('active', node.dataset.view === currentView); node.setAttribute('aria-current', node.dataset.view === currentView ? 'page' : 'false'); node.title = viewLabels[node.dataset.view]; });
  $('view-crumb').textContent = viewLabels[currentView];
  document.title = `${viewLabels[currentView]} · MatriXpost`;
  if (updateHash && location.hash !== `#${currentView}`) history.replaceState(null, '', `#${currentView}`);
  renderActiveView();
}

async function refresh() {
  if (refreshPromise) return refreshPromise;
  refreshPromise = (async () => {
    try {
      const snapshot = await rpc('snapshot');
      Object.assign(state, snapshot);
      for (const key of ['drafts', 'accounts', 'jobs', 'assets']) if (!Array.isArray(state[key])) state[key] = [];
      state.vault ||= { configured: false, unlocked: false };
      state.settings ||= {};
      $('connection-error').hidden = true;
      lastSnapshotError = '';
      $('draft-count').textContent = String(state.drafts.length);
      $('job-count').textContent = String(state.jobs.filter((job) => ['queued', 'scheduled', 'running', 'awaiting_approval'].includes(statusOf(job))).length);
      $('sidebar-version').textContent = state.version || '0.6.0';
      $('settings-version').textContent = `版本 ${state.version || '0.6.0'} · 浏览器本地工作区`;
      $('vault-label').textContent = !state.vault.configured ? '设置凭证保险箱' : state.vault.unlocked ? '凭证已解锁' : '凭证已锁定';
      $('vault-open').classList.toggle('unlocked', !!state.vault.unlocked);
      renderStorageSummary();
      setTheme(state.settings.theme || currentTheme);
      const fingerprint = JSON.stringify(state.accounts) + currentKind;
      if (fingerprint !== accountFingerprint) { accountFingerprint = fingerprint; renderTargets(); }
      renderReadiness();
      renderActiveView();
    } catch (error) {
      lastSnapshotError = prettyError(error);
      $('connection-error').textContent = `工作区暂时不可用：${lastSnapshotError}`;
      $('connection-error').hidden = false;
    } finally { refreshPromise = null; }
  })();
  return refreshPromise;
}

function renderActiveView() {
  if (currentView === 'drafts') renderDrafts();
  if (currentView === 'queue') renderJobs();
  if (currentView === 'accounts') renderAccounts();
  if (currentView === 'platforms') renderPlatforms();
}

function setKind(kind, markDirty = true) {
  currentKind = kind === 'product' ? 'product' : 'video';
  document.querySelectorAll('[data-kind]').forEach((node) => { node.classList.toggle('active', node.dataset.kind === currentKind); node.setAttribute('aria-selected', String(node.dataset.kind === currentKind)); });
  $('video-upload-section').hidden = currentKind !== 'video';
  $('video-options').hidden = currentKind !== 'video';
  $('product-options').hidden = currentKind !== 'product';
  $('draft-title-label').textContent = currentKind === 'video' ? '视频标题' : '商品名称';
  $('compose-heading').textContent = `${editorId ? '编辑' : '新建'}${currentKind === 'video' ? '视频' : '商品'}`;
  $('draft-title').placeholder = currentKind === 'video' ? '例如：周末在家做一杯手冲咖啡' : '例如：白色陶瓷杯 · 300 mL';
  $('draft-description-label').textContent = currentKind === 'video' ? '内容描述' : '商品描述';
  $('draft-description').placeholder = currentKind === 'video' ? '这次拍了什么？补充做法、用到的东西或相关链接。' : '写清材质、尺寸、包装内容，以及购买前需要知道的事。';
  selectedAccounts = new Set([...selectedAccounts].filter((id) => platform(account(id)?.platform)?.kind === currentKind));
  renderTargets();
  if (markDirty) markEditorDirty();
}

function rememberEditor() {
  if (!editorReady) return;
  try {
    if (!editorId) { localStorage.removeItem(editorMemoryKey); return; }
    localStorage.setItem(editorMemoryKey, JSON.stringify({ id: editorId, accountIds: [...selectedAccounts], action: $('publish-action').value, scheduledAt: $('publish-time').value }));
  } catch { /* Draft content remains in IndexedDB when browser preference storage is unavailable. */ }
}

function restoreEditor() {
  let memory;
  try { memory = JSON.parse(localStorage.getItem(editorMemoryKey) || 'null'); } catch { return; }
  const draft = state.drafts.find((item) => item.id === memory?.id);
  if (!draft) return;
  const view = currentView;
  loadDraft(draft);
  selectedAccounts = new Set((Array.isArray(memory.accountIds) ? memory.accountIds : []).filter((id) => state.accounts.some((item) => item.id === id)));
  $('publish-action').value = memory.action === 'publish' ? 'publish' : 'draft';
  $('publish-time').value = typeof memory.scheduledAt === 'string' ? memory.scheduledAt : '';
  if ($('publish-time').value) $('publish-time').closest('details').open = true;
  renderTargets();
  navigate(view);
  $('editor-status').textContent = '已恢复上次编辑';
}

function scheduleAutosave() {
  clearTimeout(autosaveTimer);
  if (!editorDirty || editorConflict || !$('draft-title').value.trim() || !$('editor-form').checkValidity()) return;
  autosaveTimer = setTimeout(() => { saveEditor(false, true).catch(() => {}); }, 900);
}

function markEditorDirty() {
  editorDirty = true;
  editorChanges += 1;
  $('editor-status').textContent = editorConflict ? '修改尚未保存' : $('draft-title').value.trim() ? '等待保存…' : '填写标题后自动保存';
  $('title-count').textContent = `${$('draft-title').value.length} 字`;
  renderReadiness();
  scheduleAutosave();
}

function renderReadiness() {
  const selected = [...selectedAccounts].map(account).filter(Boolean);
  const assistedOnly = selected.length > 0 && selected.every((item) => accountMode(item) === 'assisted');
  $('publish-action-field').hidden = assistedOnly;
  $('publish-time').closest('details').hidden = assistedOnly;
  $('preview-publish').replaceChildren(assistedOnly ? '准备到平台后台' : '检查并继续', icon('arrow'));
  $('schedule-label').textContent = $('publish-time').value ? date($('publish-time').value) : '默认立即处理';
  let text;
  if (!$('draft-title').value.trim()) text = currentKind === 'video' ? '先写一个标题。' : '先填写商品名称。';
  else if (!selected.length) text = '选好账号，就可以检查发布。';
  else if (selected.some((item) => accountMode(item) === 'api' && !item.hasCredentials)) text = '有账号还缺少凭证，检查时可以补充。';
  else if (selected.some((item) => accountMode(item) === 'api') && !state.vault.unlocked) text = '发布前需要解锁账号凭证。';
  else if (selected.every((item) => accountMode(item) === 'assisted')) text = '内容会带到平台后台，由你完成最后提交。';
  else text = `已选 ${selected.length} 个账号。下一步核对内容和执行方式。`;
  $('compose-readiness').textContent = text;
}
function readDraft() {
  return {
    ...(editorId ? { id: editorId, revision: editorRevision } : {}),
    kind: currentKind,
    title: $('draft-title').value.trim(),
    description: $('draft-description').value,
    tags: $('draft-tags').value.split(/[,，\n]/u).map((item) => item.trim().replace(/^#/u, '')).filter(Boolean),
    assetIds: currentKind === 'video' ? editorAssets.map((item) => item.id) : [],
    video: { privacy: $('video-privacy').value, categoryId: $('video-category').value.trim() || '22', madeForKids: $('video-kids').checked },
    product: { price: $('product-price').value.trim(), currency: $('product-currency').value, quantity: Number($('product-quantity').value), sku: $('product-sku').value.trim(), images: $('product-images').value.split(/\n/u).map((item) => item.trim()).filter(Boolean), vendor: $('product-vendor').value.trim(), productType: $('product-type').value.trim(), publicationId: $('product-publication').value.trim(), trackInventory: $('product-track').checked, locationId: $('product-location').value.trim() },
  };
}

async function saveEditor(announce = true, automatic = false) {
  clearTimeout(autosaveTimer);
  while (savePromise) await savePromise;
  if (!editorDirty && lastSavedDraft) return lastSavedDraft;
  if (editorConflict) throw new Error('另一窗口已更新这份草稿。请将当前修改另存，或从草稿箱重新打开。');
  if (automatic ? !$('editor-form').checkValidity() : !$('editor-form').reportValidity()) throw new Error('请先检查标题和标记的字段。');
  const epoch = editorEpoch;
  const changes = editorChanges;
  const draft = readDraft();
  $('editor-status').textContent = '保存中…';
  savePromise = (async () => {
    try {
      const result = await rpc('drafts.save', { draft });
      const saved = result.draft || result;
      if (epoch === editorEpoch) {
        editorId = saved.id;
        editorRevision = saved.revision;
        lastSavedDraft = saved;
        editorDirty = changes !== editorChanges;
        $('editor-status').textContent = editorDirty ? '等待保存…' : '已保存';
        $('compose-heading').textContent = `编辑${currentKind === 'video' ? '视频' : '商品'}`;
        showError('editor-error', '');
        $('save-copy').hidden = true;
        rememberEditor();
        await refresh();
        if (editorDirty) scheduleAutosave();
        if (announce) toast('草稿已保存。');
      }
      return saved;
    } catch (error) {
      if (epoch === editorEpoch) {
        editorConflict = error.code === 'REVISION_CONFLICT';
        $('editor-status').textContent = '修改尚未保存';
        showError('editor-error', editorConflict ? '另一窗口已更新这份草稿。你输入的内容还在这里；可以另存一份，或从草稿箱重新打开最新内容。' : `自动保存未完成：${prettyError(error)}。可以点击“保存草稿”重试。`);
        $('save-copy').hidden = !editorConflict;
      }
      throw error;
    } finally { savePromise = null; }
  })();
  return savePromise;
}

function loadDraft(draft) {
  clearTimeout(autosaveTimer);
  editorEpoch += 1;
  editorChanges = 0;
  editorConflict = false;
  lastSavedDraft = draft || null;
  showError('editor-error', '');
  $('save-copy').hidden = true;
  editorId = draft?.id || null;
  editorRevision = draft?.revision;
  $('editor-form').reset();
  $('draft-title').value = draft?.title || '';
  $('draft-description').value = draft?.description || '';
  $('draft-tags').value = (draft?.tags || []).join(', ');
  $('video-privacy').value = draft?.video?.privacy || 'private';
  $('video-category').value = draft?.video?.categoryId || '22';
  $('video-kids').checked = !!draft?.video?.madeForKids;
  $('product-price').value = draft?.product?.price ?? '0.00';
  $('product-currency').value = draft?.product?.currency || 'USD';
  $('product-quantity').value = String(draft?.product?.quantity ?? 0);
  $('product-sku').value = draft?.product?.sku || '';
  $('product-images').value = (draft?.product?.images || []).join('\n');
  $('product-vendor').value = draft?.product?.vendor || '';
  $('product-type').value = draft?.product?.productType || '';
  $('product-publication').value = draft?.product?.publicationId || '';
  $('product-track').checked = !!draft?.product?.trackInventory;
  $('product-location').value = draft?.product?.locationId || '';
  editorAssets = (draft?.assetIds || []).map((id) => state.assets.find((item) => item.id === id) || { id, name: '本地素材', size: 0 });
  selectedAccounts.clear();
  $('publish-time').value = '';
  setKind(draft?.kind || 'video', false);
  editorDirty = false;
  $('editor-status').textContent = draft ? '已保存' : '填写标题后自动保存';
  $('title-count').textContent = `${$('draft-title').value.length} 字`;
  renderEditorAssets();
  rememberEditor();
  navigate('compose');
  window.scrollTo({ top: 0, behavior: 'instant' });
}

async function newDraft() {
  if (savePromise) { try { await savePromise; } catch { /* Keep the unsaved input for the decision below. */ } }
  if (editorDirty && $('draft-title').value.trim() && !editorConflict && $('editor-form').checkValidity()) {
    try { await saveEditor(false); } catch { return; }
  }
  if (editorDirty && ($('draft-title').value || $('draft-description').value || editorAssets.length) && !window.confirm('当前修改还没有保存。放弃这些修改并新建草稿？')) return;
  loadDraft(null);
  $('draft-title').focus();
}

async function addMedia(file) {
  if (!file) return;
  if (!file.type.startsWith('video/')) throw new Error('请选择视频文件（MP4、MOV 或 WebM）。');
  $('upload-zone').classList.add('busy');
  $('upload-title').textContent = '正在保存素材…';
  try {
    const value = await uploadAsset(file);
    const asset = value.asset || value;
    if (!asset.id) throw new Error('素材未能保存，请重试。');
    editorAssets = [{ name: file.name, size: file.size, type: file.type, ...asset }];
    markEditorDirty();
    renderEditorAssets();
    await refresh();
    toast('视频已加入本地素材库。');
  } finally {
    $('upload-zone').classList.remove('busy');
    $('upload-title').textContent = '选择视频，或拖到这里';
    $('media-file').value = '';
  }
}

function renderEditorAssets() {
  $('selected-assets').replaceChildren(...editorAssets.map((asset) => el('div', { class: 'asset-row' }, icon('video'), el('div', {}, el('strong', {}, asset.name || '视频素材'), el('small', {}, bytes(asset.size))), button('', 'icon-button', () => { editorAssets = editorAssets.filter((item) => item.id !== asset.id); renderEditorAssets(); markEditorDirty(); }, 'close'))));
  $('selected-assets').querySelectorAll('button').forEach((node) => { node.title = '从草稿移除素材'; node.setAttribute('aria-label', '从草稿移除素材'); });
  $('upload-description').textContent = editorAssets.length ? '重新选择文件会替换下方视频' : '文件会保存在此浏览器，发布前不会上传';
}

function renderTargets() {
  const items = state.accounts.filter((item) => platform(item.platform)?.kind === currentKind && platformMode(item.platform) !== 'planned');
  selectedAccounts = new Set([...selectedAccounts].filter((id) => items.some((item) => item.id === id)));
  if (!items.length) {
    $('target-accounts').replaceChildren(el('div', { class: 'target-empty' }, icon('accounts'), el('p', {}, `还没有${currentKind === 'video' ? '视频' : '电商'}账号`), button('添加第一个账号', 'inline-link', () => openAccountDialog())));
  } else {
    $('target-accounts').replaceChildren(...items.map((item) => {
      const input = el('input', { type: 'checkbox', checked: selectedAccounts.has(item.id), 'aria-label': `选择 ${item.label}`, onchange: () => { if (input.checked) selectedAccounts.add(item.id); else selectedAccounts.delete(item.id); renderTargetSummary(); } });
      const sub = accountMode(item) === 'api' ? (item.hasCredentials ? 'API 独立凭证' : '待添加凭证') : '当前浏览器会话';
      return el('label', { class: 'target-account' }, avatar(item.platform), el('span', { class: 'target-account-text' }, el('strong', {}, item.label), el('small', {}, `${platformName(item.platform)} · ${sub}`)), input);
    }));
  }
  renderTargetSummary();
}

function renderTargetSummary() {
  $('target-summary').replaceChildren(el('span', {}, selectedAccounts.size ? `已选择 ${selectedAccounts.size} 个账号` : '尚未选择账号'), selectedAccounts.size ? button('清空', 'inline-link', () => { selectedAccounts.clear(); renderTargets(); }) : el('span', {}, '按需选择'));
  renderReadiness();
  rememberEditor();
}

async function previewPublish() {
  if (!selectedAccounts.size) throw new Error('请先选择至少一个发布账号。');
  const saved = await saveEditor(false);
  const scheduled = [...selectedAccounts].some((id) => accountMode(account(id)) === 'api') ? $('publish-time').value : '';
  let scheduledAt;
  if (scheduled) {
    const time = new Date(scheduled);
    if (Number.isNaN(time.getTime()) || time.getTime() <= Date.now()) throw new Error('计划时间需要晚于现在。');
    scheduledAt = time.toISOString();
  }
  const params = { draftId: saved.id, accountIds: [...selectedAccounts], action: $('publish-action').value, revision: saved.revision, ...(scheduledAt ? { scheduledAt } : {}) };
  const preview = await rpc('publish.preview', { draftId: params.draftId, accountIds: params.accountIds, action: params.action });
  pendingPlan = { params, preview, draft: saved };
  renderPublishPlan();
  showError('preview-error', '');
  $('preview-dialog').showModal();
}

function renderPublishPlan() {
  const { params, preview, draft } = pendingPlan;
  const targets = preview.targets || [];
  const actionLabel = params.action === 'publish' ? (draft.kind === 'product' ? '上架商品' : `发布视频 · ${draft.video?.privacy === 'public' ? '公开' : draft.video?.privacy === 'unlisted' ? '不公开列出' : '私密'}`) : '平台草稿 / 私密视频';
  $('preview-summary').replaceChildren(el('strong', {}, draft.title), el('p', {}, `${actionLabel} · ${targets.length} 个目标${params.scheduledAt ? ` · ${date(params.scheduledAt)}` : ' · 加入队列后执行'}`));
  $('preview-targets').replaceChildren(...targets.map((target) => {
    const targetAccount = account(target.accountId);
    const id = target.platform || targetAccount?.platform;
    const mode = target.mode || platformMode(id);
    const label = target.label || targetAccount?.label || platformName(id);
    const errors = target.errors || [];
    const row = el('div', { class: 'preview-target' }, el('div', { class: 'preview-target-heading' }, avatar(id), el('strong', {}, label), statePill(target.valid ? modeLabels[mode] || '已准备' : '需要修改', target.valid ? '' : 'error')));
    if (errors.length) row.append(el('ul', {}, errors.map((item) => el('li', {}, typeof item === 'string' ? item : item.message || item.code || '字段不完整'))));
    if (mode === 'api' && !target.valid && targetAccount) {
      const fix = !state.vault.unlocked && targetAccount.hasCredentials
        ? button('解锁凭证后重新检查', 'inline-link', () => { $('preview-dialog').close(); openVaultDialog(); })
        : button('检查这个账号', 'inline-link', () => { $('preview-dialog').close(); openAccountDialog(targetAccount); });
      row.append(fix);
    }
    const previousVersion = state.jobs.find((job) => job.draftId === draft.id && job.accountId === target.accountId && job.draftRevision !== draft.revision && (job.hasRemoteContent || job.result?.remoteId));
    if (mode === 'api' && previousVersion) row.append(el('p', { class: 'revision-notice' }, '这个账号已有较早版本的内容。本次修改会新建一条视频或商品；如果只是想继续之前的进度，请返回发布任务。'));
    if (mode === 'assisted') {
      row.append(el('p', {}, '准备标题、描述与商品信息，并打开创作页面。请核对当前登录账号；本步骤不会提交发布。'));
      const prepare = button('准备并打开', 'secondary-button', async () => busy(prepare, async () => {
        const result = await rpc('assisted.prepare', { draftId: params.draftId, accountId: target.accountId });
        if (result.text) {
          try { await navigator.clipboard.writeText(result.text); toast('内容已复制，创作页已打开。'); }
          catch { toast('创作页已打开，可在下方复制准备内容。'); }
          const copy = button('复制内容', 'inline-link', async () => busy(copy, async () => { await navigator.clipboard.writeText(result.text); toast('内容已复制。'); }), 'copy');
          row.append(copy);
        } else toast('创作页已打开。');
        if (result.tabId != null) {
          const fill = button('填入已打开页面', 'secondary-button', () => busy(fill, async () => {
            const pageUrl = new URL(result.url || platform(id)?.composeUrl || platform(id)?.home);
            await requestOrigins([`${pageUrl.protocol}//${pageUrl.hostname}/*`]);
            const filled = await rpc('assisted.fill', { draftId: params.draftId, accountId: target.accountId, tabId: result.tabId });
            const found = Array.isArray(filled.filled) ? filled.filled.length : Number(filled.filled || 0);
            const missing = [...(filled.missing || []), ...(filled.ambiguous || []), ...(filled.occupied || [])].length;
            toast(found ? `已填写 ${found} 个字段${missing ? `，另有 ${missing} 个字段需要核对` : ''}。请到平台完成提交。` : '没有可安全填写的空字段，请到平台手动填写。');
          }));
          row.append(el('div', { class: 'button-row' }, fill));
        }
      }));
      prepare.disabled = !target.valid;
      row.append(el('div', { class: 'button-row' }, prepare));
    } else if (target.valid) row.append(el('p', {}, '将使用此账号的独立授权执行。平台返回的草稿、处理中与公开结果会分别记录。'));
    return row;
  }));
  const apiTargets = targets.filter((target) => (target.mode || platformMode(target.platform || account(target.accountId)?.platform)) === 'api');
  const readyTargets = apiTargets.filter((target) => target.valid);
  $('enqueue-confirm').disabled = !readyTargets.length;
  $('enqueue-confirm').textContent = readyTargets.length ? `确认加入队列 · ${readyTargets.length} 个账号` : apiTargets.length ? '请先处理上方问题' : '请在平台后台完成提交';
  $('preview-note').textContent = apiTargets.length ? `这次只加入已通过检查的 ${readyTargets.length} 个账号。${apiTargets.length > readyTargets.length ? `另有 ${apiTargets.length - readyTargets.length} 个账号需要处理，不会加入队列。` : ''}${targets.length > apiTargets.length ? '辅助账号请分别打开后台。' : ''}` : '这些平台需要手动提交。打开后台后，请确认登录账号、素材和内容。';
}

async function enqueuePlan() {
  if (!pendingPlan) return;
  const accountIds = (pendingPlan.preview.targets || []).filter((target) => (target.mode || platformMode(target.platform || account(target.accountId)?.platform)) === 'api' && target.valid).map((target) => target.accountId);
  if (!accountIds.length) throw new Error('没有可执行的 API 目标。');
  const jobs = await rpc('jobs.enqueue', { ...pendingPlan.params, accountIds });
  $('preview-dialog').close();
  await refresh();
  navigate('queue');
  toast(jobs.some((job) => statusOf(job) === 'cancelled' && job.hasRemoteContent) ? '平台已有内容，可在发布任务中继续原任务。' : jobs.every((job) => statusOf(job) === 'succeeded') ? '这份内容已处理，已为你打开原任务。' : '任务已加入队列，可在这里查看进度。');
}

function emptyState(glyph, title, description, actionLabel, action) {
  return el('div', { class: 'empty-state' }, el('span', { class: 'empty-icon' }, icon(glyph)), el('h3', {}, title), el('p', {}, description), actionLabel ? button(actionLabel, 'secondary-button', action, 'plus') : null);
}

function renderDrafts() {
  const query = $('draft-search').value.trim().toLocaleLowerCase();
  const filter = $('draft-filter').value;
  const items = state.drafts.filter((draft) => (filter === 'all' || draft.kind === filter) && `${draft.title} ${draft.description || ''} ${(draft.tags || []).join(' ')}`.toLocaleLowerCase().includes(query)).sort((a, b) => new Date(b.updatedAt || b.createdAt || 0) - new Date(a.updatedAt || a.createdAt || 0));
  $('draft-total').textContent = `${items.length} 份草稿`;
  if (!items.length) { $('draft-list').replaceChildren(emptyState('drafts', query || filter !== 'all' ? '没有匹配的草稿' : '还没有草稿', query || filter !== 'all' ? '试试更短的关键词，或查看全部类型。' : '写好标题后会自动保存，下次回来可以接着编辑。', query || filter !== 'all' ? null : '新建草稿', newDraft)); return; }
  $('draft-list').replaceChildren(...items.map((draft) => {
    const remove = button('', 'icon-button', () => busy(remove, async () => {
      if (!window.confirm(`删除草稿「${draft.title}」？已创建的发布任务不会因此撤回。`)) return;
      await rpc('drafts.delete', { id: draft.id });
      if (editorId === draft.id) loadDraft(null);
      await refresh();
      toast('草稿已删除。');
    }), 'close');
    remove.title = '删除草稿'; remove.setAttribute('aria-label', `删除草稿 ${draft.title}`);
    return el('article', { class: 'draft-card' }, el('div', { class: 'card-top' }, el('span', { class: 'kind-label' }, icon(draft.kind === 'product' ? 'product' : 'video'), draft.kind === 'product' ? '商品草稿' : '视频草稿'), remove), el('h3', {}, draft.title || '未命名草稿'), el('p', {}, draft.description || '还没有添加描述。'), el('div', { class: 'card-bottom' }, el('span', {}, `${date(draft.updatedAt || draft.createdAt)} 更新`), button('继续编辑', 'secondary-button', () => { if (!editorDirty || editorId === draft.id || window.confirm('当前编辑内容尚未保存，确定打开另一份草稿吗？')) loadDraft(draft); })));
  }));
}

function renderJobs() {
  const active = ['queued', 'scheduled', 'running', 'pending'];
  const attention = ['failed', 'blocked', 'uncertain', 'awaiting_approval', 'needs_attention', 'awaiting_user'];
  const complete = ['succeeded', 'completed', 'prepared'];
  const groups = [{ label: '全部任务', value: state.jobs.length }, { label: '等待与进行中', value: state.jobs.filter((job) => active.includes(statusOf(job))).length }, { label: '需要处理', value: state.jobs.filter((job) => attention.includes(statusOf(job))).length }, { label: '已完成', value: state.jobs.filter((job) => complete.includes(statusOf(job))).length }];
  $('queue-stats').replaceChildren(...groups.map((item) => el('div', { class: 'stat-item' }, el('span', {}, item.label), el('strong', {}, String(item.value)))));
  const blockedByVault = state.jobs.filter((job) => statusOf(job) === 'blocked' && ['VAULT_LOCKED', 'VAULT_NOT_CONFIGURED'].includes(job.error?.code));
  $('queue-attention').hidden = !blockedByVault.length;
  if (blockedByVault.length) $('queue-attention').replaceChildren(el('p', {}, `${blockedByVault.length} 个任务在等账号凭证解锁。解锁后，点击任务旁的“继续”。`), button('解锁凭证', 'secondary-button', openVaultDialog, 'lock'));
  const items = state.jobs.filter((job) => queueFilter === 'all' || (queueFilter === 'active' ? active : queueFilter === 'attention' ? attention : complete).includes(statusOf(job))).sort((a, b) => new Date(b.createdAt || 0) - new Date(a.createdAt || 0));
  $('queue-total').textContent = `${items.length} 个任务`;
  if (!items.length) { $('job-list').replaceChildren(emptyState('queue', state.jobs.length ? '这个分类下没有任务' : '还没有发布任务', state.jobs.length ? '切换筛选查看其他任务。' : '在创作页选择账号，检查通过后加入队列。', state.jobs.length ? null : '去写内容', () => navigate('compose'))); return; }
  const reviewStates = new Map([...$('job-list').querySelectorAll('.job-approval-preview')].map((node) => [node.dataset.jobId, { open: node.open, scrollTop: node.querySelector('.job-approval-content')?.scrollTop || 0 }]));
  $('job-list').replaceChildren(...items.map((job) => {
    const status = statusOf(job);
    const targetAccount = account(job.accountId);
    const id = job.platform || targetAccount?.platform || job.account?.platform;
    const draft = job.draft || job.draftSnapshot || state.drafts.find((item) => item.id === job.draftId);
    const actions = el('div', { class: 'job-actions' });
    const invoke = (label, method, className = 'secondary-button') => {
      const node = button(label, className, () => busy(node, async () => { await rpc(method, { id: job.id }); await refresh(); toast(label === '批准发布' ? '已批准此任务，队列将继续执行。' : '任务已更新。'); })); actions.append(node);
    };
    if (job.canRetry === true || (job.canRetry === undefined && ['failed', 'blocked', 'needs_attention'].includes(status))) invoke(job.hasRemoteContent ? '继续原任务' : '继续', 'jobs.retry');
    if (['queued', 'scheduled', 'pending', 'awaiting_approval', 'blocked', 'prepared', 'failed'].includes(status)) invoke('停止任务', 'jobs.cancel');
    if (status === 'blocked' && ['VAULT_LOCKED', 'VAULT_NOT_CONFIGURED'].includes(job.error?.code)) actions.prepend(button('解锁凭证', 'secondary-button', openVaultDialog));
    else if (['failed', 'blocked'].includes(status) && targetAccount) actions.prepend(button('检查账号', 'text-button', () => openAccountDialog(targetAccount)));
    const main = el('div', { class: 'job-main' }, el('div', { class: 'job-title' }, el('h3', {}, job.title || draft?.title || '内容任务'), statePill(jobLabels[status] || status, complete.includes(status) ? 'success' : attention.includes(status) ? (status === 'failed' ? 'error' : 'warning') : '')), el('div', { class: 'job-meta' }, el('span', {}, `${platformName(id)} · ${job.accountLabel || targetAccount?.label || job.account?.label || '账号已移除'}`), el('span', {}, job.action === 'publish' ? '发布 / 上架' : '远端草稿'), el('span', {}, `${date(job.createdAt)} 创建`)));
    if (job.scheduledAt && new Date(job.scheduledAt).getTime() - new Date(job.createdAt).getTime() > 60000) main.append(el('div', { class: 'job-detail' }, `计划时间：${date(job.scheduledAt)}`));
    if (job.error) main.append(el('div', { class: 'job-detail error' }, typeof job.error === 'string' ? job.error : job.error.message || job.error.code || '执行未完成'));
    if (status === 'uncertain') {
      main.append(el('div', { class: 'job-detail' }, job.canRetry ? '已有保存的进度，可以继续原任务。' : '请求已发出，但还没有收到明确结果。先到平台搜索这条内容；核对前不会重新创建。'));
      const reviewUrl = platformReviewUrl(id, targetAccount);
      if (reviewUrl) actions.append(el('a', { class: 'secondary-button job-result-link', href: reviewUrl, target: '_blank', rel: 'noopener noreferrer' }, icon('external'), '到平台核对'));
    }
    if (status === 'cancelled' && job.hasRemoteContent) main.append(el('div', { class: 'job-detail' }, '已停止后续处理，平台上的内容仍然保留。继续原任务会沿用已保存的进度。'));
    if (status === 'awaiting_approval') {
      main.append(el('div', { class: 'job-detail' }, '这项任务由 AI 发起。以下为创建任务时固定的内容，之后修改草稿不会改变此任务。'));
      const preview = job.preview;
      if (preview) {
        const review = el('details', { class: 'job-approval-preview', open: reviewStates.get(job.id)?.open ?? true, 'data-job-id': job.id }, el('summary', {}, '核对本次发布的完整内容'));
        const content = el('div', { class: 'job-approval-content', tabindex: 0, 'aria-label': '本次发布的固定内容快照' });
        const fields = el('dl', { class: 'job-approval-fields' });
        const add = (name, value) => fields.append(el('dt', {}, name), el('dd', {}, value == null || value === '' ? '未填写' : String(value)));
        add('目标平台', platformName(id));
        add('目标账号', job.accountLabel || targetAccount?.label || '账号已移除');
        add('目标动作', job.action === 'publish' ? (job.kind === 'product' ? '上架商品' : '发布视频') : '保存平台草稿 / 私密视频');
        add('内容版本', job.draftRevision != null ? `修订 ${job.draftRevision}` : '任务固定快照');
        add('标题', preview.title || job.title);
        add('完整描述', preview.description);
        add('全部标签', (preview.tags || []).join('、') || '无');
        if (job.kind === 'product') {
          const product = preview.product || {};
          add('价格与币种', `${product.price ?? '未填写'} ${product.currency || ''}`.trim());
          add('库存数量', product.quantity ?? 0);
          add('SKU', product.sku);
          add('品牌 / 供应商', product.vendor);
          add('商品类型', product.productType);
          add('全部商品图片', (product.images || []).join('\n') || '无');
          if (id === 'shopify') {
            add('发布渠道 ID', product.publicationId);
            add('管理库存', product.trackInventory ? '是' : '否');
            add('库存地点 ID', product.locationId);
          }
        } else {
          const privacyLabels = { private: '私密', unlisted: '不公开列出', public: '公开' };
          add('视频可见性', job.action === 'draft' ? '私密（平台草稿）' : privacyLabels[preview.video?.privacy] || preview.video?.privacy || '未指定');
          add('视频分类 ID', preview.video?.categoryId);
          add('面向儿童', preview.video?.madeForKids ? '是' : '否');
          add('本地素材', (preview.assetIds || []).map((assetId) => { const asset = state.assets.find((item) => item.id === assetId); return asset ? `${asset.name} · ${bytes(asset.size)} · ${assetId}` : `素材 ID：${assetId}`; }).join('\n') || '无');
        }
        content.append(fields);
        const approve = button('已核对以上内容，批准发布', 'primary-button', () => busy(approve, async () => { await rpc('jobs.approve', { id: job.id }); await refresh(); toast('已批准此任务，队列将继续执行。'); }));
        review.append(content, el('div', { class: 'job-approval-actions' }, approve));
        main.append(review);
      } else main.append(el('div', { class: 'job-detail error' }, '尚未载入任务的固定内容快照，暂时无法批准。请刷新工作区。'));
    }
    if (job.result?.status) {
      const resultLabels = { draft: '已存为平台草稿', processing: '平台正在处理', uploaded: '已上传', published: '已发布', private: '私密视频', unlisted: '不公开列出', public: '公开', active: '商品已启用', accepted: '平台已接收', prepared: '待你在后台提交' };
      main.append(el('div', { class: 'job-detail' }, `${resultLabels[job.result.status] || `平台状态：${job.result.status}`}${job.result.remoteId ? ` · ID ${job.result.remoteId}` : ''}`));
    }
    if (status === 'running' || (status === 'queued' && Number(job.progress) > 0)) main.append(el('div', { class: 'job-detail' }, `执行进度：${Number(job.progress || 0)}%${job.stage ? ` · ${job.stage}` : ''}`));
    const resultUrl = cleanUrl(job.result?.url || job.result?.remoteUrl);
    if (resultUrl) main.append(el('a', { class: 'job-result-link', href: resultUrl, target: '_blank', rel: 'noopener noreferrer' }, '查看平台结果', icon('external')));
    if (job.checkpoint?.offset && draft?.assetIds?.length) {
      const asset = state.assets.find((item) => item.id === draft.assetIds[0]);
      if (asset?.size) main.append(el('div', { class: 'job-detail' }, `上传 ${bytes(job.checkpoint.offset)} / ${bytes(asset.size)}`));
    }
    return el('article', { class: 'job-card' }, avatar(id), main, actions);
  }));
  $('job-list').querySelectorAll('.job-approval-preview').forEach((node) => { const previous = reviewStates.get(node.dataset.jobId); if (previous) node.querySelector('.job-approval-content').scrollTop = previous.scrollTop; });
}

function renderAccounts() {
  if (!state.accounts.length) { $('account-list').replaceChildren(emptyState('accounts', '连接你的第一个账号', '为品牌、店铺或个人频道取一个好认的名字，然后按平台选择授权方式。', '添加账号', () => openAccountDialog())); return; }
  $('account-list').replaceChildren(...state.accounts.map((item) => {
    const mode = accountMode(item);
    const note = mode === 'api' ? (item.hasCredentials ? '凭证已保存，执行时由平台核验' : '还缺少授权凭证，暂时无法发布') : '使用浏览器当前登录账号，提交前请核对';
    const remove = button('移除', 'text-button', () => busy(remove, async () => { if (!window.confirm(`移除账号「${item.label}」及其本地凭证？平台账号本身不会被删除。`)) return; await rpc('accounts.remove', { id: item.id }); await refresh(); toast('账号已移除。'); }));
    return el('article', { class: 'account-card' }, el('div', { class: 'card-top' }, avatar(item.platform, true), el('div', {}, el('h3', {}, item.label), el('span', { class: 'account-platform-name' }, platformName(item.platform)))), el('div', { class: 'account-details' }, note, item.endpoint ? el('span', { class: 'muted' }, item.endpoint) : null, item.remoteAccountId ? el('span', { class: 'muted' }, `账号备注：${item.remoteAccountId}`) : null), el('div', { class: 'card-bottom' }, statePill(mode === 'api' && !item.hasCredentials ? '待接入' : modeLabels[mode] || mode, mode === 'api' && !item.hasCredentials ? 'warning' : ''), button(mode === 'api' && !item.hasCredentials ? '补充凭证' : '编辑', 'secondary-button', () => openAccountDialog(item)), remove));
  }));
}

function fillPlatformSelect(selected) {
  const list = [...PLATFORMS].filter((item) => item.mode !== 'planned').sort((a, b) => a.kind.localeCompare(b.kind) || (a.mode === 'api' ? -1 : 1) - (b.mode === 'api' ? -1 : 1) || a.name.localeCompare(b.name));
  $('account-platform').replaceChildren(...list.map((item) => el('option', { value: item.id }, `${item.name} · ${modeLabels[item.mode] || item.mode}`)));
  if (selected) $('account-platform').value = selected;
  else {
    const first = list.find((item) => item.kind === currentKind && item.mode === 'api') || list[0];
    if (first) $('account-platform').value = first.id;
  }
}

function renderCredentialFields() {
  const selected = platform($('account-platform').value);
  const editing = !!$('account-id').value;
  const existing = account($('account-id').value);
  const isApi = selected?.mode === 'api' && existing?.mode !== 'browser';
  $('account-mode-note').textContent = isApi ? '填写店铺或频道的授权凭证后，可以在这里直接提交。也可以先保存账号，稍后补充。' : '内容会交给平台后台，由你完成提交。这里的账号名称只是备注，请在后台核对当前登录账号。';
  const setup = {
    youtube: ['准备自己频道的 OAuth access token。上传需 youtube.upload 权限；修改已有视频还需更新权限。', '先设置凭证密码，再粘贴 token。token 过期后可在这里换新。'],
    woocommerce: ['准备店铺的 Consumer Key 和 Consumer Secret，授权需要 Read/Write。', '填写店铺首页的 HTTPS 地址；商品币种必须与店铺一致。'],
    shopify: ['准备自己店铺的 Admin API access token，并授予商品写入权限。', '填写 myshopify.com 店铺地址。设置库存或上架渠道时，还需要对应权限和地点 / 渠道 ID。'],
  };
  $('account-guide').hidden = !isApi;
  $('account-guide').open = false;
  $('account-guide-content').replaceChildren(el('ol', {}, (setup[selected?.id] || []).map((text) => el('li', {}, text))), el('a', { href: 'https://github.com/LIghtJUNction/MatriXpost/blob/main/docs/PLATFORMS.md', target: '_blank', rel: 'noopener noreferrer' }, '查看授权要求与平台说明 ↗'));
  $('account-endpoint-field').hidden = !['woocommerce', 'shopify'].includes(selected?.id);
  $('account-endpoint').required = ['woocommerce', 'shopify'].includes(selected?.id);
  $('account-endpoint').placeholder = selected?.id === 'shopify' ? 'https://your-store.myshopify.com' : 'https://your-shop.example.com';
  $('account-remote-field').hidden = !isApi;
  const fields = isApi ? selected?.credentialFields || [] : [];
  $('account-credentials').replaceChildren();
  if (!fields.length) return;
  $('account-credentials').append(el('div', { class: 'credentials-heading' }, el('strong', {}, '平台授权凭证'), !state.vault.unlocked ? button(state.vault.configured ? '解锁保险箱' : '设置保险箱', 'inline-link', openVaultDialog) : el('span', {}, editing ? '留空保留已有凭证' : '可稍后补充')));
  for (const field of fields) {
    const key = field.key || field.name;
    const id = `credential-${key}`;
    $('account-credentials').append(el('div', { class: 'field' }, el('label', { htmlFor: id }, field.label || key), el('input', { id, name: key, type: 'password', autocomplete: 'off', placeholder: field.placeholder || (editing ? '留空以保留已有凭证' : '粘贴平台授权凭证'), 'data-credential': key })));
  }
  if (!state.vault.unlocked) $('account-credentials').append(el('p', { class: 'field-hint', id: 'account-vault-note' }, state.vault.configured ? '请先解锁保险箱。也可不填写凭证，先保存账号描述。' : '请先设置保险箱。也可不填写凭证，先保存账号描述。'));
}

function openAccountDialog(item, selectedPlatform) {
  accountOpenedFromCompose = currentView === 'compose' && !item;
  $('account-form').reset();
  $('account-id').value = item?.id || '';
  $('account-dialog-title').textContent = item ? '编辑账号' : '添加账号';
  fillPlatformSelect(item?.platform || selectedPlatform);
  $('account-platform').disabled = !!item;
  $('account-label').value = item?.label || '';
  $('account-endpoint').value = item?.endpoint || '';
  $('account-endpoint').readOnly = !!item;
  $('account-remote').value = item?.remoteAccountId || '';
  $('account-remote').readOnly = !!item;
  renderCredentialFields();
  showError('account-error', '');
  $('account-dialog').showModal();
}

async function requestOrigins(origins) {
  if (!origins.length) return;
  if (!globalThis.chrome?.permissions?.request) throw new Error('请在已安装的 MatriXpost 扩展中授予平台访问权限。');
  const granted = await chrome.permissions.request({ origins: [...new Set(origins)] });
  if (!granted) throw new Error('未获得所需的站点访问权限，账号尚未保存。');
}

async function saveAccount() {
  const id = $('account-id').value;
  const selected = platform($('account-platform').value);
  if (!selected) throw new Error('请选择平台。');
  const credentials = {};
  $('account-credentials').querySelectorAll('[data-credential]').forEach((input) => { if (input.value.trim()) credentials[input.dataset.credential] = input.value.trim(); });
  if (Object.keys(credentials).length && !state.vault.unlocked) throw new Error('请先设置并解锁凭证保险箱，再添加平台授权。');
  const endpoint = $('account-endpoint-field').hidden ? '' : $('account-endpoint').value.trim();
  const effectiveMode = account(id)?.mode || (selected.mode === 'api' ? 'api' : 'browser');
  let origins = effectiveMode === 'api' ? (selected.hosts || []) : [];
  if (endpoint) {
    let url;
    try { url = new URL(endpoint); } catch { throw new Error('店铺地址需要是完整的 HTTPS 地址。'); }
    if (url.protocol !== 'https:' || url.username || url.password) throw new Error('店铺地址需要使用 HTTPS，且不能包含用户名或密码。');
    if (effectiveMode === 'api') origins = [`https://${url.hostname}/*`];
  }
  await requestOrigins(origins);
  const savedAccount = { ...(id ? { id } : {}), platform: selected.id, label: $('account-label').value.trim(), endpoint, remoteAccountId: $('account-remote').value.trim(), mode: effectiveMode };
  const result = await rpc('accounts.save', { account: savedAccount, ...(Object.keys(credentials).length ? { credentials } : {}) });
  const saved = result.account || result;
  $('account-form').reset();
  $('account-dialog').close();
  await refresh();
  if (accountOpenedFromCompose && selected.kind === currentKind && saved.id) {
    selectedAccounts.add(saved.id);
    renderTargets();
  }
  toast(effectiveMode === 'api' && !saved.hasCredentials ? '账号已保存。补充授权凭证后就可以发布。' : accountOpenedFromCompose ? '账号已添加，也已选为这份内容的发布目标。' : '账号已保存。');
}

function openVaultDialog() {
  $('vault-form').reset();
  showError('vault-error', '');
  const configured = state.vault.configured;
  const unlocked = state.vault.unlocked;
  $('vault-dialog-title').textContent = unlocked ? '凭证保险箱已解锁' : configured ? '解锁凭证保险箱' : '设置凭证保险箱';
  $('vault-description').textContent = unlocked ? '当前会话可以使用平台凭证执行 API 任务。锁定后，新的 API 步骤将等待解锁。' : '平台密钥加密保存在此浏览器，解锁后才能执行 API 发布。';
  $('vault-password-fields').hidden = unlocked;
  $('vault-passphrase').required = !unlocked;
  $('vault-confirm-field').hidden = configured;
  $('vault-confirm').required = !configured;
  $('vault-no-recovery').hidden = configured;
  $('vault-submit').textContent = unlocked ? '锁定保险箱' : configured ? '解锁保险箱' : '创建保险箱';
  $('vault-dialog').showModal();
}

async function submitVault() {
  if (state.vault.unlocked) await rpc('vault.lock');
  else {
    const passphrase = $('vault-passphrase').value;
    if (passphrase.length < 12) throw new Error('保险箱密码至少需要 12 个字符。');
    if (!state.vault.configured && passphrase !== $('vault-confirm').value) throw new Error('两次输入的密码不一致。');
    await rpc(state.vault.configured ? 'vault.unlock' : 'vault.setup', { passphrase });
  }
  $('vault-form').reset();
  $('vault-dialog').close();
  await refresh();
  if ($('account-vault-note')) $('account-vault-note').textContent = state.vault.unlocked ? '保险箱已解锁，可保存平台授权凭证。' : '保险箱已锁定。保存凭证前请先解锁。';
  toast(state.vault.unlocked ? '凭证保险箱已解锁。' : '凭证保险箱已锁定。');
}

function renderPlatforms() {
  const query = $('platform-search').value.trim().toLocaleLowerCase();
  const filter = $('platform-filter').value;
  const items = (state.platforms || PLATFORMS).filter((item) => `${item.id} ${item.name} ${item.description || ''}`.toLocaleLowerCase().includes(query) && (filter === 'all' || item.kind === filter || item.mode === filter));
  $('platform-count').textContent = `${items.length} 个平台`;
  if (!items.length) { $('platform-list').replaceChildren(emptyState('platforms', '没有找到这个平台', '试试平台的英文名称，或清空筛选条件。')); return; }
  $('platform-list').replaceChildren(...items.map((item) => {
    const connect = item.mode === 'planned' ? el('span', { class: 'muted' }, '接入准备中') : button('添加账号', 'inline-link', () => openAccountDialog(null, item.id));
    const open = button('', 'icon-button', () => busy(open, async () => { await rpc('platforms.open', { platform: item.id }); }), 'external');
    open.title = `打开 ${item.name}`; open.setAttribute('aria-label', `打开 ${item.name}`);
    return el('article', { class: 'platform-card' }, el('div', { class: 'card-top' }, el('div', { class: 'platform-title' }, avatar(item.id, true), el('div', {}, el('h3', {}, item.name), el('div', { class: 'platform-kind' }, item.kind === 'product' ? '电商平台' : '视频平台'))), open), el('p', { class: 'platform-description' }, item.description || (item.mode === 'api' ? '官方 API 适配器已实现，实际执行需要授权并完成账号验证。' : item.mode === 'assisted' ? '准备内容并打开创作页面，在平台核对后手动提交。' : '已列入接入目录，当前没有可执行的发布适配器。')), el('div', { class: 'card-bottom' }, statePill(modeLabels[item.mode] || item.mode, item.mode === 'api' ? 'success' : item.mode === 'assisted' ? 'warning' : ''), connect));
  }));
}

function updateBridge(detail = {}) {
  const previousError = currentBridge.error;
  currentBridge = { connected: !!detail.connected, url: detail.url || null, error: detail.error || null };
  currentBridge.active = currentBridge.connected || !!currentBridge.url;
  $('bridge-state').textContent = currentBridge.connected ? '已连接' : detail.error ? '连接已中断' : '未连接';
  $('bridge-state').className = `state-pill ${currentBridge.connected ? 'success' : detail.error ? 'error' : ''}`;
  $('bridge-disconnect').disabled = !currentBridge.active;
  $('bridge-connect').textContent = currentBridge.active ? '重新配对' : '连接 Agent';
  if (detail.error && detail.error !== previousError) toast(prettyError(detail.error), true);
}

async function runCommand() {
  let request;
  try { request = JSON.parse($('command-input').value); } catch { throw new Error('命令不是有效的 JSON，请检查引号和逗号。'); }
  if (!request || typeof request.method !== 'string' || Array.isArray(request)) throw new Error('命令需要包含 method 字段和 params 对象。');
  if (request.params != null && (typeof request.params !== 'object' || Array.isArray(request.params))) throw new Error('params 需要是 JSON 对象。');
  try {
    const result = await rpc(request.method, request.params || {});
    lastOutput = JSON.stringify(result, null, 2);
    $('command-output').textContent = lastOutput;
    await refresh();
  } catch (error) {
    lastOutput = JSON.stringify({ error: { code: error.code || 'OPERATION_FAILED', message: prettyError(error) } }, null, 2);
    $('command-output').textContent = lastOutput;
    throw error;
  }
}

async function loadMethods() {
  const result = await rpc('methods.list');
  const methods = Array.isArray(result) ? result : result.methods || result.tools || [];
  $('method-list').replaceChildren(...methods.map((item) => el('div', { class: 'method-item' }, el('code', {}, typeof item === 'string' ? item : item.name || item.method), typeof item === 'object' && item.description ? el('span', {}, item.description) : null)));
  if (!methods.length) $('method-list').textContent = JSON.stringify(result, null, 2);
}

async function exportBackup() {
  const data = await rpc('backup.export');
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = el('a', { href: url, download: `MatriXpost-backup-${new Date().toISOString().slice(0, 10)}.json` });
  document.body.append(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  toast('备份已导出，不包含凭证和视频文件。');
}

function renderStorageSummary() {
  const total = state.assets.reduce((sum, asset) => sum + (Number.isFinite(asset.size) && asset.size > 0 ? asset.size : 0), 0);
  $('asset-storage-summary').textContent = `${state.assets.length} 个文件 · ${bytes(total)}`;
}

async function readStoragePermission() {
  if (!globalThis.chrome?.permissions?.contains) return;
  try {
    const allowed = await chrome.permissions.contains({ permissions: ['unlimitedStorage'] });
    $('storage-permission-status').textContent = allowed ? '已允许较大素材存储。实际可用容量仍受设备剩余空间限制。' : '当前为默认存储配额。需要保存较大视频时可手动授权。';
    $('storage-permission').disabled = allowed;
  } catch { $('storage-permission-status').textContent = '暂时无法读取存储权限状态。'; }
}

async function pruneAssets() {
  const result = await rpc('assets.prune');
  await refresh();
  renderStorageSummary();
  toast(result.removed ? `已清理 ${result.removed} 个未使用素材，释放 ${bytes(result.bytes || 0)}。` : '没有可清理的未使用素材。');
}

async function importBackup(file) {
  if (!file) return;
  if (file.size > 10 * 1024 * 1024) throw new Error('备份文件过大，请选择小于 10 MB 的 JSON 文件。');
  let data;
  try { data = JSON.parse(await file.text()); } catch { throw new Error('无法读取 JSON 备份，请检查文件格式。'); }
  if (!window.confirm('导入这份 MatriXpost 备份？请只选择你信任的备份文件，平台凭证和视频文件不会被导入。')) return;
  await rpc('backup.import', { data });
  await refresh();
  toast('备份已导入。请检查账号授权和本地素材。');
}

document.querySelectorAll('[data-view]').forEach((node) => node.addEventListener('click', () => navigate(node.dataset.view)));
document.querySelectorAll('[data-go]').forEach((node) => node.addEventListener('click', () => navigate(node.dataset.go)));
document.querySelectorAll('[data-close-dialog]').forEach((node) => node.addEventListener('click', () => node.closest('dialog').close()));
document.querySelectorAll('[data-kind]').forEach((node) => node.addEventListener('click', () => setKind(node.dataset.kind)));
document.querySelectorAll('dialog').forEach((dialog) => dialog.addEventListener('click', (event) => { if (event.target === dialog) { const rect = dialog.getBoundingClientRect(); if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close(); } }));
window.addEventListener('hashchange', () => navigate(location.hash.slice(1), false));
$('editor-form').addEventListener('input', markEditorDirty);
$('editor-form').addEventListener('change', markEditorDirty);
$('editor-form').addEventListener('submit', (event) => { event.preventDefault(); busy($('save-draft'), () => saveEditor()); });
$('save-copy').addEventListener('click', () => busy($('save-copy'), async () => {
  if (savePromise) { try { await savePromise; } catch { /* Preserve the user's current input as a separate draft. */ } }
  editorEpoch += 1;
  editorId = null;
  editorRevision = undefined;
  lastSavedDraft = null;
  editorConflict = false;
  editorDirty = true;
  await saveEditor();
}));
$('publish-action').addEventListener('change', rememberEditor);
$('publish-time').addEventListener('change', () => { rememberEditor(); renderReadiness(); });
for (const id of ['new-draft', 'drafts-new']) $(id).addEventListener('click', newDraft);
for (const id of ['header-add-account', 'target-add-account', 'accounts-add']) $(id).addEventListener('click', () => openAccountDialog());
for (const id of ['vault-open', 'accounts-vault']) $(id).addEventListener('click', openVaultDialog);
$('media-file').addEventListener('change', () => addMedia($('media-file').files[0]).catch((error) => toast(prettyError(error), true)));
for (const type of ['dragenter', 'dragover']) $('upload-zone').addEventListener(type, (event) => { event.preventDefault(); $('upload-zone').classList.add('dragover'); });
for (const type of ['dragleave', 'drop']) $('upload-zone').addEventListener(type, (event) => { event.preventDefault(); $('upload-zone').classList.remove('dragover'); });
$('upload-zone').addEventListener('drop', (event) => addMedia(event.dataTransfer?.files?.[0]).catch((error) => toast(prettyError(error), true)));
$('preview-publish').addEventListener('click', () => busy($('preview-publish'), previewPublish));
$('enqueue-confirm').addEventListener('click', () => busy($('enqueue-confirm'), enqueuePlan, { errorId: 'preview-error' }));
$('draft-search').addEventListener('input', renderDrafts);
$('draft-filter').addEventListener('change', renderDrafts);
$('platform-search').addEventListener('input', renderPlatforms);
$('platform-filter').addEventListener('change', renderPlatforms);
$('job-filters').querySelectorAll('[data-filter]').forEach((node) => node.addEventListener('click', () => { queueFilter = node.dataset.filter; $('job-filters').querySelectorAll('[data-filter]').forEach((item) => item.classList.toggle('active', item === node)); renderJobs(); }));
$('queue-run').addEventListener('click', () => busy($('queue-run'), async () => { await rpc('jobs.run'); await refresh(); toast('已请求处理待执行任务。'); }));
$('account-platform').addEventListener('change', renderCredentialFields);
$('account-form').addEventListener('submit', (event) => { event.preventDefault(); showError('account-error', ''); busy($('account-save'), saveAccount, { errorId: 'account-error' }); });
$('vault-form').addEventListener('submit', (event) => { event.preventDefault(); showError('vault-error', ''); busy($('vault-submit'), submitVault, { errorId: 'vault-error' }); });
$('account-dialog').addEventListener('close', () => { $('account-credentials').querySelectorAll('input').forEach((node) => { node.value = ''; }); });
$('vault-dialog').addEventListener('close', () => $('vault-form').reset());
$('settings-open').addEventListener('click', () => { showError('settings-error', ''); showError('storage-error', ''); renderStorageSummary(); $('settings-dialog').showModal(); readStoragePermission(); });
document.querySelectorAll('button[data-theme]').forEach((node) => node.addEventListener('click', () => busy(node, async () => { await rpc('settings.save', { settings: { theme: node.dataset.theme } }); setTheme(node.dataset.theme); await refresh(); })));
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => { if (currentTheme === 'system') setTheme('system'); });
$('backup-export').addEventListener('click', () => busy($('backup-export'), exportBackup, { errorId: 'settings-error' }));
$('assets-prune').addEventListener('click', () => { showError('storage-error', ''); busy($('assets-prune'), pruneAssets, { errorId: 'storage-error' }); });
$('storage-permission').addEventListener('click', async () => {
  showError('storage-error', '');
  await busy($('storage-permission'), async () => {
    if (!globalThis.chrome?.permissions?.request) throw new Error('请在已安装的 MatriXpost 扩展中授予存储权限。');
    // No asynchronous work before this request: retain the explicit button gesture.
    const allowed = await chrome.permissions.request({ permissions: ['unlimitedStorage'] });
    $('storage-permission-status').textContent = allowed ? '已允许较大素材存储。实际可用容量仍受设备剩余空间限制。' : '未授予额外存储权限，继续使用默认配额。';
    toast(allowed ? '已允许较大素材存储。' : '未更改存储权限。');
  }, { errorId: 'storage-error' });
  await readStoragePermission();
});
$('backup-file').addEventListener('change', async () => { showError('settings-error', ''); try { await importBackup($('backup-file').files[0]); } catch (error) { showError('settings-error', prettyError(error)); } finally { $('backup-file').value = ''; } });
$('bridge-form').addEventListener('submit', (event) => { event.preventDefault(); busy($('bridge-connect'), async () => { const result = await connectBridge({ url: $('bridge-url').value.trim(), token: $('bridge-token').value.trim() }); updateBridge(result || { connected: true }); $('bridge-token').value = ''; toast('Agent 桥接已连接。'); }); });
$('bridge-disconnect').addEventListener('click', async () => { await busy($('bridge-disconnect'), async () => { const result = await disconnectBridge(); updateBridge(result || { connected: false }); toast('Agent 桥接已断开。'); }); updateBridge(currentBridge); });
window.addEventListener('matrixpost:bridge', (event) => updateBridge(event.detail));
$('ai-methods-load').addEventListener('click', () => busy($('ai-methods-load'), loadMethods));
$('command-run').addEventListener('click', () => busy($('command-run'), runCommand));
$('command-copy').addEventListener('click', () => busy($('command-copy'), async () => { if (!lastOutput) throw new Error('还没有可复制的结果。'); await navigator.clipboard.writeText(lastOutput); toast('结果已复制。'); }));
document.querySelectorAll('[data-command]').forEach((node) => node.addEventListener('click', () => { $('command-input').value = JSON.stringify({ method: node.dataset.command, params: {} }, null, 2); $('command-input').focus(); }));
document.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === 'Enter' && !document.querySelector('dialog[open]')) {
    event.preventDefault();
    if (currentView === 'compose') busy($('save-draft'), () => saveEditor());
    else if (currentView === 'ai' && event.target === $('command-input')) busy($('command-run'), runCommand);
  }
  if (event.key.toLowerCase() === 'n' && !event.ctrlKey && !event.metaKey && !event.altKey && !['INPUT', 'TEXTAREA', 'SELECT'].includes(event.target.tagName) && !document.querySelector('dialog[open]')) { event.preventDefault(); newDraft(); }
});
window.addEventListener('beforeunload', (event) => {
  if (editorDirty && ($('draft-title').value || $('draft-description').value || editorAssets.length)) {
    event.preventDefault();
    event.returnValue = '';
  }
});

setTheme('light');
navigate(location.hash.slice(1) || 'compose', false);
renderTargets();
refresh().then(() => { restoreEditor(); editorReady = true; });
setInterval(() => { if (!document.hidden && !document.querySelector('dialog[open]')) refresh(); }, 3000);
document.addEventListener('visibilitychange', () => { if (!document.hidden) refresh(); });
