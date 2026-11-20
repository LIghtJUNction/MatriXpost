#!/usr/bin/env node
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtemp, mkdir, cp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.MATRIXPOST_PLAYWRIGHT_MODULE || 'playwright');
const runFile = promisify(execFile);
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const baselineCommit = 'd9dacc4e88bd176a734e38bd0b23b96b480f7b09';
const chromiumPath = process.env.MATRIXPOST_CHROMIUM || '/usr/bin/chromium';
const reportPath = join(root, 'test-results', 'upgrade-smoke.json');
const passphrase = 'matrixpost-upgrade-' + randomUUID();
const credentials = { consumerKey: 'ck_upgrade_fixture', consumerSecret: 'cs_upgrade_fixture' };
const bytes = Array.from({ length: 4097 }, (_, i) => (i * 29 + 11) % 256);
const expectedAssetHash = createHash('sha256').update(Buffer.from(bytes)).digest('hex');
const secrets = [passphrase, ...Object.values(credentials)];
const clean = value => secrets.reduce((text, secret) => text.replaceAll(secret, '[redacted]'), String(value));
const checks = [], pageErrors = [], externalAttempts = [];
const report = {
  startedAt: new Date().toISOString(), baselineCommit, browser: chromiumPath, headless: true,
  checks, fixtureOnly: true,
  fixtureBoundary: 'API account and task metadata are explicit IndexedDB fixtures. Credentials are encrypted by the real 0.5.0 Vault. No host permissions are requested or mocked; no real accounts or platform publishing are used.',
  sameProfile: true, sameInstallationPath: true, realPlatformRequests: 0,
};
let temporaryPath, profilePath, extensionPath, context, page;
function checked(name, detail = {}) { checks.push({ name, passed: true, ...detail }); console.log('PASS ' + name); }

async function sourceDigest(directory) {
  const hash = createHash('sha256');
  async function walk(path) {
    const entries = await readdir(path, { withFileTypes: true });
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const full = join(path, entry.name);
      if (entry.isDirectory()) await walk(full);
      else {
        assert.ok(entry.isFile(), 'Extension source must consist of regular files.');
        hash.update(relative(directory, full)); hash.update('\0'); hash.update(await readFile(full)); hash.update('\0');
      }
    }
  }
  await walk(directory);
  return hash.digest('hex');
}

async function guardWorker(worker) {
  if (!worker.url().startsWith('chrome-extension:')) return;
  await worker.evaluate(() => {
    if (globalThis.__upgradeNetworkGuard) return;
    globalThis.__upgradeNetworkGuard = true;
    globalThis.__upgradeBlockedRequests = [];
    const original = globalThis.fetch.bind(globalThis);
    globalThis.fetch = (input, options) => {
      const value = typeof input === 'string' ? input : input.url;
      const url = new URL(value, globalThis.location.href);
      if (['http:', 'https:'].includes(url.protocol)) {
        globalThis.__upgradeBlockedRequests.push({ origin: url.origin, method: options?.method || 'GET' });
        return Promise.reject(new Error('Upgrade fixtures cannot contact external services.'));
      }
      return original(input, options);
    };
  });
}

async function launch() {
  context = await chromium.launchPersistentContext(profilePath, {
    executablePath: chromiumPath, headless: true, viewport: { width: 1280, height: 900 }, locale: 'zh-CN',
    args: [
      '--disable-extensions-except=' + extensionPath, '--load-extension=' + extensionPath,
      '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
      '--host-resolver-rules=MAP * ~NOTFOUND',
    ],
  });
  context.on('page', opened => {
    opened.on('pageerror', error => pageErrors.push(clean(error.message)));
  });
  context.on('serviceworker', worker => { void guardWorker(worker).catch(error => pageErrors.push(clean(error.message))); });
  await context.route(/^https?:\/\//, route => {
    const request = route.request();
    externalAttempts.push({ origin: new URL(request.url()).origin, method: request.method(), source: 'page' });
    return route.abort('blockedbyclient');
  });
  const worker = context.serviceWorkers().find(item => item.url().startsWith('chrome-extension:'))
    || await context.waitForEvent('serviceworker', { predicate: item => item.url().startsWith('chrome-extension:'), timeout: 20000 });
  await guardWorker(worker);
  const extensionId = new URL(worker.url()).hostname;
  assert.match(extensionId, /^[a-p]{32}$/);
  page = await context.newPage();
  await page.goto('chrome-extension://' + extensionId + '/app.html', { waitUntil: 'load' });
  await page.locator('#draft-title').waitFor({ state: 'visible', timeout: 15000 });
  return { extensionId, version: await page.evaluate(() => chrome.runtime.getManifest().version) };
}

async function closeBrowser() {
  if (!context) return;
  for (const worker of context.serviceWorkers()) {
    if (!worker.url().startsWith('chrome-extension:')) continue;
    const blocked = await worker.evaluate(() => globalThis.__upgradeBlockedRequests || []).catch(() => []);
    externalAttempts.push(...blocked.map(entry => ({ ...entry, source: 'service-worker' })));
  }
  await context.close(); context = null; page = null;
}

async function rpc(method, params = {}, source = 'ui') {
  return page.evaluate(async ({ method, params, source }) => {
    const { rpc } = await import('./core/client.js');
    return rpc(method, params, source);
  }, { method, params, source });
}

async function databaseDump() {
  return page.evaluate(async () => {
    const { IndexedStore } = await import('./core/store.js');
    const store = new IndexedStore(), result = {};
    for (const name of ['drafts', 'accounts', 'jobs', 'assets', 'settings', 'secrets']) {
      const records = await store.all(name);
      records.sort((a, b) => a.id.localeCompare(b.id));
      result[name] = await Promise.all(records.map(async record => {
        if (name !== 'assets') return record;
        const { blob, ...metadata } = record;
        if (!(blob instanceof Blob)) throw new Error('Asset is not a Blob.');
        const data = await blob.arrayBuffer();
        const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', data));
        return { ...metadata, blob: { type: blob.type, size: blob.size,
          sha256: Array.from(hash, byte => byte.toString(16).padStart(2, '0')).join(''),
          bytes: Array.from(new Uint8Array(data)) } };
      }));
    }
    return result;
  });
}

try {
  await mkdir(dirname(reportPath), { recursive: true });
  temporaryPath = await mkdtemp(join(tmpdir(), 'matrixpost-upgrade-'));
  profilePath = join(temporaryPath, 'profile');
  const installationRoot = join(temporaryPath, 'installed');
  extensionPath = join(installationRoot, 'extension');
  await mkdir(installationRoot);
  const archive = join(temporaryPath, 'baseline.tar');
  await runFile('git', ['archive', '--format=tar', '--output=' + archive, baselineCommit, 'extension'], { cwd: root });
  await runFile('tar', ['-xf', archive, '-C', installationRoot]);
  const original = await launch();
  assert.equal(original.version, '0.5.0');
  report.originalVersion = original.version;
  report.originalExtensionId = original.extensionId;
  checked('Loaded the actual 0.5.0 extension from its immutable Git commit');

  await rpc('vault.setup', { passphrase });
  const created = await page.evaluate(async ({ bytes, credentials }) => {
    const { rpc, uploadAsset } = await import('./core/client.js');
    const { IndexedStore } = await import('./core/store.js');
    const { Vault } = await import('./core/vault.js');
    const { now, uuid, jobKey, safeAccount } = await import('./core/model.js');
    const store = new IndexedStore(), vault = new Vault(store, chrome.storage.session);
    const asset = await uploadAsset(new File([new Uint8Array(bytes)], 'upgrade-fixture.mp4', { type: 'video/mp4' }));
    let video = await rpc('drafts.save', { draft: { kind: 'video', title: '升级保留视频 🎬',
      description: '保存于旧版本的素材和文字。', tags: ['upgrade-fixture'], assetIds: [asset.id] } });
    video = await rpc('drafts.save', { draft: { ...video, description: video.description + '\n第二次修订。' } });
    const product = title => ({ kind: 'product', title, description: '升级验收商品，不会发布。',
      product: { price: '19.50', currency: 'USD', quantity: 3, sku: 'UPGRADE-FIXTURE', images: [] } });
    const cancelledDraft = await rpc('drafts.save', { draft: product('已取消任务的商品') });
    const uncertainDraft = await rpc('drafts.save', { draft: product('待核对任务的商品') });
    await rpc('settings.save', { settings: { theme: 'dark' } });
    // Explicit fixtures bypass only account enrollment, which needs optional host permission.
    // Encryption and persistence still use the unmodified 0.5.0 implementation.
    const account = { id: uuid(), platform: 'woocommerce', label: 'Upgrade fixture — no real store',
      endpoint: 'https://upgrade-fixture.example.com', remoteAccountId: '', mode: 'api', hasCredentials: true,
      revision: 1, createdAt: now(), updatedAt: now() };
    const envelope = await vault.seal(credentials, account.id);
    const job = (draft, state, action, checkpoint, error) => ({
      id: uuid(), key: jobKey(draft, account, action), draftId: draft.id, draftRevision: draft.revision,
      accountId: account.id, accountRevision: account.revision, accountLabel: account.label,
      platform: account.platform, title: draft.title, kind: draft.kind, action, mode: 'api',
      state, source: 'ui', scheduledAt: now(), createdAt: now(), updatedAt: now(), attempts: 1,
      content: structuredClone(draft), accountSnapshot: safeAccount(account), checkpoint, error, result: null, inFlight: null,
    });
    const cancelled = job(cancelledDraft, 'cancelled', 'publish',
      { remoteId: '8123', promote: true, stage: 'publish_existing' }, null);
    const uncertain = job(uncertainDraft, 'uncertain', 'draft',
      { currencyVerified: true, storeCurrency: 'USD', stage: 'create_product' },
      { code: 'NETWORK_ERROR', message: 'Upgrade fixture: result requires checking.', uncertain: true });
    await store.batch([
      { store: 'accounts', value: account }, { store: 'secrets', value: { id: account.id, envelope } },
      { store: 'jobs', value: cancelled }, { store: 'jobs', value: uncertain },
    ]);
    return { assetId: asset.id, accountId: account.id, videoId: video.id,
      cancelledJobId: cancelled.id, uncertainJobId: uncertain.id };
  }, { bytes, credentials });
  const before = await databaseDump();
  assert.equal(before.drafts.length, 3);
  assert.equal(before.drafts.find(draft => draft.id === created.videoId).revision, 2);
  assert.equal(before.assets[0].blob.sha256, expectedAssetHash);
  assert.deepEqual(before.assets[0].blob.bytes, bytes);
  const persistedText = JSON.stringify(before);
  for (const secret of secrets) assert.ok(!persistedText.includes(secret), 'Persistent records must not contain plaintext test credentials.');
  assert.equal(await page.evaluate(async () => Boolean((await chrome.storage.session.get('matrixpostKey')).matrixpostKey)), true);
  assert.deepEqual((await page.evaluate(() => chrome.permissions.getAll())).origins, []);
  checked('Created 0.5.0 drafts, Blob, encrypted API fixture and cancelled/uncertain task snapshots', {
    drafts: before.drafts.length, accounts: before.accounts.length, jobs: before.jobs.length,
    assetBytes: bytes.length, assetSha256: expectedAssetHash, hostPermissionsGranted: 0,
  });

  await closeBrowser();
  await rm(extensionPath, { recursive: true });
  await cp(join(root, 'extension'), extensionPath, { recursive: true });
  report.targetExtensionSha256 = await sourceDigest(extensionPath);
  const currentManifest = JSON.parse(await readFile(join(extensionPath, 'manifest.json'), 'utf8'));
  assert.equal(currentManifest.version, '0.6.0');
  const upgraded = await launch();
  report.upgradedVersion = upgraded.version;
  report.upgradedExtensionId = upgraded.extensionId;
  assert.equal(upgraded.extensionId, original.extensionId);
  assert.equal(upgraded.version, currentManifest.version);
  // Unpacked extensions require their normal Reload action after files change.
  // Restarting Chromium alone can retain the previous service-worker module cache.
  const afterRestartSnapshot = await rpc('snapshot');
  report.backgroundVersionBeforeReload = afterRestartSnapshot.version;
  assert.deepEqual(afterRestartSnapshot.vault, { configured: true, unlocked: false });
  assert.equal(await page.evaluate(async () => Boolean((await chrome.storage.session.get('matrixpostKey')).matrixpostKey)), false);
  report.sessionClearedBeforeReload = true;
  const settings = await context.newPage();
  await settings.goto('chrome://extensions/?id=' + upgraded.extensionId);
  await settings.locator('extensions-detail-view').waitFor({ state: 'visible', timeout: 10000 });
  report.reloadControls = await settings.locator('extensions-detail-view cr-icon-button, extensions-detail-view cr-button, extensions-detail-view button, extensions-toolbar cr-toggle').evaluateAll(elements => elements.map(element => ({
    tag: element.tagName, id: element.id, label: element.getAttribute('aria-label'), title: element.getAttribute('title'),
    text: element.textContent.trim().slice(0, 100), visible: Boolean(element.getBoundingClientRect().height),
    checked: element.hasAttribute('checked')
  })));
  const developerMode = settings.locator('extensions-toolbar #devMode');
  if (await developerMode.getAttribute('checked') === null) await developerMode.click();
  assert.equal(await developerMode.evaluate(element => element.hasAttribute('checked')), true);
  report.developerModeEnabledVia = 'Chromium extensions toolbar toggle';
  const reloadControl = report.reloadControls.find(control => control.visible && /reload|重新加载/i.test([control.label,control.title,control.text].join(' ')));
  if (!reloadControl?.id) throw new Error('Reload control requires inspection: ' + JSON.stringify(report.reloadControls));
  await settings.locator('extensions-detail-view #' + reloadControl.id).click();
  page = await context.newPage();
  const reloadDeadline = Date.now() + 10000;
  report.reloadNavigationRetries = 0;
  for (;;) {
    try {
      await page.goto('chrome-extension://' + upgraded.extensionId + '/app.html', { waitUntil: 'load' });
      break;
    } catch (error) {
      if (!error.message.includes('ERR_BLOCKED_BY_CLIENT') || Date.now() >= reloadDeadline) {
        report.reloadFailureText = await settings.locator('extensions-detail-view div, extensions-detail-view span').evaluateAll(elements =>
          [...new Set(elements.filter(element => element.children.length === 0 && element.getBoundingClientRect().height)
            .map(element => element.textContent.trim()).filter(Boolean))].slice(0, 80));
        throw error;
      }
      report.reloadNavigationRetries++;
      await new Promise(resolve => setTimeout(resolve, 200));
    }
  }
  await page.locator('#draft-title').waitFor({ state: 'visible', timeout: 15000 });
  await page.waitForFunction(async expectedVersion => {
    try {
      const response = await chrome.runtime.sendMessage({ channel: 'matrixpost', method: 'snapshot', params: {}, source: 'ui' });
      return response?.ok && response.result.version === expectedVersion;
    } catch { return false; }
  }, currentManifest.version, { timeout: 15000 });
  for (const worker of context.serviceWorkers()) await guardWorker(worker);
  report.backgroundVersionAfterReload = (await rpc('snapshot')).version;
  assert.equal(report.backgroundVersionAfterReload, currentManifest.version);
  report.reloadMechanism = 'Normal unpacked extension Reload button in chrome://extensions; no profile or database reset.';
  checked('Reopened and reloaded 0.6.0 in the same profile and installation path with the same extension ID', {
    backgroundVersionBeforeReload: report.backgroundVersionBeforeReload,
    backgroundVersionAfterReload: report.backgroundVersionAfterReload,
  });

  const after = await databaseDump();
  assert.deepEqual(after, before, 'All six IndexedDB stores must survive the upgrade without record changes.');
  checked('Preserved all six IndexedDB stores including exact ciphertext and task checkpoints', {
    stores: Object.fromEntries(Object.entries(after).map(([name, records]) => [name, records.length])),
  });
  const asset = after.assets.find(record => record.id === created.assetId);
  assert.deepEqual(asset.blob.bytes, bytes);
  assert.equal(asset.blob.sha256, expectedAssetHash);
  assert.equal(asset.blob.type, 'video/mp4');
  checked('Preserved the original video Blob byte for byte', { bytes: bytes.length, sha256: expectedAssetHash });

  const locked = await rpc('snapshot');
  assert.deepEqual(locked.vault, { configured: true, unlocked: false });
  assert.equal(await page.evaluate(async () => Boolean((await chrome.storage.session.get('matrixpostKey')).matrixpostKey)), false);
  await rpc('vault.unlock', { passphrase });
  const decrypted = await page.evaluate(async accountId => {
    const { IndexedStore } = await import('./core/store.js');
    const { Vault } = await import('./core/vault.js');
    const store = new IndexedStore();
    return new Vault(store, chrome.storage.session).open((await store.get('secrets', accountId)).envelope, accountId);
  }, created.accountId);
  assert.deepEqual(decrypted, credentials);
  checked('Cleared the browser session key and decrypted old credentials with the original passphrase');

  const jobs = await rpc('jobs.list');
  const cancelled = jobs.find(job => job.id === created.cancelledJobId);
  const uncertain = jobs.find(job => job.id === created.uncertainJobId);
  assert.equal(cancelled.state, 'cancelled');
  assert.equal(cancelled.remoteId, '8123');
  assert.equal(cancelled.hasRemoteContent, true);
  assert.equal(cancelled.canRetry, true);
  assert.equal(uncertain.state, 'uncertain');
  assert.equal(uncertain.canRetry, false);
  const retryError = await page.evaluate(async id => {
    const { rpc } = await import('./core/client.js');
    try { await rpc('jobs.retry', { id }); return null; } catch (error) { return error.code; }
  }, created.uncertainJobId);
  assert.equal(retryError, 'OUTCOME_UNCERTAIN');
  assert.deepEqual(await databaseDump(), before, 'Opening and unlocking the upgraded extension must not dispatch these old tasks.');
  checked('Kept cancelled work stopped and unknown creation results blocked without issuing a new task');

  assert.deepEqual((await page.evaluate(() => chrome.permissions.getAll())).origins, []);
  await closeBrowser();
  assert.deepEqual(pageErrors, []);
  assert.deepEqual(externalAttempts, []);
  checked('Finished with no page errors, no host grants and no platform network attempts');
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.error = { name: error.name, message: clean(error.message), stack: clean(error.stack) };
  process.exitCode = 1;
} finally {
  try { await closeBrowser(); } catch (error) {
    report.passed = false; report.closeError = clean(error.message); process.exitCode = 1;
  }
  report.pageErrors = pageErrors;
  report.blockedExternalAttempts = externalAttempts;
  report.completedAt = new Date().toISOString();
  await mkdir(dirname(reportPath), { recursive: true });
  await writeFile(reportPath, JSON.stringify(report, null, 2) + '\n');
  if (temporaryPath) await rm(temporaryPath, { recursive: true, force: true });
  console.log((report.passed ? 'PASS' : 'FAIL') + ' upgrade smoke: ' + reportPath);
  if (report.error) console.error(report.error.message);
}
