---
name: matrixpost-operations
description: This skill should be used when the user asks to "publish with MatriXpost", "run matrixpostd", "manage MatriXpost lifecycle objects", "add a MatriXpost MCP server", "use the MatriXpost CLI", "build the Tauri desktop app", "install the MatriXpost systemd service", "package MatriXpost for AUR", or "release MatriXpost to Cargo or GitHub".
---

# MatriXpost Operations

Operate MatriXpost as a Rust product with separate publication and operating-lifecycle surfaces. Preserve the boundary between local records, local runner workflows, and confirmed remote platform outcomes. Do not introduce an agent runtime, secret store, browser automation shortcut, or `.cursor` directory.

## Select a surface

Use `matrixpost` for deterministic local JSON commands. Use `matrixpostd` for the HTTP service and systemd deployment. Use `matrixpost-mcp` for stdio MCP clients. Use the Tauri desktop app for local state and lifecycle UI. Keep `matrixpost-core` free of network, browser, provider, and UI side effects.

Treat the generic lifecycle model as the common foundation for a vehicle, asset, project, product, or other business object. Do not hardcode VIN or a vertical-specific schema into the shared model. Use a vertical template only at the caller boundary.

Read `references/protocol.md` before composing a public CLI, HTTP, MCP, desktop, runner, or release command. Treat source and tests as authoritative if they disagree with this reference.

## Preflight

1. Inspect the repository state, selected state path, and current implementation.
2. Query `matrixpost accounts --json` and `matrixpost history --json` before planning a publication operation. Treat empty local data as normal for a fresh database, not as provider evidence.
3. Check the daemon bind address, state path, and health endpoint before service changes. Keep non-loopback exposure behind an explicit reverse-proxy or firewall decision.
4. Validate identifiers, platform codes, titles, local scheduling, and runner declarations before runner work.
5. Stop if an operation would print, copy, commit, or place a token, cookie, password, session, browser profile, or other secret in configuration, state projection, documentation, or logs.

## Operate publication safely

Require fresh explicit approval naming targets and publication intent immediately before real provider dispatch. Do not treat build, queue, schedule, draft, inspection, or test permission as publication permission.

Treat `unavailable` as a correct result when no eligible loopback runner exists. Do not start browsers, invoke websites directly, or fabricate remote success. Treat `queued` as completion of a local runner workflow only; it never proves platform-side acceptance, publication, or processing.

Use repeatable `--provider-runner PLATFORM=tcp:127.0.0.1:PORT` declarations only for the separately started video runner. Use exactly one `--article-runner tcp:127.0.0.1:PORT` for Juejin and require that runner to have started with `--allow-article-publish`. Keep all runner addresses loopback and credential-free.

## Operate generic lifecycle records

Create an object before adding ledger entries, content attribution, or relations. Use caller-defined `kind`, stable `id`, optional external identifier, and safe attributes. Keep attributes descriptive; reject sensitive key names rather than guessing from ordinary business text.

Create a relation only after both local endpoint objects exist. Model it as immutable and directed: source object, target object, caller-defined type, and safe attributes. Do not permit source and target to be the same object. List relations from either endpoint to obtain both inbound and outbound links.

Append ledger entries rather than editing history. Record expense or revenue in integer minor currency units, with category and approval state. Link content only to an existing local history record. Use the returned object revision for every transition; reload after a stale-revision rejection instead of retrying blindly.

Treat a missing object or history record as `not_found`. Treat an existing object with no ledger entries, attribution links, or inbound/outbound relations as a successful empty list. Keep lifecycle work local to SQLite: it must not invoke a provider, runner, browser, remote publication, secret source, or agent framework.

## Deploy and release deliberately

Preserve the supplied systemd hardening model: a dedicated state directory, least privilege, secret-free configuration, and intentional bind address. Do not install, enable, restart, or expose the unit without explicit approval.

Run relevant format, tests, lint, documentation, package/build checks, and CI before a release. Treat GitHub, crates.io, AUR, and server writes as external actions requiring explicit authorization. Publish dependent public crates only after `matrixpost-core` is visible to crates.io.

Version `0.3.0` is the latest coordinated lifecycle release: `matrixpost-core`, `matrixpost-cli`, `matrixpostd`, and `matrixpost-mcp` are published on crates.io, and the v0.3.0 GitHub Release exists. Validate any AUR recipe against its exact immutable source archive and checksum; a local recipe or GitHub commit is not evidence of an AUR remote upload.

## Reference

Load **`references/protocol.md`** for exact CLI subcommands, HTTP routes, MCP tool and field names, desktop IPC commands, lifecycle invariants, and delivery evidence. Update that reference in the same change as any public-surface change.
