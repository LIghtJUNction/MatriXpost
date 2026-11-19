# MatriXpost AI bridge

MatriXpost exposes nine MCP tools for preparing content and managing publication jobs. The extension owns drafts, account credentials and publication execution. This companion forwards validated requests to the open extension workspace; it does not run an AI model or store platform credentials.

## Connect an AI client

1. Load the extension and copy its ID from `chrome://extensions`.
2. Start the companion with Node.js 22 or later, replacing the placeholders below with the actual extension ID and an absolute path to a new private token file:

   ```sh
   node bridge/cli.mjs serve --extension-id <extension-id> --token-file /absolute/private/path/matrixpost-token
   ```

3. Open MatriXpost's AI page. Use `http://127.0.0.1:43176` and paste the pairing token printed by the companion. Allow the extension's requested loopback host permission and keep the workspace open.
4. Add the MCP stdio server to the AI client's configuration:

   ```json
   {
     "mcpServers": {
       "matrixpost": {
         "command": "node",
         "args": [
           "/absolute/path/to/MatriXpost/bridge/cli.mjs",
           "mcp",
           "--token-file",
           "/absolute/private/path/matrixpost-token"
         ]
       }
     }
   }
   ```

Use the same `--port` on `serve` and `mcp` to change the port. MCP can also read `MATRIXPOST_TOKEN` from its process environment. The token file must be a regular file with owner-only permissions on Unix; symlinks are rejected. `serve` creates new token files with mode `0600` and never overwrites existing files. Choose a new token file for a new bridge session. The token is printed once to stderr; MCP stdout contains only JSON-RPC messages.

## Tools and outcomes

| MCP tool | Extension method | Result |
| --- | --- | --- |
| `platforms_list` | `platforms.list` | Platform capabilities and implementation status |
| `accounts_list` | `accounts.list` | Account references, without credentials |
| `drafts_list` | `drafts.list` | Local video and product drafts |
| `drafts_save` | `drafts.save` | A saved draft with its current revision |
| `publish_preview` | `publish.preview` | Validation of specified accounts and content |
| `jobs_enqueue` | `jobs.enqueue` | A local job; AI publish requests require extension review |
| `jobs_list` | `jobs.list` | Job state and confirmed remote results |
| `jobs_retry` | `jobs.retry` | Retry of an eligible failed job |
| `jobs_cancel` | `jobs.cancel` | Cancellation of eligible local work |

An AI publication request carries `action: "publish"`; the extension places it in `pending_approval`. A human reviews the exact job in MatriXpost before approving dispatch. `action: "draft"` may create a remote private video or product draft according to the selected adapter, so it is still a write operation. A queue response alone never proves that a platform accepted or published anything.

The bridge exposes no vault, credential, arbitrary HTTP, shell or browser-script tool. Files must already be imported through the extension. Updating an existing draft requires its current `revision`.

## Extension transport contract

Every data request carries `Authorization: Bearer <pairing-token>`. The server binds only `127.0.0.1`, requires its exact `Host`, and accepts browser requests only from the configured `chrome-extension://<id>` origin. Browser CORS preflight is an exception to bearer authentication: it returns no data and requires the exact origin, host, method and header allowlist.

| Endpoint | Caller | Body or response |
| --- | --- | --- |
| `GET /health` | Paired extension or local client | `{ok, connected, lastPollAt, pending}` |
| `POST /v1/call` | Local MCP process | `{method, params}` → `{result}` or `{error}` |
| `GET /v1/poll` | Paired extension | `{requests: [{id, method, params, source: "agent", expiresAt}]}` |
| `POST /v1/results` | Paired extension | `{id, result}` or `{id, error: {code, message, uncertain?}}` → `{ok: true}` |

Poll once per second while connected and process at most one request from each response. The extension must reject expired requests and dispatch them with a trusted `source: "agent"` context. It must never derive authority from request parameters. Return exactly one result per request. Polling and result submission require the paired extension's Origin header; a local client with the bearer token cannot drain the queue.

Each call waits for at most 24 seconds by default. Requests that expire before polling are removed and will not execute after reconnection. A request delivered before a timeout may already have changed state: the bridge returns `OUTCOME_UNKNOWN` with `uncertain: true`. Inspect drafts and jobs before retrying. The bridge does not redeliver requests or retry platform actions. A late or duplicate result is rejected.

The default limits are 32 pending calls, a 256 KiB request/result body, one request per poll and 32 concurrent MCP requests. Credentials in known secret fields and any occurrence of the bridge token are redacted from extension results before delivery to an AI client.

## Verification

```sh
node --test tests/bridge*.test.mjs
```

The tests exercise real loopback HTTP, authentication, Host and Origin checks, one-time delivery, timeouts before and after dispatch, cancellation, queue and payload limits, secret redaction, MCP framing and a complete MCP → HTTP → extension-poll → result round trip. These tests use a simulated extension response; they do not publish to any video platform or shop.

MCP stdio framing and tool results follow the official [transport specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) and [tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools). Supported negotiated versions are `2024-11-05`, `2025-03-26` and `2025-06-18`.
