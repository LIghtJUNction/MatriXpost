# MatriXpost protocol reference

Read current Rust source before changing a contract. This reference records implemented surfaces; local runner outcomes never prove remote publication.

## Publication compatibility

Serialize these canonical video platform codes: `dy`, `sph`, `blbl`, `bjh`, `tt`, `ks`, `xhs`, and `fqsp`. Preserve accepted aliases in core without adding another canonical code absent an explicit compatibility decision.

`matrixpost` writes one JSON document to stdout. Its publication commands validate local intent. `login --platform <platform>` uses only a matching explicit loopback TCP `--provider-runner <platform>=tcp:127.0.0.1:<port>` and sends its versioned request to the separately started runner's `/v1/login`. `opened` means that runner opened the platform page in its already attached browser and the user must complete login manually; it confirms neither login nor publication. An absent or non-TCP runner is `unavailable`; a failed or rejected handoff is not success. Video `publish` requires an explicit eligible loopback provider runner, and `publish-article` accepts Juejin only and requires one explicit eligible article runner. `accounts`, `history`, and `providers` expose credential-free local state.

`matrixpost-webdriver-runner` always exposes `/v1/login`, but manual navigation is enabled only when it starts with `--allow-login-navigation` plus valid loopback WebDriver and browser-debugger endpoints. Without that opt-in, the versioned response is `unavailable`. The opt-in navigates the already user-managed attached browser to a platform page; it never starts a browser or extracts a profile, cookie, or session. The runner returns versioned `opened` (with `manual_login_required:true`), `unavailable`, or `rejected` local outcomes. None proves a completed login.

`matrixpostd` defaults to `127.0.0.1:8788`. `GET /`, `GET /health`, `GET /platforms`, `GET /providers`, `GET /creative-statements`, `POST /changeData`, and `POST /publish` retain the MatrixMedia-compatible publication surface. `POST /publish` returns local availability/queue truth, never remote publication proof.

The daemon independently claims due persisted queued jobs every `scheduler_interval_seconds` (default `5`) and bounds each pass with `scheduler_batch_size` (default `16`, range `1..=64`). It uses the same explicitly configured loopback local runners as immediate HTTP dispatch; neither path opens a browser or calls a platform directly. A claim atomically changes only due scheduled queued jobs to `dispatching`; drafts, unscheduled jobs, future jobs, and already claimed jobs are excluded. The dispatch request has `scheduled_at` cleared because the daemon has already supplied the due-time decision. Every terminal pass atomically records exactly one safe local history outcome with the terminal transition: all local runners queued becomes `published`, all unavailable becomes `unavailable`, and any mixed/rejected/error result becomes `failed`. A claimed task is an at-least-once local workflow: if terminal persistence fails, the exact non-terminal claim is requeued for retry. `published` here means only the local runner workflow completed, never that a remote platform accepted or processed content.

Expose the same generic lifecycle model over HTTP: `GET|POST /lifecycle/objects`, `GET /lifecycle/objects/{id}`, `POST /lifecycle/objects/{id}/transition`, `GET|POST /lifecycle/objects/{id}/ledger`, `GET|POST /lifecycle/objects/{id}/attributions`, and `GET|POST /lifecycle/objects/{id}/relations`. Use camelCase request fields on this HTTP surface; stored core-record responses preserve snake_case fields. For relation creation, require `sourceBusinessObjectId` to match `{id}` in the path and provide `id`, `targetBusinessObjectId`, `relationType`, optional `attributes`, and optional `createdAt`. Return `404 not_found` for a missing object; return an empty child list only for an existing object with no entries, attributions, or incoming/outgoing relations.

## Generic lifecycle CLI

Keep the global `--state-path <path>` before the command. Every command emits JSON.

| Intent | CLI command |
| --- | --- |
| List objects | `matrixpost lifecycle objects` |
| Read one object | `matrixpost lifecycle object get --id <id>` |
| Create object | `matrixpost lifecycle object create --id <id> --kind <kind> --display-name <name> [--external-id <id>] [--attribute KEY=VALUE]` |
| List ledger | `matrixpost lifecycle ledger list --object <id>` |
| Append immutable entry | `matrixpost lifecycle ledger add --id <id> --object <id> --direction expense|revenue --category <category> --amount-minor <integer> --currency <ISO4217>` |
| List attribution | `matrixpost lifecycle attribution list --object <id>` |
| Link content history | `matrixpost lifecycle attribution add --object <id> --history <history-id>` |
| List incoming and outgoing relations | `matrixpost lifecycle relation list --object <id>` |
| Add directed relation | `matrixpost lifecycle relation add --id <id> --source <object-id> --target <object-id> --type <type> [--attribute KEY=VALUE]` |
| Transition object | `matrixpost lifecycle transition --id <id> --expected-revision <n> --lifecycle-status draft|active|completed|archived --approval-status pending|approved|rejected` |

Accept optional RFC3339 timestamps, counterparty, reference, and description where the CLI exposes them. Use integer minor amounts; never use floating-point currency. Append-only ledger entries have no update or delete operation. Refresh the object after every transition because success increments its revision.

Create both endpoint objects before adding a relation. Relations are immutable, directed, and caller-typed. Reject self relations. Listing an existing object returns both incoming and outgoing relations and returns an empty list only when it has neither.

## MCP stdio surface

`matrixpost-mcp` uses the same SQLite state as CLI. Resolve state in this order: `--state-path`, `MATRIXPOST_STATE_PATH`, then `matrixpost.db` in the current directory. Keep stdout exclusively MCP JSON-RPC; send optional diagnostics only to stderr through `MATRIXPOST_MCP_LOG=1`. It accepts repeatable `--provider-runner PLATFORM=tcp:127.0.0.1:PORT` declarations (the split `--provider-runner PLATFORM=tcp:127.0.0.1:PORT` form is equivalent). Declarations are parsed through the core local-only runner contract: duplicate platforms, malformed declarations, and non-loopback TCP endpoints fail before the server starts. They neither start a runner/browser nor persist an endpoint.

There are fourteen typed tools. Tool names are snake_case. All typed tool inputs use camelCase field names (for example `businessObjectId` and `expectedRevision`); returned core records preserve their Rust serialization with snake_case fields (for example `business_object_id`, `lifecycle_status`, and `amount_minor`).

| Group | Tools |
| --- | --- |
| Publication (4) | `list_accounts`, `list_history`, `publish_video`, `publish_article` |
| Lifecycle (10) | `list_business_objects`, `get_business_object`, `create_business_object`, `list_ledger_entries`, `append_ledger_entry`, `list_content_attributions`, `add_content_attribution`, `list_business_relations`, `add_business_relation`, `transition_business_object` |

Lifecycle examples use `create_business_object` with `{id, kind, displayName, externalId?, lifecycleStatus?, approvalStatus?, attributes?}`; `append_ledger_entry` with `{id, businessObjectId, direction, category, amountMinor, currency, ...}`; `add_business_relation` with `{id, sourceBusinessObjectId, targetBusinessObjectId, relationType, attributes?}` after creating both objects; and `transition_business_object` with `{id, expectedRevision, lifecycleStatus, approvalStatus, updatedAt?}`. Reject unknown input fields. Relations are directed, immutable, non-self links and list from either endpoint. Return `not_found` for a missing object or history record; return an empty list only for an existing object with no corresponding entries, links, or relations.

Reject lifecycle attribute key names such as `token`, `cookie`, `password`, and `session`. Do not scan ordinary attribute values heuristically. Never accept or return named credential, cookie, token, password, or session fields. Lifecycle tools are generic local SQLite state management only: do not invoke a provider, browser, runner, remote publishing path, or agent runtime.

For `publish_video`, an immediate request is dispatched once through the matching declared local runner. `queued` means every requested runner completed its local workflow; it sets `provider_available:true`, `remote_publish_attempted:true`, and `persisted:false`, but never proves platform-side publication. With no matching runner, the safe result is `unavailable` with no attempt. A partial or rejected local dispatch is `rejected`; it reports only per-platform `queued`/`unavailable`/`rejected` labels, never runner endpoints or runner-provided reasons. `remote_publish_attempted` is true only when a local runner was contacted. Requests with `draft:true` or `publishAt` are persisted as `draft_locally` or `scheduled_locally` and never dispatched by MCP; scheduled execution is a separate service concern.

## Tauri and service delivery

The Tauri v2 desktop shell opens application-data `matrixpost.db`; it starts no daemon, shell, browser, provider, or runner. It exposes fifteen IPC commands: `desktop_snapshot`, `save_local_draft`, `dispatch_to_local_runner`, `save_account`, `save_article_account`, `local_history`, `lifecycle_objects`, `create_lifecycle_object`, `lifecycle_ledger_entries`, `append_lifecycle_ledger_entry`, `lifecycle_content_attributions`, `add_lifecycle_content_attribution`, `lifecycle_business_relations`, `add_lifecycle_business_relation`, and `transition_lifecycle_object`. `dispatch_to_local_runner` requires explicit confirmation, accepts only one-shot explicit loopback provider declarations, rejects scheduling, persists no runner/browser/account configuration, and returns safe local outcome labels with `remotePublishConfirmed:false`; it never proves remote publication. The lifecycle UI selects an existing object as source and offers only other local objects as targets, preventing a self relation before IPC. Project only safe display metadata and local lifecycle records to the frontend.

Use `deploy/matrixpostd.service` and `deploy/matrixpostd.example.toml` as the systemd deployment starting point. Do not install or enable it automatically. Keep its bind address intentional and its configuration secret-free.

The repository includes CI for locked workspace checks, public-package metadata, a core package archive, and macOS/Windows desktop compilation checks. Version 0.4.0 of the four public crates is published on crates.io and the v0.4.0 GitHub Release exists. The desktop and WebDriver runner crates are not crates.io packages. The checksum-pinned AUR recipe has a verified remote Git upload; wait for the AUR package index before describing it as helper-installable. No authenticated live-platform acceptance proof exists, and no `.cursor` directory is used.
