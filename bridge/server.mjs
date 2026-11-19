import http from 'node:http';
import { randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';
import { BridgeError, isObject, validateCall } from './schema.mjs';

const DEFAULT_MAX_BODY = 262144;
const secretKeys = /^(?:credentials?|accessToken|refreshToken|consumerKey|consumerSecret|password|passphrase|authorization|cookie|secret|token|vault|ciphertext)$/i;

export function sanitizeResult(value, token, depth = 0) {
  if (depth > 16) throw new BridgeError('INVALID_RESULT', 'Extension result exceeds the nesting limit.');
  if (typeof value === 'string') return value.replaceAll(token, '[redacted]');
  if (Array.isArray(value)) return value.map((item) => sanitizeResult(item, token, depth + 1));
  if (isObject(value)) {
    const result = Object.create(null);
    for (const [key, entry] of Object.entries(value)) {
      if (['__proto__', 'prototype', 'constructor'].includes(key)) continue;
      result[key] = secretKeys.test(key) ? '[redacted]' : sanitizeResult(entry, token, depth + 1);
    }
    return result;
  }
  return value;
}

function writeJson(response, status, value) {
  if (response.destroyed || response.writableEnded) return;
  response.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
  response.end(JSON.stringify(value));
}

async function readJson(request, maxBytes) {
  if (!/^application\/json(?:\s*;|$)/i.test(request.headers['content-type'] ?? '')) {
    throw new BridgeError('CONTENT_TYPE', 'Content-Type must be application/json.', 415);
  }
  const length = request.headers['content-length'];
  if (length && (!/^\d+$/.test(length) || Number(length) > maxBytes)) {
    request.resume();
    throw new BridgeError('BODY_TOO_LARGE', 'Request body exceeds the limit.', 413);
  }
  let size = 0;
  const chunks = [];
  for await (const chunk of request) {
    size += chunk.length;
    if (size > maxBytes) throw new BridgeError('BODY_TOO_LARGE', 'Request body exceeds the limit.', 413);
    chunks.push(chunk);
  }
  try { return JSON.parse(Buffer.concat(chunks).toString('utf8')); }
  catch { throw new BridgeError('INVALID_JSON', 'Request body must be valid JSON.'); }
}

function tokenEquals(supplied, token) {
  const candidate = Buffer.from(supplied ?? '');
  const expected = Buffer.from(`Bearer ${token}`);
  return candidate.length === expected.length && timingSafeEqual(candidate, expected);
}

/** A custom loopback transport for the extension; MCP itself is served over stdio. */
export function createBridge({ extensionId, token = randomBytes(32).toString('hex'), requestTimeoutMs = 24000, maxPending = 32, maxBodyBytes = DEFAULT_MAX_BODY } = {}) {
  if (typeof extensionId !== 'string' || !/^[a-p]{32}$/.test(extensionId)) {
    throw new BridgeError('EXTENSION_ID_REQUIRED', 'An exact Chrome extension ID (32 a-p characters) is required.');
  }
  if (typeof token !== 'string' || !/^[a-zA-Z0-9_-]{43,128}$/.test(token)) {
    throw new BridgeError('INVALID_TOKEN', 'Pairing token must contain at least 256 bits of random material.');
  }
  if (!Number.isInteger(requestTimeoutMs) || requestTimeoutMs < 10 || requestTimeoutMs > 25000) throw new BridgeError('INVALID_TIMEOUT', 'Request timeout must be between 10 and 25000 milliseconds.');
  if (!Number.isInteger(maxPending) || maxPending < 1 || maxPending > 128) throw new BridgeError('INVALID_LIMIT', 'Pending limit must be between 1 and 128.');
  if (!Number.isInteger(maxBodyBytes) || maxBodyBytes < 1024 || maxBodyBytes > 1048576) throw new BridgeError('INVALID_LIMIT', 'Body limit must be between 1 KiB and 1 MiB.');
  const origin = `chrome-extension://${extensionId}`;
  const pending = new Map();
  let lastPollAt = 0;
  let closing = false;

  function finish(entry, status, payload) {
    if (!pending.delete(entry.id)) return false;
    clearTimeout(entry.timer);
    writeJson(entry.response, status, payload);
    return true;
  }

  const server = http.createServer(async (request, response) => {
    try {
      if (closing) throw new BridgeError('SHUTTING_DOWN', 'Bridge is shutting down.', 503);
      const address = server.address();
      const expectedHost = `127.0.0.1:${address?.port}`;
      if (request.headers.host !== expectedHost) throw new BridgeError('INVALID_HOST', 'Only the bound loopback host is allowed.', 403);
      const requestOrigin = request.headers.origin;
      if (requestOrigin !== undefined && requestOrigin !== origin) throw new BridgeError('INVALID_ORIGIN', 'Origin is not the paired extension.', 403);
      if (request.headers.referer !== undefined) {
        let referrer;
        try { referrer = new URL(request.headers.referer); } catch { throw new BridgeError('INVALID_ORIGIN', 'Invalid referrer.', 403); }
        if (`${referrer.protocol}//${referrer.hostname}` !== origin || referrer.username || referrer.password || referrer.port) throw new BridgeError('INVALID_ORIGIN', 'Referrer is not the paired extension.', 403);
      }
      if (requestOrigin === origin) {
        response.setHeader('Access-Control-Allow-Origin', origin);
        response.setHeader('Vary', 'Origin');
      }
      if (request.method === 'OPTIONS') {
        if (requestOrigin !== origin || !['GET', 'POST'].includes(request.headers['access-control-request-method'])) throw new BridgeError('INVALID_PREFLIGHT', 'Preflight is not allowed.', 403);
        const headers = (request.headers['access-control-request-headers'] ?? '').split(',').map((header) => header.trim().toLowerCase()).filter(Boolean);
        if (headers.some((header) => !['authorization', 'content-type'].includes(header))) throw new BridgeError('INVALID_PREFLIGHT', 'Preflight headers are not allowed.', 403);
        response.setHeader('Access-Control-Allow-Methods', 'GET, POST');
        response.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type');
        response.setHeader('Access-Control-Max-Age', '300');
        if (request.headers['access-control-request-private-network'] === 'true') response.setHeader('Access-Control-Allow-Private-Network', 'true');
        response.writeHead(204).end();
        return;
      }
      if (!tokenEquals(request.headers.authorization, token)) throw new BridgeError('UNAUTHORIZED', 'A valid pairing token is required.', 401);
      if (request.url === '/health' && request.method === 'GET') {
        writeJson(response, 200, { ok: true, connected: lastPollAt > 0 && Date.now() - lastPollAt <= 5000, lastPollAt: lastPollAt || null, pending: pending.size });
        return;
      }
      if (request.url === '/v1/poll' && request.method === 'GET') {
        if (requestOrigin !== origin) throw new BridgeError('EXTENSION_ONLY', 'Only the paired extension may poll.', 403);
        lastPollAt = Date.now();
        const entry = [...pending.values()].find((candidate) => !candidate.delivered && candidate.expiresAt > Date.now());
        if (entry) entry.delivered = true;
        writeJson(response, 200, { requests: entry ? [{ id: entry.id, method: entry.method, params: entry.params, source: 'agent', expiresAt: entry.expiresAt }] : [] });
        return;
      }
      if (request.url === '/v1/results' && request.method === 'POST') {
        if (requestOrigin !== origin) throw new BridgeError('EXTENSION_ONLY', 'Only the paired extension may complete requests.', 403);
        const value = await readJson(request, maxBodyBytes);
        if (!isObject(value) || typeof value.id !== 'string' || Object.keys(value).some((key) => !['id', 'result', 'error'].includes(key))
          || Object.hasOwn(value, 'result') === Object.hasOwn(value, 'error')) throw new BridgeError('INVALID_RESULT', 'Expected an ID and exactly one of result or error.');
        const entry = pending.get(value.id);
        if (!entry || !entry.delivered || entry.expiresAt <= Date.now()) throw new BridgeError('REQUEST_NOT_FOUND', 'Request is expired, unknown, or already completed.', 404);
        if (Object.hasOwn(value, 'error')) {
          if (!isObject(value.error) || typeof value.error.code !== 'string' || value.error.code.length > 128 || typeof value.error.message !== 'string' || value.error.message.length > 4096
            || Object.keys(value.error).some((key) => !['code', 'message', 'uncertain'].includes(key))
            || (value.error.uncertain !== undefined && typeof value.error.uncertain !== 'boolean')) throw new BridgeError('INVALID_RESULT', 'Invalid extension error.');
          finish(entry, 200, { error: sanitizeResult(value.error, token) });
        } else {
          finish(entry, 200, { result: sanitizeResult(value.result, token) });
        }
        writeJson(response, 200, { ok: true });
        return;
      }
      if (request.url === '/v1/call' && request.method === 'POST') {
        const call = validateCall(await readJson(request, maxBodyBytes));
        if (pending.size >= maxPending) throw new BridgeError('QUEUE_FULL', 'Bridge pending request limit reached.', 429);
        const entry = { id: randomUUID(), ...call, delivered: false, response, expiresAt: Date.now() + requestTimeoutMs, timer: null };
        entry.timer = setTimeout(() => finish(entry, 504, { error: {
          code: entry.delivered ? 'OUTCOME_UNKNOWN' : 'EXTENSION_UNAVAILABLE',
          message: entry.delivered ? 'Extension response timed out after dispatch. Inspect drafts or jobs before retrying; the action may have completed.' : 'The extension did not pick up this request. Open MatriXpost and connect its AI bridge.',
          uncertain: entry.delivered,
        } }), requestTimeoutMs);
        pending.set(entry.id, entry);
        response.on('close', () => {
          if (pending.delete(entry.id)) clearTimeout(entry.timer);
        });
        return;
      }
      throw new BridgeError('NOT_FOUND', 'Unknown route or HTTP method.', 404);
    } catch (error) {
      const safe = error instanceof BridgeError ? error : new BridgeError('INTERNAL_ERROR', 'The bridge could not complete this request.', 500);
      writeJson(response, safe.status, { error: { code: safe.code, message: safe.message, uncertain: safe.uncertain } });
    }
  });
  server.headersTimeout = 10000;
  server.requestTimeout = 30000;
  server.keepAliveTimeout = 5000;
  server.maxHeadersCount = 32;

  return {
    server, token, extensionId,
    get pendingCount() { return pending.size; },
    async listen(port = 43176) {
      if (!Number.isInteger(port) || port < 0 || port > 65535) throw new BridgeError('INVALID_PORT', 'Invalid bridge port.');
      await new Promise((resolve, reject) => {
        const onError = (error) => { server.off('listening', onListening); reject(error); };
        const onListening = () => { server.off('error', onError); resolve(); };
        server.once('error', onError);
        server.once('listening', onListening);
        server.listen(port, '127.0.0.1');
      });
      return `http://127.0.0.1:${server.address().port}`;
    },
    async close() {
      closing = true;
      for (const entry of pending.values()) finish(entry, 503, { error: { code: 'BRIDGE_CLOSED', message: 'Bridge closed. Inspect local jobs before retrying a dispatched request.', uncertain: entry.delivered } });
      if (server.listening) await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    },
  };
}
