# MatriXpost

MatriXpost is a GPL-2.0-only Rust rebuild of the publication workflow scope of
[hanliang97/MatrixMedia](https://github.com/hanliang97/MatrixMedia). We thank
the upstream project for its product direction and platform research.

## Current status

The workspace contains a shared Rust core, a JSON-first CLI, a headless HTTP
daemon, a local stdio MCP server, and a Tauri desktop shell. Publishing remains
the primary workflow. Alongside it, the portable core provides generic local
full-lifecycle management: business objects with caller-defined kinds and
optional external IDs, typed lifecycle and approval states, an append-only
minor-unit revenue/expense ledger, and links from publication history to those
objects. This is deliberately not coupled to a vehicle, VIN, or any other
vertical-specific schema. The core losslessly
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
`login` is a manual handoff, not a login-success assertion. Use a matching,
separately started loopback runner: `matrixpost --provider-runner
<platform>=tcp:127.0.0.1:<port> login --platform <platform>`. It sends only a
versioned platform request to that runner's `/v1/login`; an `opened` outcome
means the user must finish login manually in the already user-managed browser.
It does not confirm a completed login or any publication. An absent or non-TCP
runner returns `unavailable`; a rejected or failed handoff is not success.

A CLI caller may declare local runner endpoints with repeated
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
Manual-login navigation is separately opt-in with `--allow-login-navigation`;
it uses that same attached browser only to open the platform page, then leaves
the user to complete login manually. It never extracts a profile, cookie, or
session.

```bash
matrixpost-webdriver-runner \
  --bind 127.0.0.1:39001 \
  --webdriver-endpoint http://127.0.0.1:9515 \
  --browser-debugger-address 127.0.0.1:9222 \
  --allow-login-navigation
matrixpost --provider-runner dy=tcp:127.0.0.1:39001 \
  login --platform dy
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

`matrixpost lifecycle` manages the same local generic lifecycle state. It can
create, list, and retrieve objects; append or list immutable ledger entries;
link or list publication-history attributions; and apply revision-guarded
lifecycle/approval transitions. Ledger amounts are integer minor units. A
transition requires the current revision, so stale updates are rejected rather
than silently overwriting another local change.

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
The MCP server exposes fourteen tools: the four upstream tool names `list_accounts`,
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
the effective product link. With the explicit local WebDriver runner attached,
only WeChat Channels (`sph`) can apply that effective product link. A nonempty
`sphProductId` wins even when `sphLink.type` is `none`; otherwise `none` does
nothing, and `product` requires a nonempty value. The bounded local workflow
selects the product in the already user-managed browser and closes its temporary
session; a queued result still does not confirm the platform accepted the video
or product association.
For WeChat Channels only, a nonblank effective `shortTitle` is written to its
dedicated short-title field, while a supported target-specific
`creativeStatement` selects the corresponding platform label instead of being
silently appended to the description. During a user-requested WeChat publish,
the runner also follows the upstream optional original-declaration dialog flow:
no entry or dialog is a safe skip, but a dialog that appears must be completed
and disappear before draft/publish can continue. These are bounded local
browser actions, not proof of an accepted declaration, review, or publication.
Neither tool accepts cookies, passwords, tokens, sessions, or credentials.
The other ten tools are `list_business_objects`, `get_business_object`,
`create_business_object`, `list_ledger_entries`, `append_ledger_entry`,
`list_content_attributions`, `add_content_attribution`, and
`list_business_relations`, `add_business_relation`, and
`transition_business_object`. They operate exclusively on local SQLite state;
they do not open a browser, invoke a provider, or claim remote publication.

## Desktop

`matrixpost-desktop` is a Tauri v2 shell for Linux, macOS, and Windows. It
opens `matrixpost.db` below the operating system application-data directory,
directly through `matrixpost-core`; it does not start `matrixpostd` or any
provider process. Its overview exposes only credential-free account/history
metadata, and its form can save a validated **local draft**. The desktop UI
also provides an explicit-confirmation, one-shot local-runner dispatch form.
It accepts runner declarations only for that invocation, does not start a
runner or browser, and persists neither runner nor browser configuration. Its
outcomes describe only local runner results and always report remote publication
as unconfirmed. A separate local-runner diagnostics panel can infer upload-form
readiness or query a bounded Fanqie title's review state through an explicitly
entered, matching loopback runner declaration. Each check requires confirmation,
never persists the declaration or title, and returns only a safe status enum;
it neither starts a runner or browser nor proves a completed login or remote
publication.

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
launched. Native macOS and Windows desktop compilation is verified in CI, but
there is no macOS or Windows bundle or runtime evidence.

The static frontend uses Tauri's injected global IPC bridge
(`withGlobalTauri:true`) because it has no Node dependency or bundler. That
bridge reaches seventeen typed Rust commands: `desktop_snapshot`,
`save_local_draft`, `dispatch_to_local_runner`, `save_account`,
`save_article_account`, `local_history`, `account_readiness`, and
`fanqie_review_status`, plus `lifecycle_objects`, `create_lifecycle_object`,
`lifecycle_ledger_entries`, `append_lifecycle_ledger_entry`,
`lifecycle_content_attributions`, `add_lifecycle_content_attribution`, and
`lifecycle_business_relations`, `add_lifecycle_business_relation`, and
`transition_lifecycle_object`. The lifecycle commands use the same local
SQLite generic-object, immutable-ledger, attribution, and revision-guarded
transition model as the CLI, daemon, and MCP server. The snapshot projects video and Juejin accounts to safe
display fields and reports only a history count; history projects entries to
id, state, time, title, targets, and local draft/schedule intent. It never
exposes media paths, account routing, serialized requests, sessions, or
credentials to the frontend. The default capability grants `core:default`
only: no shell, filesystem, HTTP, or remote URL plugin permission is
configured. The shell has no persisted runner configuration or remote-dispatch
success state. The one-shot form sends only explicitly entered loopback runner
declarations after confirmation.

## Delivery plan

- **Server:** `matrixpostd` is designed for headless deployment. See
  `deploy/matrixpostd.service` and its secret-free example configuration. The
  unit uses a dynamic service user, a dedicated state directory, and systemd
  hardening; it is supplied only, never installed automatically.
- **Arch Linux:** the release recipe is
  [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD); it builds only the
  headless binaries (`matrixpost`, `matrixpostd`, and `matrixpost-mcp`) from
  a checksum-pinned immutable commit archive with Cargo's locked dependency
  graph. It installs
  `matrixpostd.service` but never enables or starts it. After installing a
  released package, opt in explicitly with
  `sudo systemctl enable --now matrixpostd.service`. The service owns its
  state under `/var/lib/matrixpost` through `DynamicUser`; its network policy
  permits only IPv4/IPv6 loopback, which retains the daemon API and local
  runner endpoints while preventing external network access. Use the documented
  example configuration only as a starting point and keep it credential-free.
  The checksum-pinned recipe has been uploaded to the AUR Git repository;
  confirm the AUR package page has indexed the upload before treating it as
  installable through an AUR helper.
- **Cargo:** `matrixpost-core`, `matrixpost-cli`, `matrixpostd`, and
  `matrixpost-mcp` are published public crates on
  [crates.io](https://crates.io/). Use the registry entries or the
  [GitHub releases](https://github.com/LIghtJUNction/MatriXpost/releases) to
  choose the current version and read its release notes. The Tauri desktop
  shell is intentionally not published to crates.io: it is a platform bundle,
  not a reusable library or `cargo install` target. CI compiles and tests the
  workspace, validates locked public-package metadata, and package-checks the
  independently publishable core crate. Releases publish the core crate first,
  then package-verify and publish its dependent public crates serially after
  the registry has indexed that core version.

No `.cursor` directory is used or required.

## License

This project is licensed under GPL-2.0-only; see [LICENSE](LICENSE).
