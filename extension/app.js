import { rpc, uploadAsset, connectBridge, disconnectBridge } from './core/client.js';
import { PLATFORMS } from './core/catalog.js';

const $ = (id) => document.getElementById(id);
const state = { drafts: [], accounts: [], jobs: [], assets: [], settings: {}, vault: { configured: false, unlocked: false } };
const viewLabels = { compose: '开始创作', drafts: '草稿箱', queue: '发布任务', accounts: '账号管理', platforms: '平台目录', ai: 'AI 工作台' };
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
function statusOf(job) { return job.status || job.state || 'queued'; }
function prettyError(error) { return error?.message || String(error || '操作未完成，请重试。'); }
function cleanUrl(value) {
  try { const url = new URL(value); return url.protocol === 'https:' && !url.username && !url.password ? url.href : null; } catch { return null; }
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
      $('sidebar-version').textContent = state.version || '0.1.0';
      $('settings-version').textContent = `版本 ${state.version || '0.1.0'} · 浏览器本地工作区`;
      $('vault-label').textContent = !state.vault.configured ? '设置凭证保险箱' : state.vault.unlocked ? '凭证已解锁' : '凭证已锁定';
      $('vault-open').classList.toggle('unlocked', !!state.vault.unlocked);
      setTheme(state.settings.theme || currentTheme);
      const fingerprint = JSON.stringify(state.accounts) + currentKind;
      if (fingerprint !== accountFingerprint) { accountFingerprint = fingerprint; renderTargets(); }
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
  $('draft-title').placeholder = currentKind === 'video' ? '一个值得被看见的标题' : '清楚、准确地介绍你的商品';
  $('draft-description-label').textContent = currentKind === 'video' ? '内容描述' : '商品描述';
  $('draft-description').placeholder = currentKind === 'video' ? '介绍内容，也告诉观众为什么值得停留。' : '材质、规格、用途，还有让人选择它的理由。';
  selectedAccounts = new Set([...selectedAccounts].filter((id) => platform(account(id)?.platform)?.kind === currentKind));
  renderTargets();
  if (markDirty) markEditorDirty();
}

function markEditorDirty() { editorDirty = true; $('editor-status').textContent = '有未保存更改'; $('title-count').textContent = `${$('draft-title').value.length} 字`; }
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

async function saveEditor(announce = true) {
  if (!$('editor-form').reportValidity()) throw new Error('请先填写内容标题。');
  const result = await rpc('drafts.save', { draft: readDraft() });
  const saved = result.draft || result;
  editorId = saved.id;
  editorRevision = saved.revision;
  editorDirty = false;
  $('editor-status').textContent = '已保存到草稿箱';
  await refresh();
  if (announce) toast('草稿已保存。');
  return saved;
}

function loadDraft(draft) {
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
  $('editor-status').textContent = draft ? '已保存到草稿箱' : '未保存';
  $('title-count').textContent = `${$('draft-title').value.length} 字`;
  renderEditorAssets();
  navigate('compose');
  window.scrollTo({ top: 0, behavior: 'instant' });
}

function newDraft() {
  if (editorDirty && ($('draft-title').value || $('draft-description').value || editorAssets.length) && !window.confirm('当前内容尚未保存。确定新建另一份草稿吗？')) return;
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
    $('upload-title').textContent = '把视频拖到这里';
    $('media-file').value = '';
  }
}

function renderEditorAssets() {
  $('selected-assets').replaceChildren(...editorAssets.map((asset) => el('div', { class: 'asset-row' }, icon('video'), el('div', {}, el('strong', {}, asset.name || '视频素材'), el('small', {}, bytes(asset.size))), button('', 'icon-button', () => { editorAssets = editorAssets.filter((item) => item.id !== asset.id); renderEditorAssets(); markEditorDirty(); }, 'close'))));
  $('selected-assets').querySelectorAll('button').forEach((node) => { node.title = '从草稿移除素材'; node.setAttribute('aria-label', '从草稿移除素材'); });
  $('upload-description').textContent = editorAssets.length ? '选择新文件以替换当前视频' : '或点击选择文件，素材仅保存在此浏览器';
}

function renderTargets() {
  const items = state.accounts.filter((item) => platform(item.platform)?.kind === currentKind && platformMode(item.platform) !== 'planned');
  selectedAccounts = new Set([...selectedAccounts].filter((id) => items.some((item) => item.id === id)));
  if (!items.length) {
    $('target-accounts').replaceChildren(el('div', { class: 'target-empty' }, icon('accounts'), el('p', {}, `还没有${currentKind === 'video' ? '视频' : '电商'}账号`), button('添加第一个账号', 'inline-link', () => openAccountDialog())));
  } else {
    $('target-accounts').replaceChildren(...items.map((item) => {
      const input = el('input', { type: 'checkbox', checked: selectedAccounts.has(item.id), 'aria-label': `选择 ${item.label}`, onchange: () => { if (input.checked) selectedAccounts.add(item.id); else selectedAccounts.delete(item.id); renderTargetSummary(); } });
      const sub = platformMode(item.platform) === 'api' ? (item.hasCredentials ? 'API 独立凭证' : '待添加凭证') : '当前浏览器会话';
      return el('label', { class: 'target-account' }, avatar(item.platform), el('span', { class: 'target-account-text' }, el('strong', {}, item.label), el('small', {}, `${platformName(item.platform)} · ${sub}`)), input);
    }));
  }
  renderTargetSummary();
}

function renderTargetSummary() {
  $('target-summary').replaceChildren(el('span', {}, selectedAccounts.size ? `已选择 ${selectedAccounts.size} 个账号` : '尚未选择账号'), selectedAccounts.size ? button('清空', 'inline-link', () => { selectedAccounts.clear(); renderTargets(); }) : el('span', {}, '按需选择'));
}

async function previewPublish() {
  if (!selectedAccounts.size) throw new Error('请先选择至少一个发布账号。');
  const saved = await saveEditor(false);
  const scheduled = $('publish-time').value;
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
  $('enqueue-confirm').disabled = !apiTargets.length || apiTargets.some((target) => !target.valid);
  $('preview-note').textContent = apiTargets.length ? `队列将处理 ${apiTargets.length} 个 API 账号。${targets.length > apiTargets.length ? '创作页辅助账号请分别点击「准备并打开」。' : ''}提交期间请保持浏览器运行。` : '这些平台使用创作页辅助，请分别打开页面核对内容后提交。';
}

async function enqueuePlan() {
  if (!pendingPlan) return;
  const accountIds = (pendingPlan.preview.targets || []).filter((target) => (target.mode || platformMode(target.platform || account(target.accountId)?.platform)) === 'api' && target.valid).map((target) => target.accountId);
  if (!accountIds.length) throw new Error('没有可执行的 API 目标。');
  await rpc('jobs.enqueue', { ...pendingPlan.params, accountIds });
  $('preview-dialog').close();
  await refresh();
  navigate('queue');
  toast('发布任务已加入队列。');
}

function emptyState(glyph, title, description, actionLabel, action) {
  return el('div', { class: 'empty-state' }, el('span', { class: 'empty-icon' }, icon(glyph)), el('h3', {}, title), el('p', {}, description), actionLabel ? button(actionLabel, 'secondary-button', action, 'plus') : null);
}

function renderDrafts() {
  const query = $('draft-search').value.trim().toLocaleLowerCase();
  const filter = $('draft-filter').value;
  const items = state.drafts.filter((draft) => (filter === 'all' || draft.kind === filter) && `${draft.title} ${draft.description || ''} ${(draft.tags || []).join(' ')}`.toLocaleLowerCase().includes(query)).sort((a, b) => new Date(b.updatedAt || b.createdAt || 0) - new Date(a.updatedAt || a.createdAt || 0));
  $('draft-total').textContent = `${items.length} 份草稿`;
  if (!items.length) { $('draft-list').replaceChildren(emptyState('drafts', query || filter !== 'all' ? '没有匹配的草稿' : '下一条好内容，从这里开始', query || filter !== 'all' ? '试试更短的关键词，或查看全部类型。' : '保存视频或商品资料，准备好后再分发到不同平台。', query || filter !== 'all' ? null : '创建第一份草稿', newDraft)); return; }
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
  $('queue-stats').replaceChildren(...groups.map((item) => el('div', { class: 'stat-item' }, el('span', {}, item.label), el('strong', {}, String(item.value).padStart(2, '0')))));
  const items = state.jobs.filter((job) => queueFilter === 'all' || (queueFilter === 'active' ? active : queueFilter === 'attention' ? attention : complete).includes(statusOf(job))).sort((a, b) => new Date(b.createdAt || 0) - new Date(a.createdAt || 0));
  $('queue-total').textContent = `${items.length} 个任务`;
  if (!items.length) { $('job-list').replaceChildren(emptyState('queue', state.jobs.length ? '这里还没有任务' : '每一次发布，都有迹可循', state.jobs.length ? '切换筛选查看其他任务。' : '在创作页选好内容和账号，检查计划后开始执行。', state.jobs.length ? null : '去准备内容', () => navigate('compose'))); return; }
  $('job-list').replaceChildren(...items.map((job) => {
    const status = statusOf(job);
    const targetAccount = account(job.accountId);
    const id = job.platform || targetAccount?.platform || job.account?.platform;
    const draft = job.draft || job.draftSnapshot || state.drafts.find((item) => item.id === job.draftId);
    const actions = el('div', { class: 'job-actions' });
    const invoke = (label, method, className = 'secondary-button') => {
      const node = button(label, className, () => busy(node, async () => { await rpc(method, { id: job.id }); await refresh(); toast(label === '批准发布' ? '已批准此任务，队列将继续执行。' : '任务已更新。'); })); actions.append(node);
    };
    if (status === 'awaiting_approval') invoke('批准发布', 'jobs.approve', 'primary-button');
    if (job.canRetry === true || (job.canRetry === undefined && ['failed', 'blocked', 'needs_attention'].includes(status))) invoke('重试', 'jobs.retry');
    if (['queued', 'scheduled', 'pending', 'awaiting_approval', 'blocked', 'prepared', 'failed'].includes(status)) invoke('取消', 'jobs.cancel');
    const main = el('div', { class: 'job-main' }, el('div', { class: 'job-title' }, el('h3', {}, job.title || draft?.title || '内容任务'), statePill(jobLabels[status] || status, complete.includes(status) ? 'success' : attention.includes(status) ? (status === 'failed' ? 'error' : 'warning') : '')), el('div', { class: 'job-meta' }, el('span', {}, `${platformName(id)} · ${job.accountLabel || targetAccount?.label || job.account?.label || '账号已移除'}`), el('span', {}, job.action === 'publish' ? '发布 / 上架' : '远端草稿'), el('span', {}, `${date(job.createdAt)} 创建`)));
    if (job.scheduledAt && new Date(job.scheduledAt).getTime() - new Date(job.createdAt).getTime() > 60000) main.append(el('div', { class: 'job-detail' }, `计划时间：${date(job.scheduledAt)}`));
    if (job.error) main.append(el('div', { class: 'job-detail error' }, typeof job.error === 'string' ? job.error : job.error.message || job.error.code || '执行未完成'));
    if (status === 'uncertain') main.append(el('div', { class: 'job-detail' }, '平台可能已收到请求。请先到平台核对结果，避免重复发布。'));
    if (status === 'awaiting_approval') main.append(el('div', { class: 'job-detail' }, '这项任务由 AI 发起。请核对内容、账号和目标动作后批准。'));
    if (job.result?.status) main.append(el('div', { class: 'job-detail' }, `平台状态：${job.result.status}${job.result.remoteId ? ` · ID ${job.result.remoteId}` : ''}`));
    if (status === 'running' || (status === 'queued' && Number(job.progress) > 0)) main.append(el('div', { class: 'job-detail' }, `执行进度：${Number(job.progress || 0)}%${job.stage ? ` · ${job.stage}` : ''}`));
    const resultUrl = cleanUrl(job.result?.url || job.result?.remoteUrl);
    if (resultUrl) main.append(el('a', { class: 'job-result-link', href: resultUrl, target: '_blank', rel: 'noopener noreferrer' }, '查看平台结果', icon('external')));
    if (job.checkpoint?.offset && draft?.assetIds?.length) {
      const asset = state.assets.find((item) => item.id === draft.assetIds[0]);
      if (asset?.size) main.append(el('div', { class: 'job-detail' }, `上传 ${bytes(job.checkpoint.offset)} / ${bytes(asset.size)}`));
    }
    return el('article', { class: 'job-card' }, avatar(id), main, actions);
  }));
}

function renderAccounts() {
  if (!state.accounts.length) { $('account-list').replaceChildren(emptyState('accounts', '连接你的第一个账号', '为品牌、店铺或个人频道取一个好认的名字，然后按平台选择授权方式。', '添加账号', () => openAccountDialog())); return; }
  $('account-list').replaceChildren(...state.accounts.map((item) => {
    const mode = platformMode(item.platform);
    const note = mode === 'api' ? (item.hasCredentials ? '已保存加密凭证' : '尚未添加 API 凭证') : '当前浏览器登录状态';
    const remove = button('移除', 'text-button', () => busy(remove, async () => { if (!window.confirm(`移除账号「${item.label}」及其本地凭证？平台账号本身不会被删除。`)) return; await rpc('accounts.remove', { id: item.id }); await refresh(); toast('账号已移除。'); }));
    return el('article', { class: 'account-card' }, el('div', { class: 'card-top' }, avatar(item.platform, true), el('div', {}, el('h3', {}, item.label), el('span', { class: 'account-platform-name' }, platformName(item.platform)))), el('div', { class: 'account-details' }, note, item.endpoint ? el('span', { class: 'muted' }, item.endpoint) : null, item.remoteAccountId ? el('span', { class: 'muted' }, `账号标识：${item.remoteAccountId}`) : null), el('div', { class: 'card-bottom' }, statePill(modeLabels[mode] || mode), button('编辑', 'secondary-button', () => openAccountDialog(item)), remove));
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
  const isApi = selected?.mode === 'api';
  $('account-mode-note').textContent = isApi ? '使用平台官方 API。每个账号使用自己的授权凭证，密钥加密保存在本地保险箱。' : '打开平台创作页并辅助准备内容。账号代表当前浏览器登录状态；普通标签页之间不会隔离账号。';
  $('account-endpoint-field').hidden = !['woocommerce', 'shopify'].includes(selected?.id);
  $('account-endpoint').required = ['woocommerce', 'shopify'].includes(selected?.id);
  $('account-endpoint').placeholder = selected?.id === 'shopify' ? 'https://your-store.myshopify.com' : 'https://your-shop.example.com';
  $('account-remote-field').hidden = !isApi;
  const fields = selected?.credentialFields || [];
  $('account-credentials').replaceChildren();
  if (!fields.length) return;
  $('account-credentials').append(el('div', { class: 'credentials-heading' }, el('strong', {}, '平台授权凭证'), el('span', {}, editing ? '留空保留已有凭证' : '可稍后补充')));
  for (const field of fields) {
    const key = field.key || field.name;
    const id = `credential-${key}`;
    $('account-credentials').append(el('div', { class: 'field' }, el('label', { htmlFor: id }, field.label || key), el('input', { id, name: key, type: 'password', autocomplete: 'off', placeholder: field.placeholder || (editing ? '留空以保留已有凭证' : '粘贴平台授权凭证'), 'data-credential': key })));
  }
  if (!state.vault.unlocked) $('account-credentials').append(el('p', { class: 'field-hint' }, state.vault.configured ? '填写凭证前，请先通过顶部按钮解锁保险箱。也可先保存账号描述。' : '填写凭证前，请先通过顶部按钮设置保险箱。也可先保存账号描述。'));
}

function openAccountDialog(item, selectedPlatform) {
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
  let origins = selected.mode === 'api' ? (selected.hosts || []) : [];
  if (endpoint) {
    let url;
    try { url = new URL(endpoint); } catch { throw new Error('店铺地址需要是完整的 HTTPS 地址。'); }
    if (url.protocol !== 'https:' || url.username || url.password) throw new Error('店铺地址需要使用 HTTPS，且不能包含用户名或密码。');
    origins = [`https://${url.hostname}/*`];
  }
  await requestOrigins(origins);
  const savedAccount = { ...(id ? { id } : {}), platform: selected.id, label: $('account-label').value.trim(), endpoint, remoteAccountId: $('account-remote').value.trim(), mode: selected.mode === 'api' ? 'api' : 'browser' };
  await rpc('accounts.save', { account: savedAccount, ...(Object.keys(credentials).length ? { credentials } : {}) });
  $('account-form').reset();
  $('account-dialog').close();
  await refresh();
  toast('账号已保存。');
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
  toast(state.vault.unlocked ? '凭证保险箱已解锁。' : '凭证保险箱已锁定。');
}

function renderPlatforms() {
  const query = $('platform-search').value.trim().toLocaleLowerCase();
  const filter = $('platform-filter').value;
  const items = (state.platforms || PLATFORMS).filter((item) => `${item.id} ${item.name} ${item.description || ''}`.toLocaleLowerCase().includes(query) && (filter === 'all' || item.kind === filter || item.mode === filter));
  $('platform-count').textContent = `${items.length} PLATFORMS`;
  if (!items.length) { $('platform-list').replaceChildren(emptyState('platforms', '没有找到这个平台', '试试平台的英文名称，或清空筛选条件。')); return; }
  $('platform-list').replaceChildren(...items.map((item) => {
    const connect = item.mode === 'planned' ? el('span', { class: 'muted' }, '接入准备中') : button('添加账号', 'inline-link', () => openAccountDialog(null, item.id));
    const open = button('', 'icon-button', () => busy(open, async () => { await rpc('platforms.open', { platform: item.id }); }), 'external');
    open.title = `打开 ${item.name}`; open.setAttribute('aria-label', `打开 ${item.name}`);
    return el('article', { class: 'platform-card' }, el('div', { class: 'card-top' }, el('div', { class: 'platform-title' }, avatar(item.id, true), el('div', {}, el('h3', {}, item.name), el('div', { class: 'platform-kind' }, item.kind === 'product' ? '电商平台' : '视频平台'))), open), el('p', { class: 'platform-description' }, item.description || (item.mode === 'api' ? '官方 API 适配器已实现，实际执行需要授权并完成账号验证。' : item.mode === 'assisted' ? '准备内容并打开创作页面，在平台核对后手动提交。' : '已列入接入目录，当前没有可执行的发布适配器。')), el('div', { class: 'card-bottom' }, statePill(modeLabels[item.mode] || item.mode, item.mode === 'api' ? 'success' : item.mode === 'assisted' ? 'warning' : ''), connect));
  }));
}

function updateBridge(detail = {}) {
  currentBridge = { ...currentBridge, ...detail };
  $('bridge-state').textContent = currentBridge.connected ? '已连接' : detail.error ? '连接已中断' : '未连接';
  $('bridge-state').className = `state-pill ${currentBridge.connected ? 'success' : detail.error ? 'error' : ''}`;
  $('bridge-disconnect').disabled = !currentBridge.connected;
  $('bridge-connect').textContent = currentBridge.connected ? '重新配对' : '连接 Agent';
  if (detail.error) toast(prettyError(detail.error), true);
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
$('settings-open').addEventListener('click', () => { showError('settings-error', ''); $('settings-dialog').showModal(); });
document.querySelectorAll('button[data-theme]').forEach((node) => node.addEventListener('click', () => busy(node, async () => { await rpc('settings.save', { settings: { theme: node.dataset.theme } }); setTheme(node.dataset.theme); await refresh(); })));
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => { if (currentTheme === 'system') setTheme('system'); });
$('backup-export').addEventListener('click', () => busy($('backup-export'), exportBackup, { errorId: 'settings-error' }));
$('backup-file').addEventListener('change', async () => { showError('settings-error', ''); try { await importBackup($('backup-file').files[0]); } catch (error) { showError('settings-error', prettyError(error)); } finally { $('backup-file').value = ''; } });
$('bridge-form').addEventListener('submit', (event) => { event.preventDefault(); busy($('bridge-connect'), async () => { const result = await connectBridge({ url: $('bridge-url').value.trim(), token: $('bridge-token').value.trim() }); updateBridge(result || { connected: true }); $('bridge-token').value = ''; toast('Agent 桥接已连接。'); }); });
$('bridge-disconnect').addEventListener('click', () => busy($('bridge-disconnect'), async () => { const result = await disconnectBridge(); updateBridge(result || { connected: false }); toast('Agent 桥接已断开。'); }));
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

setTheme('light');
navigate(location.hash.slice(1) || 'compose', false);
renderTargets();
refresh();
setInterval(() => { if (!document.hidden && !document.querySelector('dialog[open]')) refresh(); }, 3000);
document.addEventListener('visibilitychange', () => { if (!document.hidden) refresh(); });
