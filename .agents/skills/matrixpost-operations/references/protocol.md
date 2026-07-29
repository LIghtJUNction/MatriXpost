# MatriXpost protocol reference

Read current Rust source before changing a contract. This reference records implemented surfaces; local runner outcomes never prove remote publication.

## Publication compatibility

Serialize these canonical video platform codes: `dy`, `sph`, `blbl`, `bjh`, `tt`, `ks`, `xhs`, and `fqsp`. Preserve accepted aliases in core without adding another canonical code absent an explicit compatibility decision.

`matrixpost` writes one JSON document to stdout. Its publication commands validate local intent; `login` is unavailable, video `publish` requires an explicit eligible loopback provider runner, and `publish-article` accepts Juejin only and requires one explicit eligible article runner. `accounts`, `history`, and `providers` expose credential-free local state.

`matrixpostd` defaults to `127.0.0.1:8788`. `GET /`, `GET /health`, `GET /platforms`, `GET /providers`, `GET /creative-statements`, `POST /changeData`, and `POST /publish` retain the MatrixMedia-compatible publication surface. `POST /publish` returns local availability/queue truth, never remote publication proof.

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

`matrixpost-mcp` uses the same SQLite state as CLI. Resolve state in this order: `--state-path`, `MATRIXPOST_STATE_PATH`, then `matrixpost.db` in the current directory. Keep stdout exclusively MCP JSON-RPC; send optional diagnostics only to stderr through `MATRIXPOST_MCP_LOG=1`.

There are fourteen typed tools. Tool names are snake_case. All typed tool inputs use camelCase field names (for example `businessObjectId` and `expectedRevision`); returned core records preserve their Rust serialization with snake_case fields (for example `business_object_id`, `lifecycle_status`, and `amount_minor`).

| Group | Tools |
| --- | --- |
| Publication (4) | `list_accounts`, `list_history`, `publish_video`, `publish_article` |
| Lifecycle (10) | `list_business_objects`, `get_business_object`, `create_business_object`, `list_ledger_entries`, `append_ledger_entry`, `list_content_attributions`, `add_content_attribution`, `list_business_relations`, `add_business_relation`, `transition_business_object` |

Lifecycle examples use `create_business_object` with `{id, kind, displayName, externalId?, lifecycleStatus?, approvalStatus?, attributes?}`; `append_ledger_entry` with `{id, businessObjectId, direction, category, amountMinor, currency, ...}`; `add_business_relation` with `{id, sourceBusinessObjectId, targetBusinessObjectId, relationType, attributes?}` after creating both objects; and `transition_business_object` with `{id, expectedRevision, lifecycleStatus, approvalStatus, updatedAt?}`. Reject unknown input fields. Relations are directed, immutable, non-self links and list from either endpoint. Return `not_found` for a missing object or history record; return an empty list only for an existing object with no corresponding entries, links, or relations.

Reject lifecycle attribute key names such as `token`, `cookie`, `password`, and `session`. Do not scan ordinary attribute values heuristically. Never accept or return named credential, cookie, token, password, or session fields. Lifecycle tools are generic local SQLite state management only: do not invoke a provider, browser, runner, remote publishing path, or agent runtime.

## Tauri and service delivery

The Tauri v2 desktop shell opens application-data `matrixpost.db`; it starts no daemon, shell, browser, provider, or runner. It exposes fourteen IPC commands: `desktop_snapshot`, `save_local_draft`, `save_account`, `save_article_account`, `local_history`, `lifecycle_objects`, `create_lifecycle_object`, `lifecycle_ledger_entries`, `append_lifecycle_ledger_entry`, `lifecycle_content_attributions`, `add_lifecycle_content_attribution`, `lifecycle_business_relations`, `add_lifecycle_business_relation`, and `transition_lifecycle_object`. The lifecycle UI selects an existing object as source and offers only other local objects as targets, preventing a self relation before IPC. Project only safe display metadata and local lifecycle records to the frontend.

Use `deploy/matrixpostd.service` and `deploy/matrixpostd.example.toml` as the systemd deployment starting point. Do not install or enable it automatically. Keep its bind address intentional and its configuration secret-free.

The repository includes CI for locked workspace checks, public-package metadata, a core package archive, and macOS/Windows desktop compilation checks. Version 0.3.1 of the four public crates is published on crates.io and the v0.3.1 GitHub Release exists. The desktop and WebDriver runner crates are not crates.io packages. Validate the current checksum-pinned PKGBUILD source archive locally before any AUR operation; do not claim an AUR remote upload without inspecting the remote result. No authenticated live-platform acceptance proof exists, and no `.cursor` directory is used.
