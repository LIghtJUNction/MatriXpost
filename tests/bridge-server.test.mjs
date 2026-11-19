import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { setTimeout as delay } from 'node:timers/promises';
import { createBridge } from '../bridge/server.mjs';

const extensionId = 'abcdefghijklmnopabcdefghijklmnop';
const origin = `chrome-extension://${extensionId}`;

async function setup(t, options = {}) {
  const bridge = createBridge({ extensionId, ...options });
  const url = await bridge.listen(0);
  t.after(() => bridge.close());
  const request = (path, { body, ext = false, headers = {}, ...init } = {}) => fetch(`${url}${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${bridge.token}`, ...(ext ? { Origin: origin } : {}), ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}), ...headers },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const enqueue = (params = {}, init = {}) => request('/v1/call', { method: 'POST', body: { method: 'drafts.list', params }, ...init });
  const poll = async () => (await request('/v1/poll', { ext: true })).json();
  return { bridge, url, request, enqueue, poll };
}

async function until(predicate) {
  for (let tries = 0; tries < 100; tries++) {
    if (predicate()) return;
    await delay(5);
  }
  throw new Error('Test condition timed out.');
}

test('requires an exact extension ID and a sufficiently long pairing token', () => {
  assert.throws(() => createBridge(), /extension ID/);
  assert.throws(() => createBridge({ extensionId: '*' }), /extension ID/);
  assert.throws(() => createBridge({ extensionId, token: 'short' }), /256 bits/);
});

test('binds loopback and rejects missing credentials, hostile origins, and rebound hosts', async (t) => {
  const { bridge, url, request } = await setup(t);
  assert.equal(bridge.server.address().address, '127.0.0.1');
  assert.equal((await fetch(`${url}/health`)).status, 401);
  assert.equal((await request('/health', { headers: { Authorization: 'Bearer incorrect' } })).status, 401);
  assert.equal((await request('/health', { headers: { Origin: 'https://evil.example' } })).status, 403);
  assert.equal((await request('/health', { headers: { Origin: 'null' } })).status, 403);
  const hostileHostStatus = await new Promise((resolve, reject) => {
    const req = http.get(`${url}/health`, { headers: { Host: 'attacker.example', Authorization: `Bearer ${bridge.token}` } }, (response) => {
      response.resume();
      response.on('end', () => resolve(response.statusCode));
    });
    req.on('error', reject);
  });
  assert.equal(hostileHostStatus, 403);
  assert.equal((await request('/health', { headers: { Referer: 'https://evil.example/page' } })).status, 403);
  assert.equal((await request('/health')).status, 200);
});

test('permits only exact-origin preflight and never exposes state during preflight', async (t) => {
  const { request, bridge } = await setup(t);
  const response = await request('/v1/poll', { method: 'OPTIONS', ext: true, headers: { Authorization: '', 'Access-Control-Request-Method': 'GET', 'Access-Control-Request-Headers': 'authorization, content-type' } });
  assert.equal(response.status, 204);
  assert.equal(response.headers.get('access-control-allow-origin'), origin);
  assert.equal(await response.text(), '');
  assert.equal(bridge.pendingCount, 0);
  assert.equal((await request('/v1/poll', { method: 'OPTIONS', ext: true, headers: { 'Access-Control-Request-Method': 'DELETE' } })).status, 403);
  assert.equal((await request('/v1/poll', { method: 'OPTIONS', headers: { 'Access-Control-Request-Method': 'GET' } })).status, 403);
});

test('extension-only routes reject local callers even with the pairing token', async (t) => {
  const { request } = await setup(t);
  assert.equal((await request('/v1/poll')).status, 403);
  assert.equal((await request('/v1/results', { method: 'POST', body: { id: 'id', result: {} } })).status, 403);
});

test('delivers an authenticated call exactly once and rejects replayed results', async (t) => {
  const { request, enqueue, poll, bridge } = await setup(t);
  const awaiting = enqueue();
  await until(() => bridge.pendingCount === 1);
  const { requests } = await poll();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].source, 'agent');
  assert.equal(requests[0].method, 'drafts.list');
  assert.ok(requests[0].expiresAt > Date.now());
  assert.deepEqual((await poll()).requests, []);
  const value = { id: requests[0].id, result: [{ id: 'draft-one', title: 'Sample' }] };
  assert.equal((await request('/v1/results', { method: 'POST', ext: true, body: value })).status, 200);
  assert.deepEqual(await (await awaiting).json(), { result: value.result });
  assert.equal((await request('/v1/results', { method: 'POST', ext: true, body: value })).status, 404);
  assert.equal(bridge.pendingCount, 0);
});

test('removes a timed-out unsent call so reconnecting cannot execute it later', async (t) => {
  const { enqueue, poll, bridge } = await setup(t, { requestTimeoutMs: 30 });
  const response = await enqueue();
  assert.equal(response.status, 504);
  assert.deepEqual((await response.json()).error.uncertain, false);
  assert.deepEqual((await poll()).requests, []);
  assert.equal(bridge.pendingCount, 0);
});

test('reports uncertainty when the extension took a request but did not return its outcome', async (t) => {
  const { request, enqueue, poll, bridge } = await setup(t, { requestTimeoutMs: 100 });
  const awaiting = enqueue();
  await until(() => bridge.pendingCount === 1);
  const { requests } = await poll();
  const response = await awaiting;
  assert.equal(response.status, 504);
  const payload = await response.json();
  assert.equal(payload.error.code, 'OUTCOME_UNKNOWN');
  assert.equal(payload.error.uncertain, true);
  assert.equal((await request('/v1/results', { method: 'POST', ext: true, body: { id: requests[0].id, result: {} } })).status, 404);
});

test('cancelling a waiting client removes undispatched work', async (t) => {
  const { enqueue, poll, bridge } = await setup(t);
  const controller = new AbortController();
  const awaiting = enqueue({}, { signal: controller.signal }).catch(() => null);
  await until(() => bridge.pendingCount === 1);
  controller.abort();
  await awaiting;
  await until(() => bridge.pendingCount === 0);
  assert.deepEqual((await poll()).requests, []);
});

test('caps pending work, rejects unsupported methods and caller-supplied authority fields', async (t) => {
  const { request, enqueue, bridge } = await setup(t, { maxPending: 1 });
  assert.equal((await request('/v1/call', { method: 'POST', body: { method: 'vault.unlock', params: {} } })).status, 403);
  assert.equal((await request('/v1/call', { method: 'POST', body: { method: 'jobs.enqueue', params: { draftId: 'a', accountIds: ['b'], action: 'publish', source: 'user' } } })).status, 400);
  const awaiting = enqueue();
  await until(() => bridge.pendingCount === 1);
  assert.equal((await enqueue()).status, 429);
  await bridge.close();
  assert.equal((await awaiting).status, 503);
});

test('limits payloads and validates JSON and target IDs before queueing', async (t) => {
  const { request, bridge, url } = await setup(t, { maxBodyBytes: 1024 });
  assert.equal((await request('/v1/call', { method: 'POST', body: { method: 'drafts.save', params: { draft: { kind: 'video', title: 'x'.repeat(1100) } } } })).status, 413);
  const invalid = await fetch(`${url}/v1/call`, { method: 'POST', headers: { Authorization: `Bearer ${bridge.token}`, 'Content-Type': 'application/json' }, body: '{invalid}' });
  assert.equal(invalid.status, 400);
  assert.equal((await request('/v1/call', { method: 'POST', body: { method: 'jobs.enqueue', params: { draftId: 'x', accountIds: ['same', 'same'], action: 'draft' } } })).status, 400);
  assert.equal(bridge.pendingCount, 0);
});

test('removes secrets from extension responses before returning them to an AI client', async (t) => {
  const { request, enqueue, poll, bridge } = await setup(t);
  const awaiting = enqueue();
  await until(() => bridge.pendingCount === 1);
  const { requests } = await poll();
  await request('/v1/results', { method: 'POST', ext: true, body: { id: requests[0].id, result: {
    accounts: [{ id: 'one', credentials: { accessToken: 'not-a-real-token' }, note: `token=${bridge.token}` }],
  } } });
  const raw = await (await awaiting).text();
  assert.ok(!raw.includes(bridge.token));
  assert.ok(!raw.includes('not-a-real-token'));
  assert.equal(JSON.parse(raw).result.accounts[0].credentials, '[redacted]');
});

test('validates returned errors and propagates a confirmed extension rejection', async (t) => {
  const { request, enqueue, poll, bridge } = await setup(t);
  const awaiting = enqueue();
  await until(() => bridge.pendingCount === 1);
  const { requests } = await poll();
  assert.equal((await request('/v1/results', { method: 'POST', ext: true, body: { id: requests[0].id, error: 'bad' } })).status, 400);
  const error = { code: 'VAULT_LOCKED', message: 'Unlock the vault in the extension.', uncertain: false };
  await request('/v1/results', { method: 'POST', ext: true, body: { id: requests[0].id, error } });
  assert.deepEqual(await (await awaiting).json(), { error });
});
