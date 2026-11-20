#!/usr/bin/env node
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtemp, mkdir, readFile, rm, writeFile, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { PassThrough } from 'node:stream';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createBridge } from '../bridge/server.mjs';
import { runMcp } from '../bridge/mcp.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.MATRIXPOST_PLAYWRIGHT_MODULE || 'playwright');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const extensionPath = join(root, 'extension');
const outputPath = join(root, 'test-results');
const chromiumPath = process.env.MATRIXPOST_CHROMIUM || '/usr/bin/chromium';
const permissionMode = process.env.MATRIXPOST_PERMISSION_MODE || 'chrome-ui';
const runFile = promisify(execFile);
if (permissionMode === 'native' && process.env.MATRIXPOST_UNDER_XVFB !== '1') {
  // This wrapper owns a fresh X display and D-Bus session. It never controls the user's desktop.
  const code = await new Promise((resolveExit, reject) => {
    const child = spawn('xvfb-run', ['-a', '--server-args=-screen 0 1600x1200x24', 'dbus-run-session', '--', process.execPath, fileURLToPath(import.meta.url)], {
      env: { ...process.env, MATRIXPOST_UNDER_XVFB: '1', ACCESSIBILITY_ENABLED: '1', WAYLAND_DISPLAY: '', XDG_SESSION_TYPE: 'x11', XDG_CURRENT_DESKTOP: 'Xvfb' }, stdio: 'inherit',
    });
    child.on('error', reject);
    child.on('exit', (code) => resolveExit(code ?? 1));
  });
  process.exit(code);
}
const passphrase = `matrixpost-smoke-${randomUUID()}`;
const credentials = { consumerKey: 'ck_matrixpost_smoke_fixture', consumerSecret: 'cs_matrixpost_smoke_fixture' };
// Deterministic binary fixture for file ingestion and Blob persistence, not codec or playback validation.
const videoFixture = {
  name: 'matrixpost-storage-fixture.mp4',
  mimeType: 'video/mp4',
  buffer: Buffer.from(Array.from({ length: 1024 }, (_, index) => (index * 31 + 17) % 256)),
};
const videoFixtureHash = createHash('sha256').update(videoFixture.buffer).digest('hex');
const secrets = [passphrase, ...Object.values(credentials)];
const clean = (value) => secrets.reduce((text, secret) => text.replaceAll(secret, '[redacted]'), String(value));
const errors = [];
const externalAttempts = [];
const checks = [];
const report = { startedAt: new Date().toISOString(), browser: chromiumPath, permissionMode, checks, screenshots: [], realPlatformRequests: 0 };
let context;
let page;
let profilePath;
let bridge;

function checked(name, details = {}) { checks.push({ name, passed: true, ...details }); console.log(`PASS ${name}`); }

async function rpc(method, params = {}, source = 'ui') {
  return page.evaluate(async ({ method, params, source }) => {
    const client = await import('./core/client.js');
    return client.rpc(method, params, source);
  }, { method, params, source });
}

async function waitForDraft(title) {
  await page.waitForFunction(async (title) => {
    const result = await chrome.runtime.sendMessage({ channel: 'matrixpost', method: 'drafts.list', params: {}, source: 'ui' });
    return result?.ok && result.result.some((draft) => draft.title === title);
  }, title, { timeout: 10000 });
  return (await rpc('drafts.list')).find((draft) => draft.title === title);
}

async function assertStoredVideoFixture(assetId) {
  const stored = await page.evaluate(async (assetId) => {
    const { IndexedStore } = await import('./core/store.js');
    const record = await new IndexedStore().get('assets', assetId);
    if (!record) return null;
    const isBlob = record.blob instanceof Blob;
    return {
      id: record.id, name: record.name, size: record.size, type: record.type, isBlob,
      blobSize: record.blob?.size, blobType: record.blob?.type,
      bytes: isBlob ? Array.from(new Uint8Array(await record.blob.arrayBuffer())) : null,
    };
  }, assetId);
  assert.deepEqual(stored, {
    id: assetId, name: videoFixture.name, size: videoFixture.buffer.length, type: videoFixture.mimeType,
    isBlob: true, blobSize: videoFixture.buffer.length, blobType: videoFixture.mimeType,
    bytes: Array.from(videoFixture.buffer),
  }, 'IndexedDB must contain the exact selected file bytes and matching Blob metadata.');
}

async function navigate(view) {
  await page.locator(`[data-view="${view}"]`).click();
  await page.locator(`#view-${view}`).waitFor({ state: 'visible' });
}

async function guardWorker(worker) {
  if (!worker.url().startsWith('chrome-extension:')) return;
  await worker.evaluate(() => {
    if (globalThis.__matrixSmokeFetchGuard) return;
    globalThis.__matrixSmokeFetchGuard = true;
    globalThis.__matrixSmokeBlocked = [];
    const original = globalThis.fetch.bind(globalThis);
    globalThis.fetch = (resource, options) => {
      const url = new URL(typeof resource === 'string' ? resource : resource.url);
      if (['http:', 'https:'].includes(url.protocol) && url.hostname !== '127.0.0.1') {
        globalThis.__matrixSmokeBlocked.push({ origin: url.origin, method: options?.method || 'GET' });
        return Promise.reject(new Error('Browser smoke test forbids external requests.'));
      }
      return original(resource, options);
    };
  });
}

async function launch() {
  context = await chromium.launchPersistentContext(profilePath, {
    executablePath: chromiumPath,
    headless: permissionMode !== 'native',
    viewport: { width: 1440, height: 1050 },
    locale: 'zh-CN',
    ...(permissionMode === 'native' ? { env: { ...process.env, WAYLAND_DISPLAY: '', XDG_SESSION_TYPE: 'x11', XDG_CURRENT_DESKTOP: 'Xvfb' } } : {}),
    args: [
      `--disable-extensions-except=${extensionPath}`,
      `--load-extension=${extensionPath}`,
      '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
      ...(permissionMode === 'native' ? ['--force-renderer-accessibility', '--lang=en-US', '--ozone-platform=x11'] : []),
    ],
  });
  context.on('page', (opened) => {
    opened.on('pageerror', (error) => errors.push({ type: 'pageerror', message: clean(error.message) }));
    opened.on('console', (message) => {
      if (message.type() === 'error') errors.push({ type: 'console', message: clean(message.text()) });
    });
  });
  context.on('serviceworker', (worker) => { void guardWorker(worker).catch((error) => errors.push({ type: 'worker-guard', message: clean(error.message) })); });
  await context.route(/^https?:\/\//, (route) => {
    const url = new URL(route.request().url());
    if (url.hostname === '127.0.0.1') return route.continue();
    externalAttempts.push({ origin: url.origin, method: route.request().method() });
    return route.abort('blockedbyclient');
  });
  const worker = context.serviceWorkers().find((item) => item.url().startsWith('chrome-extension:'))
    || await context.waitForEvent('serviceworker', { predicate: (item) => item.url().startsWith('chrome-extension:'), timeout: 20000 });
  await guardWorker(worker);
  const extensionId = new URL(worker.url()).hostname;
  assert.match(extensionId, /^[a-p]{32}$/);
  page = await context.newPage();
  page.setDefaultTimeout(10000);
  await page.goto(`chrome-extension://${extensionId}/app.html`, { waitUntil: 'load' });
  await page.locator('#draft-title').waitFor({ state: 'visible' });
  await page.waitForFunction(() => document.querySelector('#target-accounts')?.children.length > 0, null, { timeout: 10000 });
  assert.equal(await page.locator('#connection-error').isVisible(), false, 'Extension boot must not display a connection error.');
  return { worker, extensionId };
}

async function grantOrigins(origins) {
  if (permissionMode !== 'native') {
    await grantOriginsInChromeSettings(origins);
    // Already granted permissions still exercise contains/request below, without an invisible native prompt.
  }
  // Exercise the real extension permissions API from a genuine click, without modifying or mocking it.
  await page.evaluate((origins) => {
    globalThis.__matrixSmokePermission = { pending: true };
    const button = document.createElement('button');
    button.id = 'matrix-smoke-permission';
    button.className = 'secondary-button';
    button.textContent = '授予测试所需站点权限';
    button.addEventListener('click', async () => {
      try { globalThis.__matrixSmokePermission = { pending: false, granted: await chrome.permissions.request({ origins }) }; }
      catch (error) { globalThis.__matrixSmokePermission = { pending: false, granted: false, message: error.message }; }
    }, { once: true });
    document.querySelector('.top-actions').append(button);
  }, origins);
  await page.locator('#matrix-smoke-permission').click();
  if (permissionMode === 'native') await acceptNativePermissionPrompt(origins);
  await page.waitForFunction(() => globalThis.__matrixSmokePermission?.pending === false, null, { timeout: 15000 });
  const result = await page.evaluate(() => globalThis.__matrixSmokePermission);
  await page.locator('#matrix-smoke-permission').evaluate((element) => element.remove());
  assert.equal(result.granted, true, result.message || 'Chromium did not grant the requested optional test origins.');
  assert.equal(await page.evaluate((origins) => chrome.permissions.contains({ origins }), origins), true);
}

async function inspectPermissionSettings() {
  const settings = await context.newPage();
  await settings.goto(`chrome://extensions/?id=${report.extensionId}`);
  await settings.locator('extensions-detail-view').waitFor({ state: 'visible', timeout: 10000 });
  const controls = await settings.locator('extensions-detail-view select, extensions-detail-view cr-button, extensions-detail-view cr-toggle').evaluateAll((items) => items.map((item) => ({
    tag: item.tagName, id: item.id, text: item.textContent.trim().slice(0, 100), visible: !!item.getBoundingClientRect().height,
    ...(item.tagName === 'SELECT' ? { value: item.value, options: [...item.options].map((option) => ({ value: option.value, text: option.text })) } : {}),
  })));
  const count = await settings.locator('extensions-runtime-host-permissions').count();
  report.permissionSettings = { runtimeHostSections: count, controls };
  await settings.screenshot({ path: join(outputPath, 'permissions-settings.png'), fullPage: true });
  if (!report.screenshots.includes('permissions-settings.png')) report.screenshots.push('permissions-settings.png');
  console.log(`PERMISSION_UI ${JSON.stringify(report.permissionSettings)}`);
  return { settings, count };
}

async function grantOriginsInChromeSettings(origins) {
  // These are Chromium's real settings controls, not injected developerPrivate calls.
  // Source: chromium/chromium chrome/browser/resources/extensions/runtime_host_permissions.html.ts
  // and runtime_hosts_dialog.html.ts. Optional-only extensions may not show this section yet.
  const { settings, count } = await inspectPermissionSettings();
  try {
    if (!count) throw new Error('This Chromium does not expose site-access settings before optional hosts are granted. Rerun with MATRIXPOST_PERMISSION_MODE=native for isolated Xvfb + native accessibility approval.');
    const section = settings.locator('extensions-runtime-host-permissions');
    const select = section.locator('select:visible').first();
    for (const origin of origins) {
      if (await page.evaluate((origin) => chrome.permissions.contains({ origins: [origin] }), origin)) continue;
      const options = await select.locator('option').evaluateAll((items) => items.map((item) => ({ value: item.value, text: item.text })));
      const specific = options.find((option) => option.value === 'ON_SPECIFIC_SITES');
      assert.ok(specific, `Chromium does not expose its documented specific-sites control: ${JSON.stringify(options)}`);
      if (await select.inputValue() !== specific.value) await select.selectOption(specific.value);
      else await section.locator('#add-host:visible, #add-site-button:visible').first().click();
      const dialog = settings.locator('extensions-runtime-hosts-dialog');
      await dialog.waitFor({ state: 'visible' });
      await dialog.locator('cr-input#input input').fill(origin);
      await dialog.locator('cr-button#submit').click();
      await dialog.waitFor({ state: 'hidden' });
      assert.equal(await page.evaluate((origin) => chrome.permissions.contains({ origins: [origin] }), origin), true, `Chrome settings did not grant ${origin}.`);
    }
    report.permissionsGrantedVia = 'Chrome extensions site-access UI';
  } finally {
    await settings.close();
    await page.bringToFront();
  }
}

async function assertNoOverflow(view) {
  const result = await page.evaluate(() => {
    const width = innerWidth;
    return {
      viewport: width,
      document: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth),
      offenders: [...document.querySelectorAll('main *')].filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && (rect.right > width + 2 || rect.left < -2) && getComputedStyle(element).position !== 'fixed';
      }).slice(0, 8).map((element) => ({ tag: element.tagName, id: element.id, class: element.className })),
    };
  });
  assert.ok(result.document <= result.viewport + 1, `${view} causes document overflow: ${JSON.stringify(result)}`);
  return { view, width: result.viewport, documentWidth: result.document };
}

async function acceptNativePermissionPrompt(origins) {
  assert.equal(process.env.MATRIXPOST_UNDER_XVFB, '1', 'Native approval is restricted to this script\'s isolated Xvfb display.');
  const nativePath = join(outputPath, 'permissions-native.png');
  report.nativePermission = { display: process.env.DISPLAY, xauthority: process.env.XAUTHORITY, pid: process.pid, origins, screenshot: nativePath };
  console.log(`NATIVE_PERMISSION_WAIT ${JSON.stringify(report.nativePermission)}`);
  // Capture browser chrome and the actual permission dialog, rather than only the web page.
  let captured = false;
  for (const [command, args] of [
    ['magick', ['import', '-display', process.env.DISPLAY, '-window', 'root', nativePath]],
    ['import', ['-display', process.env.DISPLAY, '-window', 'root', nativePath]],
    ['python3', ['-c', 'from PIL import ImageGrab\nimport os,sys\nImageGrab.grab(xdisplay=os.environ["DISPLAY"]).save(sys.argv[1])', nativePath]],
  ]) {
    try { await runFile(command, args, { timeout: 5000 }); captured = true; break; }
    catch { /* Try the next installed native screenshot backend. */ }
  }
  if (captured) report.screenshots.push('permissions-native.png');
  console.log(`NATIVE_PERMISSION_SCREENSHOT ${JSON.stringify({ captured, path: nativePath, display: process.env.DISPLAY })}`);
  // Inspect the screenshot and click the real Allow button on this isolated display.
  // Never guess coordinates or substitute a mock permission result.
  await page.waitForFunction(() => globalThis.__matrixSmokePermission?.pending === false, null, { timeout: 60000 });
  report.permissionsGrantedVia = 'Real Chromium permissions.request result on an isolated Xvfb desktop';
}

async function screenshot(name, { failure = false } = {}) {
  const path = join(outputPath, name);
  if (!failure) {
    await page.evaluate(() => window.scrollTo({ top: 0, left: 0, behavior: 'instant' }));
    try {
      await page.waitForFunction(() => document.querySelectorAll('.toast').length === 0, null, { timeout: 8000 });
    } catch (error) {
      if (error.name !== 'TimeoutError') throw error;
      (report.screenshotTransientTimeouts ||= []).push(name);
    }
    await page.evaluate(() => window.scrollTo({ top: 0, left: 0, behavior: 'instant' }));
  }
  await page.screenshot({ path, fullPage: true, animations: 'disabled' });
  report.screenshots.push(name);
}

async function assertNoExternalWork() {
  for (const worker of context.serviceWorkers()) {
    if (worker.url().startsWith('chrome-extension:')) externalAttempts.push(...await worker.evaluate(() => globalThis.__matrixSmokeBlocked || []));
  }
  assert.deepEqual(externalAttempts, [], 'No smoke-test action may attempt a real platform request.');
}

async function main() {
  for (const file of ['manifest.json', 'app.html', 'app.js', 'core/client.js', 'icons/16.png']) await access(join(extensionPath, file));
  const manifest = JSON.parse(await readFile(join(extensionPath, 'manifest.json'), 'utf8'));
  report.version = manifest.version;
  await mkdir(outputPath, { recursive: true });
  profilePath = await mkdtemp(join(tmpdir(), 'matrixpost-browser-smoke-'));
  let { extensionId } = await launch();
  report.extensionId = extensionId;
  const initial = await rpc('snapshot');
  assert.equal(initial.drafts.length, 0);
  assert.equal(initial.accounts.length, 0);
  assert.equal(initial.jobs.length, 0);
  checked('Real MV3 extension boots with empty local state');
  if (process.env.MATRIXPOST_PERMISSION_PROBE === '1') {
    const { settings } = await inspectPermissionSettings();
    await settings.close();
    report.status = 'probe_complete';
    return;
  }

  const videoTitle = '一段好内容，开启新的连接';
  await page.locator('#draft-title').fill(videoTitle);
  await page.locator('#draft-description').fill('记录灵感，用一次创作连接更多平台。\n这是一份本地浏览器验证草稿。');
  await page.locator('#draft-tags').fill('MatriXpost, 创作, 产品');
  await page.locator('#media-file').setInputFiles(videoFixture);
  await page.locator('#selected-assets .asset-row strong').filter({ hasText: videoFixture.name }).waitFor({ state: 'visible' });
  await page.waitForFunction(() => !document.querySelector('#upload-zone').classList.contains('busy'));
  assert.equal(await page.locator('#selected-assets .asset-row').count(), 1);
  await page.locator('#draft-title').focus();
  await page.waitForTimeout(3200); // The app refreshes snapshots every three seconds; it must preserve editor input.
  assert.equal(await page.locator('#draft-title').inputValue(), videoTitle);
  assert.equal(await page.locator('#draft-title').evaluate((element) => element === document.activeElement), true);
  await page.keyboard.press('Control+Enter');
  const videoDraft = await waitForDraft(videoTitle);
  assert.equal(videoDraft.kind, 'video');
  assert.deepEqual(videoDraft.tags, ['MatriXpost', '创作', '产品']);
  checked('UI draft save and Ctrl+Enter preserve focused editor across refresh');
  assert.equal(videoDraft.assetIds.length, 1, 'Saving the UI draft must retain its selected video asset reference.');
  const videoAssetId = videoDraft.assetIds[0];
  assert.equal(typeof videoAssetId, 'string');
  await assertStoredVideoFixture(videoAssetId);
  checked('UI video file ingestion preserves exact fixture bytes in IndexedDB', {
    bytes: videoFixture.buffer.length, sha256: videoFixtureHash, scope: 'File ingestion and storage only; no codec or playback validation.',
  });

  await page.locator('#vault-open').click();
  await page.locator('#vault-passphrase').fill(passphrase);
  await page.locator('#vault-confirm').fill(passphrase);
  await page.locator('#vault-submit').click();
  await page.locator('#vault-dialog').waitFor({ state: 'hidden' });
  assert.equal((await rpc('snapshot')).vault.unlocked, true);
  checked('Vault can be created through the extension UI');

  await navigate('accounts');
  await page.locator('#accounts-add').click();
  await page.locator('#account-platform').selectOption('bilibili');
  await page.locator('#account-label').fill('视频账号 · 本地测试');
  await page.locator('#account-save').click();
  await page.locator('#account-dialog').waitFor({ state: 'hidden' });
  await grantOrigins(['https://example.com/*', 'http://127.0.0.1/*']);
  await page.locator('#accounts-add').click();
  await page.locator('#account-platform').selectOption('woocommerce');
  await page.locator('#account-label').fill('示例店铺 · 本地测试');
  await page.locator('#account-endpoint').fill('https://example.com');
  await page.locator('#credential-consumerKey').fill(credentials.consumerKey);
  await page.locator('#credential-consumerSecret').fill(credentials.consumerSecret);
  await page.locator('#account-save').click();
  await page.locator('#account-dialog').waitFor({ state: 'hidden' });
  const accounts = await rpc('accounts.list');
  assert.equal(accounts.length, 2);
  const woo = accounts.find((account) => account.platform === 'woocommerce');
  assert.equal(woo.hasCredentials, true);
  assert.equal(accounts.find((account) => account.platform === 'bilibili').mode, 'browser');
  assert.ok(!JSON.stringify(accounts).includes(credentials.consumerSecret));
  checked('UI saves browser account reference and independently encrypted API credentials');

  const encryption = await page.evaluate(async ({ accountId, credentials }) => {
    const { IndexedStore } = await import('./core/store.js');
    const record = await new IndexedStore().get('secrets', accountId);
    return { encrypted: record?.envelope?.version === 1, leaks: Object.values(credentials).some((value) => JSON.stringify(record).includes(value)) };
  }, { accountId: woo.id, credentials });
  assert.deepEqual(encryption, { encrypted: true, leaks: false });
  const backup = await rpc('backup.export');
  assert.ok(!JSON.stringify(backup).includes(credentials.consumerKey));
  assert.ok(!JSON.stringify(backup).includes(credentials.consumerSecret));
  checked('Stored credential envelope and exported metadata contain no plaintext credentials');

  await navigate('compose');
  await page.locator('#new-draft').click();
  await page.locator('[data-kind="product"]').click();
  const productTitle = 'MatriXpost 创作者笔记本';
  await page.locator('#draft-title').fill(productTitle);
  await page.locator('#draft-description').fill('轻巧、简洁，让每一个好想法都有地方停留。\n本地测试资料，不会上架。');
  await page.locator('#product-price').fill('29.90');
  await page.locator('#product-quantity').fill('12');
  await page.locator('#product-sku').fill('MX-NOTE-001');
  await page.locator('#save-draft').click();
  const productDraft = await waitForDraft(productTitle);
  assert.equal(productDraft.product.price, '29.90');
  assert.equal(productDraft.product.quantity, 12);
  const preview = await rpc('publish.preview', { draftId: productDraft.id, accountIds: [woo.id], action: 'publish' });
  assert.equal(preview.valid, true);
  const jobs = await rpc('jobs.enqueue', { draftId: productDraft.id, accountIds: [woo.id], action: 'publish', scheduledAt: new Date(Date.now() + 3600000).toISOString() }, 'agent');
  assert.equal(jobs.length, 1);
  assert.equal(jobs[0].state, 'awaiting_approval');
  const approvalDenied = await page.evaluate(async (id) => {
    const { rpc } = await import('./core/client.js');
    try { await rpc('jobs.approve', { id }, 'agent'); return false; }
    catch (error) { return error.code === 'METHOD_DENIED'; }
  }, jobs[0].id);
  assert.equal(approvalDenied, true);
  await rpc('jobs.cancel', { id: jobs[0].id }, 'agent');
  assert.equal((await rpc('jobs.list'))[0].state, 'cancelled');
  checked('Product UI validates fields; AI publish waits for approval and cannot self-approve');

  await rpc('vault.lock');
  assert.equal((await rpc('snapshot')).vault.unlocked, false);
  const wrongPasswordRejected = await page.evaluate(async () => {
    const { rpc } = await import('./core/client.js');
    try { await rpc('vault.unlock', { passphrase: 'deliberately-incorrect-fixture' }); return false; }
    catch (error) { return error.code === 'VAULT_UNLOCK_FAILED'; }
  });
  assert.equal(wrongPasswordRejected, true);
  await rpc('vault.unlock', { passphrase });
  checked('Vault rejects an incorrect password and reopens with the correct password');

  report.bridgeEvents = [];
  bridge = createBridge({ extensionId, onEvent: (event) => {
    if (report.bridgeEvents.length >= 100) report.bridgeEvents.shift();
    report.bridgeEvents.push(event);
  } });
  const bridgeUrl = await bridge.listen(0);
  secrets.push(bridge.token);
  await navigate('ai');
  await page.locator('#bridge-url').fill(bridgeUrl);
  await page.locator('#bridge-token').fill(bridge.token);
  await page.locator('#bridge-connect').click();
  await page.waitForFunction(() => document.querySelector('#bridge-state')?.textContent === '已连接', null, { timeout: 10000 });
  const input = new PassThrough();
  const output = new PassThrough();
  let wire = '';
  output.on('data', (chunk) => { wire += chunk.toString(); });
  const mcp = runMcp({ input, output, token: bridge.token, port: Number(new URL(bridgeUrl).port) });
  input.end([
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'browser-smoke', version: '1' } } },
    { jsonrpc: '2.0', method: 'notifications/initialized' },
    { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'drafts_save', arguments: { draft: { kind: 'video', title: 'AI 桥接联通验证', description: '经真实扩展 Origin 和本机 MCP 保存的本地草稿。' } } } },
  ].map((message) => JSON.stringify(message)).join('\n') + '\n');
  let timeout;
  try { await Promise.race([mcp, new Promise((_resolve, reject) => { timeout = setTimeout(() => reject(new Error('Real browser MCP round trip timed out.')), 20000); })]); }
  finally { clearTimeout(timeout); }
  const replies = wire.trim().split('\n').map((line) => JSON.parse(line));
  const reply = replies.find((message) => message.id === 2);
  assert.equal(reply?.result?.isError, false, clean(JSON.stringify(reply)));
  assert.equal(reply.result.structuredContent.title, 'AI 桥接联通验证');
  assert.ok((await rpc('drafts.list')).some((draft) => draft.title === 'AI 桥接联通验证'));
  await page.locator('#bridge-disconnect').click();
  await bridge.close();
  bridge = null;
  checked('Real Chromium extension Origin completes MCP draft creation through the loopback bridge');

  await navigate('drafts');
  await page.locator('#draft-search').fill(videoTitle);
  await page.locator('#draft-list').getByRole('button', { name: '继续编辑' }).click();
  await page.locator('#view-compose').waitFor({ state: 'visible' });
  const targets = page.locator('#target-accounts input[type="checkbox"]');
  if (await targets.count()) await targets.first().check();
  await page.setViewportSize({ width: 1440, height: 1050 });
  await assertNoOverflow('compose-wide');
  await screenshot('workspace-wide.png');
  await page.setViewportSize({ width: 420, height: 900 });
  const responsive = [];
  responsive.push(await assertNoOverflow('compose-mobile'));
  await screenshot('workspace-mobile.png');
  for (const view of ['drafts', 'queue', 'accounts', 'platforms', 'ai']) {
    await navigate(view);
    responsive.push(await assertNoOverflow(`${view}-mobile`));
  }
  await page.setViewportSize({ width: 1440, height: 1050 });
  await navigate('accounts');
  await screenshot('accounts.png');
  checked('All workspace views avoid document overflow at 420px', { responsive });

  await assertNoExternalWork();
  await context.close();
  context = null;
  const restarted = await launch();
  assert.equal(restarted.extensionId, extensionId);
  const persisted = await rpc('snapshot');
  assert.equal(persisted.drafts.length, 3);
  assert.equal(persisted.accounts.length, 2);
  assert.ok(persisted.drafts.some((draft) => draft.id === videoDraft.id && draft.title === videoTitle));
  assert.deepEqual(persisted.drafts.find((draft) => draft.id === videoDraft.id).assetIds, [videoAssetId]);
  assert.equal(persisted.assets.length, 1);
  assert.equal(persisted.assets[0].id, videoAssetId);
  await assertStoredVideoFixture(videoAssetId);
  assert.ok(persisted.drafts.some((draft) => draft.id === productDraft.id && draft.product.price === '29.90'));
  assert.equal(persisted.jobs[0].state, 'cancelled');
  assert.equal(persisted.vault.configured, true);
  assert.equal(persisted.vault.unlocked, false, 'A full browser restart must clear the in-memory vault key.');
  await rpc('vault.unlock', { passphrase });
  const decrypts = await page.evaluate(async ({ accountId, credentials }) => {
    const { IndexedStore } = await import('./core/store.js');
    const { Vault } = await import('./core/vault.js');
    const store = new IndexedStore();
    const { envelope } = await store.get('secrets', accountId);
    const value = await new Vault(store, chrome.storage.session).open(envelope, accountId);
    return Object.entries(credentials).every(([key, expected]) => value[key] === expected);
  }, { accountId: woo.id, credentials });
  assert.equal(decrypts, true);
  checked('Full browser restart preserves drafts/accounts/video bytes/ciphertext and clears the unlocked vault key', {
    videoBytes: videoFixture.buffer.length, videoSha256: videoFixtureHash,
  });
  await assertNoExternalWork();
  assert.deepEqual(errors, [], 'The browser emitted page or console errors.');
  checked('No page errors, console errors, or platform network requests');
  report.status = 'passed';
}

try { await main(); }
catch (error) {
  report.status = 'failed';
  report.error = clean(error.stack || error.message);
  console.error(report.error);
  if (report.bridgeEvents) console.error(`BRIDGE_DIAGNOSTICS ${JSON.stringify(report.bridgeEvents)}`);
  if (page && !page.isClosed()) {
    try { await screenshot('failure.png', { failure: true }); } catch { /* Preserve the original failure without waiting or changing its viewport. */ }
  }
  process.exitCode = 1;
} finally {
  if (bridge) await bridge.close().catch(() => {});
  if (context) await context.close().catch(() => {});
  if (profilePath) await rm(profilePath, { recursive: true, force: true });
  report.finishedAt = new Date().toISOString();
  report.errors = errors;
  report.externalAttempts = externalAttempts;
  await mkdir(outputPath, { recursive: true });
  await writeFile(join(outputPath, 'browser-smoke.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify({ status: report.status, checks: checks.length, screenshots: report.screenshots, report: 'test-results/browser-smoke.json' }));
}
