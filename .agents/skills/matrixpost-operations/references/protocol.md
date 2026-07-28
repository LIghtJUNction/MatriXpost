# MatriXpost protocol reference

Read the current Rust source before changing a contract. This reference records
the implemented local surface; a runner result is never proof of platform-side
publication or processing.

## Canonical platform codes

| Code | Platform | Accepted aliases in the core |
| --- | --- | --- |
| `dy` | Douyin | `douyin`, `抖音` |
| `sph` | WeChat Channels | `wechat_channels`, `wechat`, `视频号`, `微信视频号` |
| `blbl` | Bilibili | `bilibili`, `哔哩哔哩`, `b站` |
| `bjh` | Baijiahao | `baijiahao`, `百家号` |
| `tt` | Toutiao | `toutiao`, `今日头条`, `头条` |
| `ks` | Kuaishou | `kuaishou`, `快手` |
| `xhs` | Xiaohongshu | `xiaohongshu`, `小红书` |
| `fqsp` | Fanqie Video | `fanqie_video`, `fanqie-video`, `fanqie`, `fq`, `番茄视频` |

Serialize the first-column codes. Do not add a ninth platform or change a
canonical code without an explicit compatibility decision.

## CLI and loopback runners

`matrixpost` emits one JSON document on stdout. Exit code `0` means a completed
local query or local runner workflow, `2` invalid input, `3` unavailable, and
`4` rejected or internally failed dispatch. Exit code `1` has no stable meaning.

| Command | Current behavior |
| --- | --- |
| `login --platform <code>` | Validates then returns unavailable; no login occurs. |
| `publish --platform <code> --file <path-or-http-url> --title <title>` | Validates a typed video request. Without a runner it is unavailable. |
| `publish-article --platform juejin --title <title> (--content <text>\|--file <path>)` | Validates a typed Juejin request. Without an article runner it is unavailable. |
| `accounts --json` | Returns credential-free video account metadata from SQLite. |
| `history --json [--days N] [--platform <code>] [--status <status>] [--all]` | Returns filtered local history; default is seven trailing days. `fqsp` is not a history filter. |
| `providers --json` | Shows deterministic video-provider availability. |

Video runners are declared with repeatable
`--provider-runner PLATFORM=tcp:127.0.0.1:PORT` (or the validated but currently
unavailable Unix-socket/named-pipe forms). A TCP declaration posts only to a
separately started loopback `matrixpost-webdriver-runner` at `/v1/publish`; the
CLI never starts it. Juejin uses exactly one
`--article-runner tcp:127.0.0.1:PORT`, which posts only to
`/v1/publish-article`. Both addresses must be loopback and credential-free.
Account routing is stripped before either runner request.

The video runner needs both a loopback WebDriver endpoint and a loopback
browser DevTools address. It attaches ChromeDriver to the user-managed browser;
it does not start a browser profile. Article dispatch additionally requires the
runner startup flag `--allow-article-publish`. Without a configured or eligible
runner, no runner attempt occurs. Article schedules are rejected before
WebDriver execution. `queued` means only that the local runner completed its
workflow; it never confirms remote publication. No authenticated live-platform
acceptance proof exists.

## Daemon HTTP surface

`matrixpostd` defaults to `127.0.0.1:8788` and uses SQLite selected by
`--state-path` (or its runtime default). Its credential-free TOML
`provider_runners` accepts the same video declarations as the CLI. TCP entries
can call the separately started runner; Unix sockets and named pipes are still
unavailable.

| Method and path | Current behavior |
| --- | --- |
| `GET /`, `GET /health` | Return healthy local-service status. |
| `GET /platforms` | Return platform metadata and availability fields. |
| `GET /providers` | Return configured provider availability. |
| `GET /creative-statements` | Returns `503 unavailable`. |
| `POST /changeData` | Persists supported non-secret `account`, `pushData`, and metadata records. |
| `POST /publish` | Parses MatrixMedia-compatible JSON and dispatches the configured video registry. All unavailable yields `503`; all local runner workflows queued yields `202`; mixed outcomes yield rejection. None confirms remote publication. |

The publish DTO supports `platform`/`platforms`, string `file`, `publishAt`,
`sphProductId`, `sphLink`, and `platformOptions`; it is mapped to the typed
core request. Remote-media staging is bounded but is not invoked by a provider.

## MCP stdio surface

`matrixpost-mcp` opens the same SQLite state as CLI. Its stdout is exclusively
MCP JSON-RPC; `MATRIXPOST_MCP_LOG=1` enables diagnostics only on stderr. State
selection is `--state-path`, then `MATRIXPOST_STATE_PATH`, then
`matrixpost.db` in the current directory. It has no shell, daemon-spawn, or
credential/session path.

| Tool | Current behavior |
| --- | --- |
| `list_accounts` | Lists safe routing fields `{phone, platform, partition}` for exact supported platforms. |
| `list_history` | Applies the same local seven-day/default, platform, status, and all-history filtering as the CLI. |
| `publish_video` | Validates and persists a local video draft/queued job for `dy`, `ks`, `blbl`, `bjh`, `tt`, or `sph`; it does not invoke video automation. |
| `publish_article` | Accepts only Juejin. Without `--article-runner tcp:127.0.0.1:PORT`, returns unavailable with no runner attempt. With it, forwards only through the same local runner contract and reports queued, unavailable, or rejected truthfully. |

MCP article scheduling is rejected by the runner before browser work. Its
`queued` output only describes local runner completion. Inputs reject unknown
fields; cookies, passwords, tokens, sessions, and credentials are never
accepted or returned.

## Desktop and delivery artifacts

`matrixpost-desktop` is an implemented Tauri v2 shell for Linux, macOS, and
Windows. It opens application-data `matrixpost.db` and exposes only
`desktop_snapshot`, `save_local_draft`, `save_account`, `save_article_account`,
and `local_history`. Account snapshot entries are display metadata; history
entries are id, state, timestamp, title, targets, and local intent. Media
paths, phone/partition routing, serialized requests, sessions, and credentials
are not projected to the frontend. The desktop starts no daemon, shell,
browser, provider, or runner and has no remote-dispatch UI.

The repository contains `.github/workflows/ci.yml`,
`deploy/matrixpostd.service`, `deploy/matrixpostd.example.toml`, and
`packaging/arch/PKGBUILD`. CI defines Linux workspace checks, locked
public-package metadata validation, and a `matrixpost-core` package archive,
plus macOS/Windows desktop type checks. It intentionally does not package
dependent crate archives until `matrixpost-core` is available on crates.io.
The only desktop bundle evidence is a locally generated and inspected Linux
amd64 `.deb`; it was not installed or launched. CI run 30376876522 passed
native macOS and Windows desktop compilation, but neither platform has bundle
or runtime evidence.
The first Cargo release publishes core first, then package-verifies/publishes
CLI, daemon, and MCP serially. The systemd unit is supplied but never installed
or enabled automatically. The PKGBUILD is an unreleased tag-based AUR recipe.
The four public crates are prepared for Cargo packaging; desktop and WebDriver
runner crates are not published. GitHub commits/pushes and CI run 30376876522
have occurred; no GitHub release, crates.io publication, AUR upload, server
install/enablement, or authenticated live-platform draft acceptance has been
performed. No `.cursor` directory is used.
