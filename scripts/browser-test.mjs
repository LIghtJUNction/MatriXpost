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
  name: '周末手冲咖啡.mp4',
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

async function waitForDescription(targetPage, id, description) {
  await targetPage.waitForFunction(async ({ id, description }) => {
    const response = await chrome.runtime.sendMessage({ channel: 'matrixpost', method: 'drafts.list', params: {}, source: 'ui' });
    return response?.ok && response.result.some((draft) => draft.id === id && draft.description === description);
  }, { id, description }, { timeout: 10000 });
}

async function assertUsableForm(scope = '#editor-form') {
  const details = await page.locator(scope).evaluate((form) => {
    const visible = [...form.querySelectorAll('input:not([type="hidden"]):not([type="file"]), textarea, select, button')].filter((element) => {
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && getComputedStyle(element).visibility !== 'hidden';
    });
    return {
      controls: visible.length,
      unlabeled: visible.filter((element) => !element.labels?.length && !element.getAttribute('aria-label') && !element.textContent.trim()).map((element) => element.id),
      clipped: visible.filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.left < -1 || rect.right > innerWidth + 1;
      }).map((element) => element.id),
    };
  });
  assert.ok(details.controls > 0, `${scope} must expose visible controls.`);
  assert.deepEqual(details.unlabeled, [], `${scope} contains controls without accessible labels.`);
  assert.deepEqual(details.clipped, [], `${scope} contains controls outside the viewport.`);
  return details;
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
      offenders: [...document.querySelectorAll('main *, dialog[open] *')].filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && (rect.right > width + 2 || rect.left < -2) && getComputedStyle(element).position !== 'fixed';
      }).slice(0, 8).map((element) => ({ tag: element.tagName, id: element.id, class: element.className })),
      dialogs: [...document.querySelectorAll('dialog[open]')].map((element) => ({ id: element.id, width: element.clientWidth, scrollWidth: element.scrollWidth })),
    };
  });
  assert.ok(result.document <= result.viewport + 1, `${view} causes document overflow: ${JSON.stringify(result)}`);
  assert.ok(result.dialogs.every((dialog) => dialog.scrollWidth <= dialog.width + 1), `${view} causes horizontal dialog overflow: ${JSON.stringify(result.dialogs)}`);
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

  const videoTitle = '周末手冲咖啡：20 克粉的冲煮记录';
  const videoDescription = '周六早上试了新到的埃塞豆。20 克咖啡粉、300 克水，分三次注水。\n这份草稿用于本地验收，不会发布到平台。';
  await page.locator('#draft-title').fill(videoTitle);
  await page.locator('#draft-description').fill(videoDescription);
  await page.locator('#draft-tags').fill('手冲咖啡, 周末, 冲煮记录');
  await page.locator('#media-file').setInputFiles(videoFixture);
  await page.locator('#selected-assets .asset-row strong').filter({ hasText: videoFixture.name }).waitFor({ state: 'visible' });
  await page.waitForFunction(() => !document.querySelector('#upload-zone').classList.contains('busy'));
  assert.equal(await page.locator('#selected-assets .asset-row').count(), 1);
  await page.locator('#draft-title').focus();
  await page.waitForTimeout(3200); // The app refreshes snapshots every three seconds; it must preserve editor input.
  assert.equal(await page.locator('#draft-title').inputValue(), videoTitle);
  assert.equal(await page.locator('#draft-title').evaluate((element) => element === document.activeElement), true);
  const videoDraft = await waitForDraft(videoTitle);
  assert.equal(videoDraft.kind, 'video');
  assert.deepEqual(videoDraft.tags, ['手冲咖啡', '周末', '冲煮记录']);
  assert.equal(videoDraft.description, videoDescription);
  assert.equal(await page.locator('#editor-error').isVisible(), false);
  checked('Typing automatically saves the draft without moving focus across snapshot refresh');
  await page.keyboard.press('Tab');
  assert.equal(await page.locator('#draft-description').evaluate((element) => element === document.activeElement), true, 'Tab must move from the title to the description.');
  await page.keyboard.press('Control+Enter');
  await page.waitForFunction(() => document.querySelector('#save-draft')?.disabled === false);
  assert.equal((await rpc('drafts.list')).filter((draft) => draft.id === videoDraft.id).length, 1);
  assert.equal((await rpc('drafts.list')).find((draft) => draft.id === videoDraft.id).revision, videoDraft.revision, 'Saving unchanged content must keep its revision.');
  checked('Keyboard navigation and Ctrl+Enter save preserve the same draft revision');
  assert.equal(videoDraft.assetIds.length, 1, 'Saving the UI draft must retain its selected video asset reference.');
  const videoAssetId = videoDraft.assetIds[0];
  assert.equal(typeof videoAssetId, 'string');
  await assertStoredVideoFixture(videoAssetId);
  checked('UI video file ingestion preserves exact fixture bytes in IndexedDB', {
    bytes: videoFixture.buffer.length, sha256: videoFixtureHash, scope: 'File ingestion and storage only; no codec or playback validation.',
  });

  await page.reload({ waitUntil: 'load' });
  await page.waitForFunction((title) => document.querySelector('#draft-title')?.value === title, videoTitle);
  assert.equal(await page.locator('#draft-description').inputValue(), videoDescription);
  assert.equal(await page.locator('#selected-assets .asset-row').count(), 1);
  checked('Reload restores the saved editor and its selected video');

  const stalePage = await context.newPage();
  await stalePage.goto(`chrome-extension://${extensionId}/app.html`, { waitUntil: 'load' });
  await stalePage.waitForFunction((title) => document.querySelector('#draft-title')?.value === title, videoTitle);
  const latestDescription = `${videoDescription}\n补记：水温 92°C，最后一段注水放慢。`;
  await page.locator('#draft-description').fill(latestDescription);
  await waitForDescription(page, videoDraft.id, latestDescription);
  const secondWindowDescription = `${videoDescription}\n另一份笔记：下次试试 90°C。`;
  await stalePage.locator('#draft-description').fill(secondWindowDescription);
  await stalePage.locator('#editor-error').waitFor({ state: 'visible', timeout: 10000 });
  assert.equal((await rpc('drafts.list')).find((draft) => draft.id === videoDraft.id).description, latestDescription);
  assert.equal(await stalePage.locator('#draft-description').inputValue(), secondWindowDescription);
  await stalePage.locator('#save-copy').click();
  await stalePage.waitForFunction(async ({ id, description }) => {
    const response = await chrome.runtime.sendMessage({ channel: 'matrixpost', method: 'drafts.list', params: {}, source: 'ui' });
    return response?.ok && response.result.some((draft) => draft.id !== id && draft.description === description);
  }, { id: videoDraft.id, description: secondWindowDescription });
  assert.equal((await rpc('drafts.list')).find((draft) => draft.id === videoDraft.id).description, latestDescription);
  await stalePage.close();
  await page.bringToFront();
  checked('Two extension windows reject a stale revision and preserve the second edit as a separate draft');

  await page.locator('#vault-open').click();
  await page.locator('#vault-passphrase').fill(passphrase);
  await page.locator('#vault-confirm').fill(passphrase);
  await page.locator('#vault-submit').click();
  await page.locator('#vault-dialog').waitFor({ state: 'hidden' });
  assert.equal((await rpc('snapshot')).vault.unlocked, true);
  checked('Vault can be created through the extension UI');

  await page.locator('#target-add-account').click();
  await page.locator('#account-platform').selectOption('bilibili');
  await page.locator('#account-label').fill('周末咖啡日记');
  await assertUsableForm('#account-form');
  await screenshot('account-setup-wide.png');
  await page.setViewportSize({ width: 360, height: 800 });
  await assertNoOverflow('account-setup-360');
  await assertUsableForm('#account-form');
  await screenshot('account-setup-360.png');
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.locator('#account-save').click();
  await page.locator('#account-dialog').waitFor({ state: 'hidden' });
  const browserAccount = (await rpc('accounts.list')).find((account) => account.platform === 'bilibili');
  await page.waitForFunction((label) => [...document.querySelectorAll('#target-accounts input[type="checkbox"]')].some((element) => element.getAttribute('aria-label') === `选择 ${label}` && element.checked), browserAccount.label);
  assert.equal(await page.locator('#target-accounts').getByRole('checkbox', { name: `选择 ${browserAccount.label}`, exact: true }).isChecked(), true);
  checked('Adding an account from the editor keeps the draft and selects the new target');
  await navigate('accounts');
  await grantOrigins(['https://example.com/*', 'http://127.0.0.1/*']);
  await page.locator('#accounts-add').click();
  await page.locator('#account-platform').selectOption('woocommerce');
  await page.locator('#account-label').fill('小器物 · 测试店铺');
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
  const productTitle = '白色陶瓷杯 · 300 mL';
  await page.locator('#draft-title').fill(productTitle);
  await page.locator('#draft-description').fill('哑光白釉，容量 300 mL，杯口直径 8.5 cm。可放入洗碗机。\n本地验收资料，不会上架。');
  await page.locator('#product-price').fill('29.90');
  await page.locator('#product-quantity').fill('12');
  await page.locator('#product-sku').fill('CUP-WHITE-300');
  await page.locator('#save-draft').click();
  const productDraft = await waitForDraft(productTitle);
  assert.equal(productDraft.product.price, '29.90');
  assert.equal(productDraft.product.quantity, 12);
  await page.locator('#target-accounts').getByRole('checkbox', { name: `选择 ${woo.label}`, exact: true }).check();
  await page.locator('#publish-action').selectOption('publish');
  await page.locator('#preview-publish').click();
  await page.locator('#preview-dialog').waitFor({ state: 'visible' });
  assert.ok((await page.locator('#preview-summary').innerText()).includes(productTitle));
  assert.ok((await page.locator('#preview-targets').innerText()).includes(woo.label));
  assert.equal(await page.locator('#enqueue-confirm').isEnabled(), true);
  await screenshot('publish-check-wide.png');
  await page.setViewportSize({ width: 420, height: 900 });
  await assertNoOverflow('publish-check-420');
  await screenshot('publish-check-420.png');
  await page.setViewportSize({ width: 360, height: 800 });
  await assertNoOverflow('publish-check-360');
  await screenshot('publish-check-360.png');
  await page.locator('#preview-dialog .dialog-footer [data-close-dialog]').click();
  assert.equal((await rpc('jobs.list')).length, 0, 'Inspecting a publish plan must not create or execute a task.');
  await page.setViewportSize({ width: 1440, height: 1050 });
  checked('Publish preview shows the selected content/account and remains reviewable at 360px without enqueueing');
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
  await navigate('queue');
  await page.locator('.job-approval-preview').waitFor({ state: 'visible' });
  await screenshot('task-review-wide.png');
  await page.setViewportSize({ width: 420, height: 900 });
  await assertNoOverflow('task-review-420');
  await screenshot('task-review-420.png');
  await page.setViewportSize({ width: 1440, height: 1050 });
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
  await page.locator('#draft-list .draft-card').filter({ hasText: '补记：水温 92°C' }).getByRole('button', { name: '继续编辑' }).click();
  await page.locator('#view-compose').waitFor({ state: 'visible' });
  const targets = page.locator('#target-accounts input[type="checkbox"]');
  if (await targets.count()) await targets.first().check();
  assert.equal(await page.locator('#publish-action-field').isVisible(), false, 'A browser-assisted account must not offer an API publish action.');
  assert.equal(await page.locator('.schedule-options').isVisible(), false, 'A browser-assisted account must not offer unattended scheduling.');
  assert.equal((await page.locator('#preview-publish').innerText()).trim(), '准备到平台后台');
  await page.setViewportSize({ width: 1440, height: 1050 });
  await assertNoOverflow('compose-wide');
  await assertUsableForm();
  await screenshot('workspace-wide.png');
  await page.locator('#settings-open').click();
  await page.locator('#settings-dialog button[data-theme="dark"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
  await page.locator('#settings-dialog [data-close-dialog]').click();
  await assertNoOverflow('compose-dark');
  await screenshot('workspace-dark.png');
  await page.locator('#settings-open').click();
  await page.locator('#settings-dialog button[data-theme="light"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
  await page.locator('#settings-dialog [data-close-dialog]').click();
  await page.setViewportSize({ width: 420, height: 900 });
  const responsive = [];
  responsive.push(await assertNoOverflow('compose-mobile'));
  await assertUsableForm();
  await screenshot('workspace-mobile.png');
  for (const view of ['drafts', 'queue', 'accounts', 'platforms', 'ai']) {
    await navigate(view);
    responsive.push(await assertNoOverflow(`${view}-mobile`));
  }
  await page.setViewportSize({ width: 360, height: 800 });
  await navigate('compose');
  responsive.push(await assertNoOverflow('compose-360'));
  await assertUsableForm();
  await screenshot('workspace-360.png');
  for (const view of ['drafts', 'queue', 'accounts', 'platforms', 'ai']) {
    await navigate(view);
    responsive.push(await assertNoOverflow(`${view}-360`));
  }
  await page.setViewportSize({ width: 1440, height: 1050 });
  await navigate('accounts');
  await screenshot('accounts.png');
  checked('All six workspace views and editor controls remain usable at 360px and 420px', { responsive, assistedControlsHidden: true, darkThemeViaSettings: true });

  await navigate('drafts');
  await page.locator('#draft-search').fill(productTitle);
  await page.locator('#draft-list').getByRole('button', { name: '继续编辑' }).click();
  await page.locator('#view-compose').waitFor({ state: 'visible' });
  await page.locator('#target-accounts').getByRole('checkbox', { name: `选择 ${woo.label}`, exact: true }).check();
  assert.equal(await page.locator('#publish-action-field').isVisible(), true);
  assert.equal(await page.locator('.schedule-options').isVisible(), true);
  await page.locator('#publish-action').selectOption('draft');
  const rememberedSchedule = await page.evaluate(() => {
    const local = new Date(Date.now() + 6 * 3600000);
    const two = (value) => String(value).padStart(2, '0');
    return `${local.getFullYear()}-${two(local.getMonth() + 1)}-${two(local.getDate())}T${two(local.getHours())}:${two(local.getMinutes())}`;
  });
  if (await page.locator('.schedule-options').getAttribute('open') === null) await page.locator('.schedule-options > summary').click();
  await page.locator('#publish-time').fill(rememberedSchedule);
  await page.locator('#publish-time').press('Tab');

  // Seed interruption boundaries in the isolated test profile; no remote object is created.
  // The next real browser startup must run the production recovery logic against these records.
  const uncertainJobId = await page.evaluate(async (jobId) => {
    const { IndexedStore } = await import('./core/store.js');
    const store = new IndexedStore();
    const original = await store.get('jobs', jobId);
    original.checkpoint = { remoteId: '42', promote: true, stage: 'publish_existing' };
    await store.put('jobs', original);
    const uncertain = {
      ...structuredClone(original),
      id: `${jobId}-interrupted-fixture`,
      key: `${original.key}-interrupted-fixture`,
      draftId: `${original.draftId}-interrupted-fixture`,
      title: '棉麻收纳袋 · 创建结果待核对',
      action: 'draft', state: 'running',
      checkpoint: { currencyVerified: true, stage: 'create_product' },
      inFlight: { instance: 'previous-browser-fixture', at: new Date().toISOString() },
      error: null, result: null,
    };
    uncertain.content.title = uncertain.title;
    await store.put('jobs', uncertain);
    return uncertain.id;
  }, jobs[0].id);

  await assertNoExternalWork();
  await context.close();
  context = null;
  const restarted = await launch();
  assert.equal(restarted.extensionId, extensionId);
  const persisted = await rpc('snapshot');
  assert.equal(persisted.drafts.length, 4);
  assert.equal(persisted.accounts.length, 2);
  assert.ok(persisted.drafts.some((draft) => draft.id === videoDraft.id && draft.title === videoTitle));
  assert.equal(persisted.drafts.find((draft) => draft.id === videoDraft.id).description, latestDescription);
  assert.deepEqual(persisted.drafts.find((draft) => draft.id === videoDraft.id).assetIds, [videoAssetId]);
  assert.equal(persisted.assets.length, 1);
  assert.equal(persisted.assets[0].id, videoAssetId);
  await assertStoredVideoFixture(videoAssetId);
  assert.ok(persisted.drafts.some((draft) => draft.id === productDraft.id && draft.product.price === '29.90'));
  assert.equal(persisted.jobs.find((job) => job.id === jobs[0].id).state, 'cancelled');
  assert.equal(persisted.vault.configured, true);
  assert.equal(persisted.vault.unlocked, false, 'A full browser restart must clear the in-memory vault key.');
  await page.waitForFunction((title) => document.querySelector('#draft-title')?.value === title, productTitle);
  assert.equal(await page.locator('#draft-description').inputValue(), productDraft.description);
  assert.equal(await page.locator('#product-price').inputValue(), productDraft.product.price);
  assert.equal(await page.locator('#product-quantity').inputValue(), String(productDraft.product.quantity));
  assert.equal(await page.locator('#product-sku').inputValue(), productDraft.product.sku);
  assert.equal(await page.locator('#publish-action').inputValue(), 'draft');
  assert.equal(await page.locator('#publish-time').inputValue(), rememberedSchedule);
  assert.equal(await page.locator('#target-accounts').getByRole('checkbox', { name: `选择 ${woo.label}`, exact: true }).isChecked(), true);
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
  checked('Full browser restart restores the product editor, WooCommerce target, action, and local scheduled time');

  const interrupted = persisted.jobs.find((job) => job.id === uncertainJobId);
  assert.equal(interrupted.state, 'uncertain');
  assert.equal(interrupted.canRetry, false);
  assert.equal(interrupted.hasRemoteContent, false);
  const resumedRecord = persisted.jobs.find((job) => job.id === jobs[0].id);
  assert.equal(resumedRecord.hasRemoteContent, true);
  assert.equal(resumedRecord.canRetry, true);
  const duplicates = await rpc('jobs.enqueue', { draftId: productDraft.id, revision: productDraft.revision, accountIds: [woo.id], action: 'publish' }, 'agent');
  assert.equal(duplicates.length, 1);
  assert.equal(duplicates[0].id, jobs[0].id);
  assert.equal(duplicates[0].state, 'cancelled');
  const recovered = await rpc('jobs.retry', { id: jobs[0].id }, 'agent');
  assert.equal(recovered.state, 'awaiting_approval', 'AI must not bypass review when continuing a public publish.');
  assert.equal(recovered.hasRemoteContent, true);
  await rpc('jobs.cancel', { id: jobs[0].id }, 'agent');
  const unknownRetry = await page.evaluate(async (id) => {
    const { rpc } = await import('./core/client.js');
    try { await rpc('jobs.retry', { id }, 'agent'); return { rejected: false }; }
    catch (error) { return { rejected: true, code: error.code }; }
  }, uncertainJobId);
  assert.deepEqual(unknownRetry, { rejected: true, code: 'OUTCOME_UNCERTAIN' });
  await navigate('queue');
  await page.locator('.job-card').filter({ hasText: '创建结果待核对' }).waitFor({ state: 'visible' });
  const unknownCard = page.locator('.job-card').filter({ hasText: '创建结果待核对' });
  assert.equal(await unknownCard.getByRole('button', { name: /重试|继续此任务|继续任务/ }).count(), 0);
  const reviewLink = unknownCard.getByRole('link', { name: '到平台核对', exact: true });
  assert.equal(await reviewLink.getAttribute('href'), 'https://example.com/wp-admin/edit.php?post_type=product');
  assert.equal(await reviewLink.getAttribute('target'), '_blank');
  await screenshot('task-recovery-wide.png');
  await page.setViewportSize({ width: 420, height: 900 });
  await assertNoOverflow('task-recovery-420');
  await screenshot('task-recovery-420.png');
  await page.setViewportSize({ width: 360, height: 800 });
  await assertNoOverflow('task-recovery-360');
  await screenshot('task-recovery-360.png');
  checked('Browser restart protects unknown create outcomes and retains the original cancelled remote object', {
    scope: 'Persisted interruption fixtures in an isolated browser profile; no live platform objects.',
    unknownState: interrupted.state, duplicateJobIdPreserved: true, aiResumeRequiresApproval: true,
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
