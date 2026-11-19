import test from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { mkdtemp, writeFile, chmod, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { runMcp } from '../bridge/mcp.mjs';
import { readPrivateToken } from '../bridge/cli.mjs';
import { createBridge } from '../bridge/server.mjs';

const token = 'a'.repeat(64);
const init = (version = '2025-06-18') => ({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: version, capabilities: {}, clientInfo: { name: 'fixture', version: '1' } } });
const request = (id, method, params) => ({ jsonrpc: '2.0', id, method, ...(params === undefined ? {} : { params }) });

async function conversation(messages, { fetchImpl, chunks } = {}) {
  const input = new PassThrough();
  const output = new PassThrough();
  let text = '';
  output.setEncoding('utf8');
  output.on('data', (chunk) => { text += chunk; });
  const run = runMcp({ input, output, token, fetchImpl });
  const wire = messages.map((message) => typeof message === 'string' ? message : JSON.stringify(message)).join('\n') + '\n';
  if (chunks) {
    for (const chunk of chunks(wire)) input.write(chunk);
    input.end();
  } else input.end(wire);
  await run;
  return text.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
}

test('negotiates all declared protocol versions and lists nine discoverable tools', async () => {
  for (const version of ['2025-06-18', '2025-03-26', '2024-11-05']) {
    const replies = await conversation([init(version), { jsonrpc: '2.0', method: 'notifications/initialized' }, request(2, 'tools/list')]);
    assert.equal(replies.find((reply) => reply.id === 1).result.protocolVersion, version);
    const tools = replies.find((reply) => reply.id === 2).result.tools;
    assert.equal(tools.length, 9);
    assert.ok(tools.some((tool) => tool.name === 'jobs_enqueue'));
    assert.ok(tools.every((tool) => tool.inputSchema.type === 'object'));
    assert.ok(tools.every((tool) => !tool.name.includes('vault') && !tool.name.includes('credentials')));
  }
});

test('requires initialization before tools and returns protocol errors for unknown methods', async () => {
  const replies = await conversation([request(0, 'tools/list'), init(), request(2, 'unknown'), request(3, 'tools/call', { name: 'vault_unlock', arguments: {} })]);
  assert.equal(replies.find((reply) => reply.id === 0).error.code, -32002);
  assert.equal(replies.find((reply) => reply.id === 2).error.code, -32601);
  assert.equal(replies.find((reply) => reply.id === 3).error.code, -32602);
});

test('handles fragmented UTF-8 JSON, malformed input, and notifications without corrupting stdout', async () => {
  const replies = await conversation(['{bad json}', init(), { jsonrpc: '2.0', method: 'notifications/initialized' }, request(2, 'ping'), request(3, 'tools/call', { name: 'drafts_save', arguments: { draft: { kind: 'video', title: '中文标题' } } })], {
    fetchImpl: async (_url, options) => new Response(JSON.stringify({ result: JSON.parse(options.body).params.draft })),
    chunks: (wire) => {
      const data = Buffer.from(wire);
      return Array.from({ length: Math.ceil(data.length / 7) }, (_, i) => data.subarray(i * 7, (i + 1) * 7));
    },
  });
  assert.equal(replies[0].error.code, -32700);
  assert.deepEqual(replies.find((reply) => reply.id === 2).result, {});
  assert.equal(replies.find((reply) => reply.id === 3).result.structuredContent.title, '中文标题');
  assert.equal(replies.length, 4);
});

test('forwards only validated calls with bearer authentication and marks publish approval', async () => {
  let calls = 0;
  const replies = await conversation([init(), request(2, 'tools/call', { name: 'jobs_enqueue', arguments: { draftId: 'draft-1', accountIds: ['account-1'], action: 'publish' } }), request(3, 'tools/call', { name: 'jobs_enqueue', arguments: { draftId: 'draft-1', accountIds: ['account-1'], action: 'publish', source: 'user' } })], {
    fetchImpl: async (url, options) => {
      calls++;
      assert.equal(url, 'http://127.0.0.1:43176/v1/call');
      assert.equal(options.headers.Authorization, `Bearer ${token}`);
      assert.equal(JSON.parse(options.body).method, 'jobs.enqueue');
      assert.equal(options.redirect, 'error');
      return new Response(JSON.stringify({ result: [{ id: 'job-1', status: 'pending_approval' }] }));
    },
  });
  assert.equal(calls, 1);
  const response = replies.find((reply) => reply.id === 2).result;
  assert.equal(response.structuredContent.approvalRequired, true);
  assert.equal(response.isError, false);
  assert.equal(replies.find((reply) => reply.id === 3).error.code, -32602);
});

test('reports transport failure as uncertain for writes and does not auto-retry', async () => {
  let calls = 0;
  const replies = await conversation([init(), request(2, 'tools/call', { name: 'drafts_save', arguments: { draft: { kind: 'product', title: 'Draft' } } })], {
    fetchImpl: async () => { calls++; throw new Error(`private error ${token}`); },
  });
  const result = replies.find((reply) => reply.id === 2).result;
  assert.equal(result.isError, true);
  assert.equal(result.structuredContent.error.uncertain, true);
  assert.ok(!JSON.stringify(replies).includes(token));
  assert.equal(calls, 1);
});

test('surfaces known bridge errors as tool errors and redacts the pairing token', async () => {
  const replies = await conversation([init(), request(2, 'tools/call', { name: 'drafts_list' })], {
    fetchImpl: async () => new Response(JSON.stringify({ error: { code: 'OUTCOME_UNKNOWN', message: `No response ${token}`, uncertain: true } }), { status: 504 }),
  });
  const result = replies.find((reply) => reply.id === 2).result;
  assert.equal(result.isError, true);
  assert.equal(result.structuredContent.error.code, 'OUTCOME_UNKNOWN');
  assert.ok(!JSON.stringify(replies).includes(token));
});

test('drops oversized frames and recovers at the next newline', async () => {
  const replies = await conversation(['x'.repeat(262145), init(), request(2, 'ping')]);
  assert.equal(replies[0].error.code, -32700);
  assert.deepEqual(replies.find((reply) => reply.id === 2).result, {});
});

test('private token files reject world-readable permissions and symlinks', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'matrixpost-bridge-test-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const path = join(directory, 'token');
  await writeFile(path, `${token}\n`, { mode: 0o600 });
  assert.equal(await readPrivateToken(path), token);
  if (process.platform !== 'win32') {
    await chmod(path, 0o644);
    await assert.rejects(() => readPrivateToken(path), /private regular file/);
    await chmod(path, 0o600);
    await symlink(path, join(directory, 'alias'));
    await assert.rejects(() => readPrivateToken(join(directory, 'alias')));
  }
});

test('completes a real MCP to HTTP to extension-poll round trip', async (t) => {
  const extensionId = 'abcdefghijklmnopabcdefghijklmnop';
  const bridge = createBridge({ extensionId });
  const url = await bridge.listen(0);
  t.after(() => bridge.close());
  const input = new PassThrough();
  const output = new PassThrough();
  let received = '';
  output.on('data', (chunk) => { received += chunk.toString(); });
  const run = runMcp({ input, output, token: bridge.token, port: Number(new URL(url).port) });
  input.write(`${JSON.stringify(init())}\n${JSON.stringify(request(2, 'tools/call', { name: 'drafts_save', arguments: { draft: { kind: 'video', title: 'Bridge round trip' } } }))}\n`);
  for (let attempt = 0; attempt < 100 && !bridge.pendingCount; attempt++) await delay(5);
  assert.equal(bridge.pendingCount, 1);
  const headers = { Authorization: `Bearer ${bridge.token}`, Origin: `chrome-extension://${extensionId}` };
  const { requests } = await (await fetch(`${url}/v1/poll`, { headers })).json();
  assert.equal(requests[0].method, 'drafts.save');
  assert.equal(requests[0].source, 'agent');
  assert.equal(requests[0].params.draft.title, 'Bridge round trip');
  await fetch(`${url}/v1/results`, { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify({ id: requests[0].id, result: { id: 'draft-round-trip', revision: 1 } }) });
  input.end();
  await run;
  const replies = received.trim().split('\n').map((line) => JSON.parse(line));
  assert.equal(replies.find((reply) => reply.id === 2).result.structuredContent.id, 'draft-round-trip');
  assert.equal(bridge.pendingCount, 0);
});

test('MCP cancellation removes an unsent bridge request without replay', async (t) => {
  const extensionId = 'abcdefghijklmnopabcdefghijklmnop';
  const bridge = createBridge({ extensionId });
  const url = await bridge.listen(0);
  t.after(() => bridge.close());
  const input = new PassThrough();
  const output = new PassThrough();
  let received = '';
  output.on('data', (chunk) => { received += chunk.toString(); });
  const run = runMcp({ input, output, token: bridge.token, port: Number(new URL(url).port) });
  input.write(`${JSON.stringify(init())}\n${JSON.stringify(request(2, 'tools/call', { name: 'drafts_list' }))}\n`);
  for (let attempt = 0; attempt < 100 && !bridge.pendingCount; attempt++) await delay(5);
  assert.equal(bridge.pendingCount, 1);
  input.end(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 2 } })}\n`);
  await run;
  for (let attempt = 0; attempt < 100 && bridge.pendingCount; attempt++) await delay(5);
  assert.equal(bridge.pendingCount, 0);
  const replies = received.trim().split('\n').map((line) => JSON.parse(line));
  assert.equal(replies.find((reply) => reply.id === 2).error.code, -32800);
  const polled = await (await fetch(`${url}/v1/poll`, { headers: { Authorization: `Bearer ${bridge.token}`, Origin: `chrome-extension://${extensionId}` } })).json();
  assert.deepEqual(polled.requests, []);
});
