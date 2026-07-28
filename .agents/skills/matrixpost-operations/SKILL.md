---
name: matrixpost-operations
description: This skill should be used when the user asks to "publish with MatriXpost", "run matrixpostd", "check MatriXpost accounts or history", "add a MatriXpost MCP server", "build the Tauri desktop app", "package MatriXpost for AUR", "install the MatriXpost systemd service", or "release MatriXpost to Cargo or GitHub".
version: 0.1.0
---

# MatriXpost Operations

Operate and extend MatriXpost without representing planned adapters as live
automation. Keep publication intent, credentials, browser state, and release
authority distinct.

## Select the right surface

Use `matrixpost` for a local, one-shot JSON command. Use `matrixpostd` for a
long-running server process and its HTTP contract. Use `matrixpost-core` when
adding domain rules, persistence, scheduling, or provider ports. The Tauri
desktop shell is present for local state, local drafts, account metadata, and
filtered local history; it has no runner configuration or remote-dispatch UI.
`matrixpost-mcp` is a real stdio server with four typed tools, but it stays
credential-free and must not be treated as authenticated platform automation.

Read `references/protocol.md` before composing a CLI invocation, HTTP request,
provider adapter, or compatibility test. Use only its canonical platform codes
on public wire surfaces.

## Preflight before an operation

1. Inspect repository state and current implementation before relying on this
   skill; source and tests are authoritative.
2. Query `matrixpost accounts` and `matrixpost history` against the configured
   SQLite state store before proposing a publish workflow. Treat an empty
   result as normal only for a fresh database; do not treat it as proof that a
   provider or account is configured.
3. For a daemon, check its bind address, state path, and health endpoint before
   changing service configuration. Keep public exposure behind an explicitly
   approved reverse proxy or firewall decision.
4. Validate platform codes, title, media source, target uniqueness, and local
   scheduling format before any adapter call.
5. Stop when credentials, cookies, browser profiles, or a secret file would be
   printed, copied, committed, or placed in a sample config.

## Publication safety

Require a fresh, explicit user confirmation that names the targets and intent
immediately before any real provider dispatch. Do not treat an earlier request
to build, test, queue, schedule, or inspect as publish authorization. Report
the exact outcome per target and preserve durable history when adapters exist.

Without a configured loopback runner, CLI login and publication commands
return `unavailable`; do not work around that response with browser automation,
direct website calls, or fabricated success. For video, only explicit
`--provider-runner PLATFORM=tcp:127.0.0.1:PORT` dispatches to a separately
started loopback WebDriver runner. For Juejin articles, exactly one explicit
`--article-runner tcp:127.0.0.1:PORT` is required in the CLI or MCP process and
the runner itself must have started with `--allow-article-publish`. The default
is unavailable and makes no runner attempt. A `queued` result proves only that
the local runner workflow completed; it never proves platform-side publication
or processing. Scheduled articles are rejected before WebDriver execution.

The daemon also accepts credential-free `provider_runners` TOML declarations.
Its TCP declarations dispatch to the separately started video runner; its
Unix-socket and Windows-named-pipe declarations remain unavailable. `/publish`
returns `503 unavailable` when every selected provider is unavailable, and a
`202 queued` response still does not describe remote publication.

## Development and release boundaries

Keep desktop, MCP, provider, remote-media, and service work behind explicit
interfaces and tests. Do not add network or browser side effects to
`matrixpost-core`. Do not create `.cursor` or introduce secrets into source,
tests, examples, generated files, commits, CI variables, or logs.

Before a GitHub, Cargo, or AUR release, require passing relevant Rust tests,
format/lint checks, package/build checks, and CI evidence. Verify version,
license, repository URL, artifacts, installation instructions, and platform
support against the actual tree. Treat publication to GitHub, crates.io, AUR,
or a server as an external write requiring explicit user authorization.

Current desktop delivery evidence is limited to a generated and locally
inspected Linux amd64 `.deb` bundle; it was not installed or launched. Native
macOS/Windows desktop compilation is verified in CI, but neither platform has
bundle or runtime evidence.

For the first Cargo release, do not read CI's core-only package archive as
proof that dependent crate archives exist. Publish `matrixpost-core` first and
wait until crates.io resolves it, then run `cargo package --locked --no-verify`
and publish `matrixpost-cli`, `matrixpostd`, and `matrixpost-mcp` serially.
CI validates the locked workspace metadata for those dependent packages but
cannot normalize their crate archives while the core dependency is unpublished.

For systemd deployment, preserve the supplied hardening model: least privilege,
dedicated state directory, secret-free configuration, and an intentional bind
address. Do not install, enable, restart, or expose the unit without explicit
approval. The repository includes a CI workflow, a systemd unit, and an Arch
PKGBUILD, but no remote CI run, GitHub release, crates.io publication, or AUR
upload is evidence until it is actually performed and checked.

## Reference loading

Load `references/protocol.md` for the exact platform mapping and the current
CLI/HTTP/MCP/desktop capability tables. Refresh that reference in the same
change whenever the public surface changes; distinguish implemented local
artifacts from unperformed external releases and unproven live-platform work.
