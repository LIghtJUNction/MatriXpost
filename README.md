# MatriXpost

MatriXpost is a GPL-2.0-only Rust rebuild of the publication workflow scope of
[hanliang97/MatrixMedia](https://github.com/hanliang97/MatrixMedia). We thank
the upstream project for its product direction and platform research.

## Current status

The first portable domain slice is implemented: the workspace contains a shared
Rust core, a JSON-first CLI, a headless HTTP daemon, and a local stdio MCP
server. The core losslessly
uses the upstream platform codes `dy`, `sph`, `blbl`, `bjh`, `tt`, `ks`, `xhs`,
and `fqsp`, while accepting English and Chinese aliases. It models account
routing, titles, tags, address, drafts, exact local schedules, WeChat link
metadata, and per-platform creative overrides. Invalid requests are rejected
before a provider can be invoked.

The server state is SQLite-backed and migrates on open. It safely persists
credential-free account metadata, immutable history, and revisioned scheduled
jobs; scheduler transitions are transactional and reject stale revisions. The
daemon accepts `--config <toml>` and `--state-path <sqlite path>`. Remote media
is represented by a strict `http`/`https` staging boundary with a bounded
metadata policy, bounded HTTP staging, and explicit cleanup ownership; staging
is not invoked by any provider.

`matrixpost` retains the upstream command names: `login`, `publish`,
`publish-article`, `accounts`, and `history`. Commands write one JSON document
to stdout. Exit codes are: `0` completed local query, `2` invalid input, `3`
provider unavailable, and `4` internal failure; `1` has no stable meaning yet.
The initial implementation intentionally returns `unavailable` for login or
publishing: it does **not** claim real browser publishing without a provider
adapter. A CLI caller may declare local runner endpoints with repeated
`--provider-runner PLATFORM=tcp:127.0.0.1:PORT` (or the documented Unix-socket
and Windows-named-pipe forms); the daemon accepts the same declarations in
`provider_runners` TOML. These declarations are validated as local and
credential-free, and are visible through `matrixpost providers --json` and
`GET /providers`. A TCP declaration dispatches only to a separately started
`matrixpost-webdriver-runner` through its versioned `/v1/publish` protocol;
MatriXpost never starts that runner. Unix-socket and Windows-named-pipe
declarations stay unavailable until audited transport adapters exist. A queued
result means the local runner completed its WebDriver workflow; it is not a
claim that the platform has completed remote media processing.

Juejin articles use a separate, deliberately opt-in local route:
`--article-runner tcp:127.0.0.1:PORT`. The core strips account routing before
posting its versioned request only to `/v1/publish-article`; it never accepts
browser profiles, sessions, or credentials in this declaration. Without that
flag, `publish-article` remains unavailable and makes no runner attempt. Article
scheduling is not implemented: any article with `--publish-at` is rejected at
the local runner boundary and is never executed immediately.

This route is an explicit local opt-in, not evidence of a Juejin login or live
publication capability. The repository has no authenticated or live-platform
article proof. Its runner coverage uses mocked WebDriver selector and
acknowledgement responses; platform UI changes, account state, and remote
processing remain unverified until an authorized, separately managed
acceptance run.

## Local WebDriver runner

`matrixpost-webdriver-runner` is an opt-in local bridge for the eight video
platforms. It accepts only `PublishRequest` values on loopback TCP and only
talks to an explicitly configured **loopback HTTP** WebDriver endpoint. It
rejects remote endpoints, endpoint credentials, query/fragment data, and any
endpoint path that appears to name session, credential, or browser-profile
material. It does not accept, read, write, log, or transmit cookies, passwords,
tokens, sessions, or browser-profile paths.

Start a Chrome instance yourself with a **loopback-only remote-debugging port**
and the browser state you personally manage, then start ChromeDriver (or a
compatible WebDriver). The runner must receive both loopback addresses so
ChromeDriver attaches to that existing browser through
`goog:chromeOptions.debuggerAddress`; it never starts a fresh browser profile.
Without `--browser-debugger-address`, the runner remains healthy but every
publish request returns `unavailable` and no WebDriver session is created.

```bash
matrixpost-webdriver-runner \
  --bind 127.0.0.1:39001 \
  --webdriver-endpoint http://127.0.0.1:9515 \
  --browser-debugger-address 127.0.0.1:9222
matrixpost --provider-runner dy=tcp:127.0.0.1:39001 \
  publish -p dy -f /absolute/path/video.mp4 -t "Title"
matrixpost-webdriver-runner \
  --bind 127.0.0.1:39002 \
  --webdriver-endpoint http://127.0.0.1:9515 \
  --browser-debugger-address 127.0.0.1:9222 \
  --allow-article-publish
matrixpost --article-runner tcp:127.0.0.1:39002 \
  publish-article -p juejin -t "Article title" --content "Body"
```

The runner uses generic WebDriver commands and static, ordered CSS selector
fallbacks for upload, title, description, draft, publish, and post-click
success indicators. Platform UIs can change without notice; every phase,
including the bounded success acknowledgement check, must succeed before the
runner returns `queued`. The acknowledgement deadline is a fixed five minutes
(60 checks at five-second intervals); this intentionally differs from the
short element-discovery retry and has no CLI override. A success marker already
visible before the click, or one that remains hidden, is rejected. It always
attempts to close its WebDriver session. It supports only local media files;
remote media staging is intentionally outside this runner.
For articles, `--allow-article-publish` is a separate startup opt-in. Before
any WebDriver session creation, local validation failures report
`automation_attempted:false`. Once session creation, attach, or navigation has
begun, a rejected result reports `automation_attempted:true`, so callers cannot
mistake a failed local automation attempt for a no-op. This remains local
runner state, never confirmation of remote publication.
Do not expose either endpoint beyond loopback. Treat manual browser login,
platform terms, upload limits, review, and final platform-side processing as
your responsibility.

`matrixpostd` exposes `GET /`, `GET /health`, `GET /platforms`, `GET /providers`, `GET
/creative-statements`, and `POST /changeData`, `POST /publish`. `/publish`
accepts MatrixMedia-compatible JSON (`platform`/`platforms`, `file`,
`publishAt`, `sphProductId`, `sphLink`, and `platformOptions`) but returns
`503` with `accepted:false` when no provider is installed; it does not queue or
publish media. `/changeData` durably supports `add`, `update`, `delete`, `get`,
and `config` for supported non-secret `account`, `pushData`, and metadata
records using `fileName`, `type`, and structured `item`. Invalid input returns `400`.

`matrixpost accounts --json` and `matrixpost history --json` read the durable
state. `history` defaults to the latest seven days and accepts `--days`,
`--platform`, `--status` (`success`, `failed`, `publishing`, or `scheduled`),
and `--all`; filters intersect, while `--all` removes the cutoff. The CLI
retains the upstream command names and preserves all parsed publish fields in
the typed core request.

## MCP server

`matrixpost-mcp` is an MCP stdio server built with the official Rust SDK. It
uses the same local SQLite state as the CLI. By default it does not spawn
`matrixpostd`, execute a shell command, or connect to any runner. With an
explicit loopback `--article-runner`, it can call that separately started local
article runner, but it never connects directly to a platform, browser, or
provider automation endpoint and never uses browser/session data. Its stdout
is reserved for MCP JSON-RPC frames. Diagnostic logging is disabled by default
and, when explicitly enabled with `MATRIXPOST_MCP_LOG=1`, is written only to
stderr.

Configure an MCP client to start the installed `matrixpost-mcp` binary with
the desired state path. Add `--article-runner tcp:127.0.0.1:PORT` only when a
separately started runner has also opted in with `--allow-article-publish`:

```json
{
  "mcpServers": {
    "matrixpost": {
      "command": "matrixpost-mcp",
      "args": ["--state-path", "/absolute/path/to/matrixpost.db", "--article-runner", "tcp:127.0.0.1:39002"]
    }
  }
}
```

`--state-path <path>` takes precedence over `MATRIXPOST_STATE_PATH`; when
neither is supplied the server uses `matrixpost.db` in its working directory.
The server exposes the four upstream tool names: `list_accounts`,
`list_history`, `publish_video`, and `publish_article`. Account and history
tools read only credential-free SQLite metadata. `publish_video` validates and
records a local draft/queued job only for `dy`, `ks`, `blbl`, `bjh`, `tt`, or
`sph`, then returns `provider_available:false` and `remote_publish_attempted:false`;
no provider automation is attempted. `list_accounts` accepts the exact upstream
account set (`dy`, `ks`, `blbl`, `bjh`, `tt`, `sph`, `xhs`, `juejin`, `fqsp`)
and returns a direct JSON array. Juejin entries are stored in a separate,
credential-free SQLite registry (`phone`, platform, `partition`, plus local display/status metadata), so a
fresh database returns none and persisted safe metadata is listed normally.
`publish_article` accepts only `juejin`, normalizes its upstream tag string, and remains explicitly
unavailable unless that loopback article runner is configured. A queued result
means only that the local runner completed its WebDriver workflow; remote
publication is not confirmed. Article `publishAt` is validated for upstream
compatibility but cannot be dispatched: scheduled articles are rejected rather
than published immediately because article scheduling is not implemented.
Video `publishAt` accepts `YYYY-MM-DD HH:mm` or seconds; article `publishAt`
also accepts `HH:mm`, normalized to the current local calendar date with zero
seconds. For video-channel links, `sphLink.type` is exactly `none` or `product`;
the latter requires `value`, while `sphProductId` takes precedence and creates
the effective product link.
Neither tool accepts cookies, passwords, tokens, sessions, or credentials.

## Desktop

`matrixpost-desktop` is a Tauri v2 shell for Linux, macOS, and Windows. It
opens `matrixpost.db` below the operating system application-data directory,
directly through `matrixpost-core`; it does not start `matrixpostd` or any
provider process. Its overview exposes only credential-free account/history
metadata, and its form can save a validated **local draft**. The desktop UI
plainly reports that provider automation is unavailable and has no remote
publish action.

Run the local shell during development from the workspace root:

```bash
cargo run -p matrixpost-desktop
```

Build the native bundle on each target platform (with the Tauri CLI installed):

```bash
cd crates/matrixpost-desktop
cargo tauri build
```

Delivery evidence is intentionally narrow: a Linux amd64 `.deb` bundle has
been generated and inspected locally, but it has not been installed or
launched. CI only configures native macOS and Windows desktop compilation, and
those jobs have not run; there is no macOS or Windows bundle or runtime
evidence.

The static frontend uses Tauri's injected global IPC bridge
(`withGlobalTauri:true`) because it has no Node dependency or bundler. That
bridge reaches only five typed Rust commands: `desktop_snapshot`,
`save_local_draft`, `save_account`, `save_article_account`, and
`local_history`. The snapshot projects video and Juejin accounts to safe
display fields and reports only a history count; history projects entries to
id, state, time, title, targets, and local draft/schedule intent. It never
exposes media paths, account routing, serialized requests, sessions, or
credentials to the frontend. The default capability grants `core:default`
only: no shell, filesystem, HTTP, or remote URL plugin permission is
configured. The shell has no runner configuration or remote-dispatch UI.

## Delivery plan

- **Server:** `matrixpostd` is designed for headless deployment. See
  `deploy/matrixpostd.service` and its secret-free example configuration. The
  unit uses a dynamic service user, a dedicated state directory, and systemd
  hardening; it is supplied only, never installed automatically.
- **Arch Linux:** the release recipe is
  [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD); it builds only the
  headless binaries (`matrixpost`, `matrixpostd`, and `matrixpost-mcp`) from a
  version tag with Cargo's locked dependency graph. It installs
  `matrixpostd.service` but never enables or starts it. After installing a
  released AUR package, opt in explicitly with
  `sudo systemctl enable --now matrixpostd.service`. The service owns its
  state under `/var/lib/matrixpost` through `DynamicUser`; its network policy
  permits only IPv4/IPv6 loopback, which retains the daemon API and local
  runner endpoints while preventing external network access. Use the documented
  example configuration only as a starting point and keep it credential-free.
  This repository does not upload to AUR yet.
- **Cargo:** `matrixpost-core`, `matrixpost-cli`, `matrixpostd`, and
  `matrixpost-mcp` are versioned public crates. The Tauri desktop shell is
  intentionally not published to crates.io: it is a platform bundle, not a
  reusable library or `cargo install` target. CI compiles/tests the workspace,
  validates its locked public-package metadata, and creates only the
  independently publishable `matrixpost-core` archive. It does not claim to
  create normalized archives for dependent crates before their unpublished core
  dependency exists on crates.io. The first release is staged: publish
  `matrixpost-core`, wait for crates.io availability, then package-verify and
  publish `matrixpost-cli`, `matrixpostd`, and `matrixpost-mcp` serially. No
  crate has been published yet.

No `.cursor` directory is used or required.

## License

This project is licensed under GPL-2.0-only; see [LICENSE](LICENSE).
