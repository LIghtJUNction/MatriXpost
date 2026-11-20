import { BridgeError, TOOL_DEFINITIONS, isObject, publicTools, validateCall } from './schema.mjs';

const VERSIONS = ['2025-06-18', '2025-03-26', '2024-11-05'];
const MAX_LINE_BYTES = 262144;
const errorReply = (id, code, message) => ({ jsonrpc: '2.0', id, error: { code, message } });

function toolResult(value, isError = false) {
  const structuredContent = isObject(value) ? value : { value };
  return { content: [{ type: 'text', text: JSON.stringify(structuredContent) }], structuredContent, isError };
}

/** Newline-delimited MCP stdio. The companion HTTP API is not a public MCP endpoint. */
export async function runMcp({ input = process.stdin, output = process.stdout, token, port = 43176, fetchImpl = fetch, requestTimeoutMs = 26000 } = {}) {
  if (typeof token !== 'string' || !/^[a-zA-Z0-9_-]{43,128}$/.test(token)) throw new BridgeError('TOKEN_REQUIRED', 'Set MATRIXPOST_TOKEN or use --token-file to authenticate the bridge.');
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new BridgeError('INVALID_PORT', 'Invalid bridge port.');
  let initialized = false;
  let buffer = '';
  let discarding = false;
  let ended = false;
  const inflight = new Map();
  const writes = new Set();

  function send(message) {
    const encoded = `${JSON.stringify(message)}\n`;
    const pending = new Promise((resolve, reject) => output.write(encoded, (error) => error ? reject(error) : resolve()));
    writes.add(pending);
    pending.finally(() => writes.delete(pending)).catch(() => {});
    return pending;
  }

  async function handle(message, controller) {
    if (message.method === 'ping') return {};
    if (message.method === 'initialize') {
      if (initialized) return { protocolError: [-32600, 'The MCP session is already initialized.'] };
      if (!isObject(message.params) || typeof message.params.protocolVersion !== 'string') return { protocolError: [-32602, 'initialize requires a protocolVersion.'] };
      initialized = true;
      return {
        protocolVersion: VERSIONS.includes(message.params.protocolVersion) ? message.params.protocolVersion : VERSIONS[0],
        capabilities: { tools: { listChanged: false } },
        serverInfo: { name: 'matrixpost', version: '0.5.0' },
        instructions: 'MatriXpost prepares video and product drafts and queues account-specific jobs. Open the extension and connect its local bridge. AI publish requests wait for approval in the extension. Credentials and vault controls are not exposed. A timeout after dispatch is uncertain: inspect existing drafts and jobs before retrying.',
      };
    }
    if (!initialized) return { protocolError: [-32002, 'Initialize the MCP session first.'] };
    if (message.method === 'tools/list') {
      if (message.params !== undefined && (!isObject(message.params) || Object.keys(message.params).some((key) => key !== '_meta'))) return { protocolError: [-32602, 'This tool list has no pagination cursor.'] };
      return { tools: publicTools() };
    }
    if (message.method !== 'tools/call') return { protocolError: [-32601, 'Method not found.'] };
    if (!isObject(message.params) || Object.keys(message.params).some((key) => !['name', 'arguments', '_meta'].includes(key))) return { protocolError: [-32602, 'Invalid tools/call parameters.'] };
    const tool = TOOL_DEFINITIONS.find((entry) => entry.name === message.params.name);
    if (!tool) return { protocolError: [-32602, 'Unknown tool.'] };
    let call;
    try { call = validateCall({ method: tool.method, params: message.params.arguments ?? {} }); }
    catch (error) { return { protocolError: [-32602, error instanceof BridgeError ? error.message : 'Invalid tool arguments.'] }; }
    try {
      const response = await fetchImpl(`http://127.0.0.1:${port}/v1/call`, {
        method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify(call), redirect: 'error',
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(requestTimeoutMs)]),
      });
      const raw = await response.text();
      if (raw.length > 1048576) throw new Error('oversized response');
      let data;
      try { data = JSON.parse(raw); } catch { throw new Error('invalid response'); }
      if (!isObject(data)) throw new Error('invalid response');
      if (isObject(data.error)) {
        return toolResult({ error: {
          code: typeof data.error.code === 'string' ? data.error.code : 'BRIDGE_ERROR',
          message: typeof data.error.message === 'string' ? data.error.message.replaceAll(token, '[redacted]') : 'The bridge rejected this request.',
          uncertain: Boolean(data.error.uncertain),
        } }, true);
      }
      if (!response.ok || !Object.hasOwn(data, 'result')) throw new Error('invalid response');
      if (tool.method === 'jobs.enqueue' && call.params.action === 'publish') {
        return toolResult({ value: data.result, approvalRequired: true, message: 'Publish request recorded. Review and approve the exact job in the extension; this response does not confirm publication.' });
      }
      return toolResult(data.result);
    } catch {
      if (controller.signal.aborted) return { protocolError: [-32800, 'Request cancelled. A dispatched action may have completed; inspect local jobs before retrying.'] };
      return toolResult({ error: { code: 'BRIDGE_UNAVAILABLE', message: 'The local bridge did not return a valid response. Check that it is running and connected. Inspect drafts and jobs before retrying a write.', uncertain: !tool.annotations.readOnlyHint } }, true);
    }
  }

  function accept(line) {
    if (!line.trim()) return;
    let message;
    try { message = JSON.parse(line); } catch { void send(errorReply(null, -32700, 'Parse error.')); return; }
    if (!isObject(message) || message.jsonrpc !== '2.0' || typeof message.method !== 'string'
      || Object.keys(message).some((key) => !['jsonrpc', 'id', 'method', 'params'].includes(key))
      || (Object.hasOwn(message, 'id') && !(typeof message.id === 'string' || Number.isSafeInteger(message.id)))) {
      void send(errorReply(null, -32600, 'Invalid JSON-RPC request.'));
      return;
    }
    const hasId = Object.hasOwn(message, 'id');
    if (!hasId) {
      if (message.method === 'notifications/cancelled' && isObject(message.params)) inflight.get(message.params.requestId)?.controller.abort();
      return;
    }
    if (inflight.has(message.id)) { void send(errorReply(message.id, -32600, 'Request ID is already in use.')); return; }
    if (inflight.size >= 32) { void send(errorReply(message.id, -32000, 'Too many in-flight requests.')); return; }
    const controller = new AbortController();
    const promise = Promise.resolve().then(() => handle(message, controller)).then(async (result) => {
      if (result.protocolError) await send(errorReply(message.id, ...result.protocolError));
      else await send({ jsonrpc: '2.0', id: message.id, result });
    }).catch(async () => { await send(errorReply(message.id, -32603, 'Internal error.')); }).finally(() => inflight.delete(message.id));
    inflight.set(message.id, { controller, promise });
  }

  input.setEncoding('utf8');
  try {
    for await (const chunk of input) {
      const parts = chunk.split('\n');
      for (let index = 0; index < parts.length; index++) {
        if (!discarding) buffer += parts[index];
        if (!discarding && Buffer.byteLength(buffer) > MAX_LINE_BYTES) {
          discarding = true;
          buffer = '';
          await send(errorReply(null, -32700, 'JSON-RPC message exceeds the size limit.'));
        }
        if (index < parts.length - 1) {
          if (!discarding) accept(buffer.endsWith('\r') ? buffer.slice(0, -1) : buffer);
          buffer = '';
          discarding = false;
        }
      }
    }
    ended = true;
    if (buffer.trim() && !discarding) accept(buffer);
    await Promise.allSettled([...inflight.values()].map((entry) => entry.promise));
    await Promise.allSettled([...writes]);
  } finally {
    if (!ended) for (const entry of inflight.values()) entry.controller.abort();
  }
}
