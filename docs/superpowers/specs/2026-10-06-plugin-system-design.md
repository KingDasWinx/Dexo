# Plugin System — Design

Date: 2026-10-06
Target release: Dexo 1.5.0
Status: approved in conversation, section by section; this document is the written spec for review.

## 1. Goal

Plugins become a core part of Dexo: a user downloads extras into an installed Dexo
without recompiling it. The first version ships two kinds:

- **Drivers** — a database engine as a separate executable. DuckDB is the first; it
  leaves the Dexo binary for good.
- **Themes** — packs of colour themes.

The foundation (package format, signed index, download and verification, the Plugins
screen, the CLI, the release pipeline) is built so further kinds — keymaps, snippets,
templates, commands, exporters — can be added later without reworking it.

### Decisions taken

| Question | Decision |
|---|---|
| Plugin kinds in v1 | Drivers and themes. |
| Who publishes | Only official plugins in v1, through an index signed by the maintainer. The manifest and index formats are public from day one, and installing from a local file or URL works behind an explicit trust prompt, so a community index can come later without changes. |
| Where plugins are managed | A new **Plugins** screen, plus offers in context (the connection form, the theme picker). |
| Release cadence | Each plugin has its own version and release. Compatibility comes from the protocol version and a `min_dexo` field, not from matching the Dexo version. |
| DuckDB compiled in | Removed. DuckDB exists only as a plugin. |
| Architecture | Declarative packages; drivers as separate processes speaking JSON-RPC over stdio. WASM, a scripting language and dynamic libraries were considered and rejected (section 3). |

## 2. Background

### What Dexo has today

- **Themes** already load from `<data_dir>/themes/*.toml` (`crates/dexo-tui/src/theme.rs`,
  `user_themes`, `parse_theme`), with validation and line-numbered errors. The five
  presets (Dracula, Gruvbox, Nord, Catppuccin, Tokyo Night) are embedded TOML in the same
  format. A theme pack is almost free.
- **Drivers** sit behind `ConnectionFactory` and `Session` (`crates/dexo-driver-api`),
  registered in `DriverRegistry` (`crates/dexo-app/src/driver_registry.rs`) at one site
  (`crates/dexo/src/main.rs`). The TUI, the CLI and the MCP server only see the traits, so a
  proxy driver plugs in without changing them.
- **DuckDB** is a cargo feature (`duckdb = ["dep:dexo-driver-duckdb", "dexo-tui/duckdb"]`)
  left out of release binaries: it adds about 11 minutes per build and takes the binary
  from 56 MB to 120 MB (`docs/src/install.md`).
- **Driver knowledge outside the drivers**: the connection form's driver list is hard-coded
  with `cfg!(feature = "duckdb")` (`crates/dexo-tui/src/screens/connection.rs`),
  `DriverDescriptor::for_id` is a closed list, `DriverDescriptor` holds `&'static str`,
  and dialects, URL schemes, SQL templates and the statement guard key on driver ids.
- **Missing**: an HTTP download client, archive extraction, artifact signatures.
  **Present**: rustls TLS (`dexo-transport`), `sha2`, child-process supervision
  (`crates/dexo-app/src/process.rs`), `.sha256` files on every release asset.

### What other tools teach

- No terminal database tool surveyed downloads drivers at runtime; all compile them in
  (usql build tags, rainfrog and lazydb cargo features, lazysql, dblab, sq). A
  downloadable driver is new in this category.
- **DBX** and **nvim-dbee** do what this spec proposes: a per-platform executable, a
  manifest with URL and checksum, RPC over stdio with a handshake and capability list.
  DBX's DuckDB worker is a separate Rust process; DBX moved from streaming to cursor
  paging, duplicated its wire types between host and agent (and they drifted), and
  compares versions by string inequality.
- **DbGate** and **Beekeeper Studio** show the in-app manager (list, detail, install,
  update, remove). Beekeeper uses a PR-reviewed index on GitHub, a `minAppVersion`, and
  strict id validation against path traversal.
- **Harlequin** shows capability flags that default to off (how it evolved its adapter API
  without breaking adapters) and what goes wrong without a protocol version.
- **Yazi** shows themes ("flavors") as data-only packages with a file allowlist, a
  pinned-and-hashed lockfile, and refusing to overwrite a plugin the user edited.
- None of them signs artifacts.

## 3. Architecture

**Chosen: declarative packages, drivers as separate processes.**

- One package format for every kind, described by a manifest.
- A theme is data: its TOML files go into the plugin's folder and the existing theme
  loader reads them. No plugin code runs.
- A driver is a per-platform executable. Dexo starts it and talks JSON-RPC over its stdin
  and stdout. A `PluginFactory` inside Dexo implements the driver traits by forwarding to
  that process and registers in `DriverRegistry`.
- Shared crates define the protocol for both sides, and an SDK turns any existing
  `ConnectionFactory` into a plugin server.

Rejected:

- **WASM (wasmtime)** — DuckDB is C++ with threads and files; compiling it to WASM is not
  practical, and the runtime is large.
- **A scripting language (Lua)** — v1 has no kind that needs scripts. Revisit when there
  are user commands.
- **Dynamic libraries** — the Rust ABI is unstable and a crash takes Dexo down.

## 4. Package format and layout

### Manifest — `dexo-plugin.toml`

```toml
schema = 1                      # manifest format version
id = "duckdb"                   # ^[a-z0-9][a-z0-9-]{1,31}$, unique
kind = "driver"                 # "driver" | "theme"
version = "1.0.0"               # semver
name = "DuckDB"
description = "Open DuckDB files, CSV, Parquet and JSON"
authors = ["KingDasWinx"]
license = "MIT OR Apache-2.0"
homepage = "https://github.com/KingDasWinx/Dexo"
min_dexo = "1.5.0"

[driver]                        # kind = "driver" only
protocol = 1                    # protocol version the executable speaks
drivers = ["duckdb"]            # connection driver ids it provides
display_name = "DuckDB"
dialect = "duckdb"              # a dialect Dexo knows; unknown means generic SQL handling
file = true                     # a file database: the form asks for a path, not a host
url_schemes = ["duckdb"]        # `duckdb:///path` opens a temporary connection
executable = "bin/dexo-driver-duckdb"   # ".exe" appended on Windows

[theme]                         # kind = "theme" only
themes = ["themes/*.toml"]
```

The manifest describes a driver without running code, so the connection form can list a
driver that is in the index but not installed.

Validation is strict: unknown keys are an error, `id` must match the pattern, `version` and
`min_dexo` must parse as semver, the section matching `kind` must be present and the
other absent.

### Archive

- A driver ships one `.tar.gz` per platform:
  `duckdb-1.0.0-x86_64-unknown-linux-gnu.tar.gz`. Artifact keys are Rust target triples.
- A theme pack ships one platform-independent `.tar.gz`: `extra-themes-1.0.0.tar.gz`.
- Allowed entries: `dexo-plugin.toml`, `LICENSE`, `README.md`, and per kind either the
  executable under `bin/` (driver) or `themes/*.toml` (theme). Anything else is refused,
  as are symlinks, hard links, absolute paths and `..` components.

### On disk

Under `AppPaths.data_dir` (which honours `DEXO_DATA_HOME`):

```
plugins/
  state.json            # installed plugins: version, enabled, origin, sha256, installed_at
  index-cache.json      # last verified index, with its fetch time
  index-cache.json.minisig
  .locks/<id>.lock
  duckdb/               # the extracted package
  extra-themes/
```

- `state.json` is plain JSON written through a temporary file and a rename, like
  `update-check.json`. It needs no schema migration. The files on disk are the source of
  truth; `state.json` is a cache rebuilt from the manifests if it is missing.
- `origin` is `official` (installed from the signed index) or `local` (from a file, URL or
  folder).
- Install and update extract into `plugins/.staging/<id>-<random>/` and rename into place
  atomically; a failure leaves the previous version untouched.
- A plugin can be **disabled** without being removed: its drivers and themes are not
  registered. Its drivers are known as disabled, so a connection on one offers to enable
  it, not to install it.
- `dexo plugin install ./folder` installs straight from a folder, as `local`, for
  development.

## 5. Index, download, verification and trust

### The index

Published as two assets of a GitHub release with a fixed tag, `plugins-index`:
`index.json` and `index.json.minisig`. Each publish replaces those two assets, so the URL
never changes:
`https://github.com/KingDasWinx/Dexo/releases/download/plugins-index/index.json`.

```json
{
  "schema": 1,
  "generated": "2026-10-20T12:00:00Z",
  "plugins": [
    {
      "id": "duckdb",
      "kind": "driver",
      "name": "DuckDB",
      "description": "Open DuckDB files, CSV, Parquet and JSON",
      "authors": ["KingDasWinx"],
      "license": "MIT OR Apache-2.0",
      "homepage": "https://github.com/KingDasWinx/Dexo",
      "versions": [
        {
          "version": "1.0.0",
          "min_dexo": "1.5.0",
          "driver": { "protocol": 1, "drivers": ["duckdb"], "display_name": "DuckDB",
                      "dialect": "duckdb", "file": true, "url_schemes": ["duckdb"] },
          "artifacts": {
            "x86_64-unknown-linux-gnu": {
              "url": "https://github.com/KingDasWinx/Dexo/releases/download/plugins%2Fduckdb-v1.0.0/duckdb-1.0.0-x86_64-unknown-linux-gnu.tar.gz",
              "sha256": "…",
              "size": 27839411
            }
          }
        }
      ]
    }
  ]
}
```

### Signature

- The index is signed with **minisign** (Ed25519). Dexo embeds a list of trusted public
  keys — the current one and the next — so a key can be rotated without breaking older
  installs.
- Because each artifact's sha256 comes from the signed index, the artifact is
  authenticated, not only checked for integrity.
- Verification uses the `minisign-verify` crate (pure Rust; must pass `cargo deny`).
- The private key lives in a GitHub Actions secret. The maintainer generates the key pair
  locally; the private key never enters the repository or a chat.
- `DEXO_PLUGIN_TRUSTED_KEY` adds a trusted public key, for tests and mirrors. It is
  documented.

### Download

- HTTP: `ureq` with rustls, run on a blocking thread so the screen never waits on the
  network. It follows the GitHub-to-CDN redirect. (Both `ureq` and the archive crates
  must pass `cargo deny`.)
- Archives: `flate2` and `tar`.
- `DEXO_PLUGIN_INDEX` overrides the index URL (tests, mirrors). The signature is still
  required.

### Install flow (TUI and CLI alike)

1. Fetch the index and verify its signature. A verified index is cached for 24 hours in
   `plugins/index-cache.json`; offline, the cache is used and its date is shown.
2. Pick the newest compatible version: `min_dexo` ≤ the running Dexo, and a protocol
   version this Dexo supports. Versions compare as semver.
3. Pick the artifact for the current target triple (derived from
   `std::env::consts::{OS, ARCH}`).
4. Download into `plugins/.staging/` with progress, a size cap (256 MB), and a check of
   size and sha256 against the index.
5. Extract with the allowlist, and check the manifest against the index entry (id,
   version, kind).
6. For a driver, start the executable and complete the handshake: this proves it runs on
   this platform and speaks the expected protocol.
7. Rename into place and write `state.json`.

A failure at any step removes the staging folder and leaves the previous state.

### Outside the index (file or URL)

- There is no signature, so Dexo asks for explicit trust. It shows the id, author and
  kind, and for a driver: "this program will receive the passwords of the connections
  that use it".
- The TUI asks to type the plugin id to confirm, as production confirmations do; the CLI
  requires `--trust`.
- These plugins are marked **local** everywhere.

### Updates and network use

- Nothing is downloaded on its own. The index is fetched only when the Plugins screen
  opens, by `dexo plugin …` commands, and by the daily update check, which honours the
  existing `update_check` setting and `DEXO_NO_UPDATE_CHECK`.
- An update is available when the index has a compatible version newer (semver) than the
  installed one. Updating is always the user's action.

### Concurrency and removal

- The TUI and the CLI may run at once: every operation takes `plugins/.locks/<id>.lock`
  (created exclusively, holding the pid). A lock older than 10 minutes is stale and
  replaced.
- Removing deletes the plugin's folder and its state entry. Connections that used its
  driver stay saved and show "driver not installed" with an offer to install it.

## 6. The driver protocol

### Transport and lifecycle

- JSON-RPC 2.0, one JSON message per line, over the executable's stdin and stdout. stdout
  carries only the protocol. stderr is the plugin's log; Dexo keeps its last 20 lines for
  error messages. A line longer than 64 MB is a protocol error.
- **One process per plugin**, serving all of its sessions. This keeps DuckDB's behaviour
  of two sessions on one file sharing the database.
- The process starts on the first connection, exits 60 seconds after its last session
  closes, and is stopped when Dexo exits. The SDK's `serve` also exits when its stdin
  reaches end of file, so a plugin never outlives a Dexo that died without stopping it. It is started with `process::detach` (its own
  process group), registered for `stop_all`, and stopped with `stop_group`.
- Its environment is minimal: `PATH`, `HOME` (`USERPROFILE` on Windows), the temporary
  directory variables and the locale variables. No `DEXO_*` variable is passed.
- `DEXO_DRIVER_<ID>=/path/to/executable` (for example `DEXO_DRIVER_DUCKDB`) runs a local
  build instead of the installed plugin. It is for development and documented as such.

### Handshake

```json
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"dexo_version":"1.5.0","protocols":[1]}}
← {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"driver_version":"1.0.0","drivers":["duckdb"]}}
```

The reply must arrive within 10 seconds, name a protocol from the offered list, and list
the drivers the manifest declares; otherwise the plugin is reported broken.

### Methods

Each family maps onto an existing trait in `dexo-driver-api`.

| Family | Methods |
|---|---|
| session | `session/open` → `{session, capabilities, server_session_id?}`; `session/close` |
| query | `query/execute`, `query/fetch`, `query/close`, `query/cancel` |
| catalog | `catalog/list_children`, `catalog/object`, `catalog/ddl`, `catalog/dependencies`, `catalog/dependents`, `catalog/foreign_keys`, `catalog/relations_named`, `catalog/databases` |
| data | `data/fetch`, `data/fetch_value`, `data/apply`, `data/table_columns`, `data/estimate_rows` |
| bulk | `bulk/insert_batch` |
| explain | `explain/explain` |
| transactions | `tx/begin`, `tx/commit`, `tx/rollback`, `tx/savepoint`, `tx/rollback_to`, `tx/release_savepoint`; each reply carries the new transaction state and any notice, which the host keeps for the sync `state()` and `take_notice()` |

`session/open` carries what `ConnectRequest` carries — endpoint, database, user, the
password and other connection secrets, read-only, transport — inside the request on
stdin. Secrets never go on argv or the environment, and Dexo never logs message bodies.

### Results: pages pulled by the host

```json
→ {"id":7,"method":"query/execute","params":{"session":"s1","query":"q1","sql":"select …","params":[],"row_limit":1000,"timeout_ms":120000,"read_only":false,"mutating":false}}
← {"id":7,"result":{"cursor":"c1","events":[],"done":false}}
→ {"id":8,"method":"query/fetch","params":{"cursor":"c1"}}
← {"id":8,"result":{"cursor":"c1","events":[{"columns":[…]},{"rows":[[…],[…]]}],"done":false}}
→ {"id":9,"method":"query/fetch","params":{"cursor":"c1"}}
← {"id":9,"result":{"events":[{"rows":[…]},{"result_set_finished":{"rows_affected":null,"truncated":false}},{"finished":{}}],"done":true}}
```

- `query/execute` answers as soon as the driver has the query, with a cursor and no
  events, so a cancel sent next reaches it. The proxy implements `QueryStream` by calling
  `query/fetch` as the stream is polled. A page holds up to 500 rows, 500 other events or
  about 4 MB; a row too large for one message fails its result only.
- Flow control is built in: the driver only produces what the host asked for, and a large
  result never holds up replies to other sessions.
- `query/close` releases a cursor that was not read to the end.

### Cancellation

- `query/cancel` is answered while an `execute` or `fetch` is in flight (the protocol is
  multiplexed by request id).
- When a stream is dropped unfinished, the proxy sends `query/cancel` and `query/close`
  itself.
- If the driver does not answer a cancel within 5 seconds, the host kills the process; the
  sessions of that plugin fail with a clear message.

### Capabilities

- Each session declares the families it supports (`catalog`, `data`, `bulk`, `explain`,
  `transactions`, `cancel`) and the `CapabilityState` list (capability, available, reason)
  that the UI already uses.
- The proxy's accessors (`catalog()`, `data()`, …) return `Some` only for declared
  families.
- DDL, security and administration are not in protocol 1: their traits have synchronous
  methods (`DdlExecutor::plan_change`, `AdministrationProvider::preview`) that cannot wait
  on a remote call, and DuckDB implements none of them. They come later as new families,
  which needs no protocol bump because families are negotiated.

### Wire types and the SDK

- The `dexo-driver-api` types that lack serde derives gain them: `DbValue`, `QueryRequest`,
  `QueryEvent`, `ColumnMeta`, `RowBatch`, `Capability`, `CapabilityState`, the mutation
  and explain request types. `DriverError` gets a serializable form (its fields are
  private). Values are tagged JSON, `{"t":"i64","v":1}`; bytes are base64.
- `dexo-plugin-protocol` (new crate) defines the method names, request and reply
  messages, the handshake and the framing, used by both sides. One definition, no drift.
- `dexo-plugin-sdk` (new crate) offers `serve(factory)`, which turns **any**
  `ConnectionFactory` into a stdio plugin server. The DuckDB plugin's `main` is a few
  lines around the existing `DuckdbFactory`.
- The host's `PluginFactory` and its proxy `Session` live in `dexo-app` (where
  `process.rs` is) and implement the traits by calling the process.

### Errors and crashes

- Driver errors travel in the JSON-RPC `error.data` with the `DriverError` fields
  (category, native code, position, row, detail, hint, retryable). Dexo shows them as it
  shows a built-in driver's errors.
- Process failures become plain messages: "the DuckDB plugin stopped: <last log line>"
  for an exit, invalid JSON, or a timeout. They map to `DriverErrorCategory::Network`
  (process gone) or `Internal` (protocol violation).
- After a crash, the next connection restarts the process, at most 3 times a minute.

## 7. Integration in Dexo

### Descriptors and the driver registry

- `DriverDescriptor` holds owned strings (`Cow<'static, str>`) so a descriptor can come
  from a manifest. `for_id` becomes the set of known drivers: the built-ins (Postgres,
  MySQL, MariaDB, SQLite), installed and enabled driver plugins, and index entries not
  installed (for offers only).
- `DriverRegistry` keys on `String` and becomes shared and updatable at runtime (an
  `Arc<RwLock<…>>` inside, so clones see changes): installing a driver from the Plugins
  screen registers it without restarting Dexo.
- Each plugin driver registers a `PluginFactory`; its process starts on the first
  connection.
- `FEATURE_GATED` goes. A known but uninstalled driver errors with: "DuckDB is a plugin:
  install it with `dexo plugin install duckdb` or from Plugins (Ctrl+G p)".

### Offers in context

- The connection form lists the registry's drivers instead of a hard-coded list. An
  uninstalled DuckDB shows as **"DuckDB · not installed"**; picking it offers to install
  it there, and the form continues where it was once the install finishes.
- Opening a saved connection whose driver is missing offers [Install] [Cancel] instead of
  only an error.
- The theme picker in Settings lists plugin themes, named with their pack.

### What stays in Dexo

The plugin brings only the engine. Dexo keeps the SQL dialect (`Dialect::Duckdb`), the
statement guard (including the DuckDB functions it refuses), the object templates and the
file URL schemes. The guard therefore runs **before** anything reaches a plugin: a plugin
driver cannot bypass production confirmations or a read-only connection. A driver whose
manifest `dialect` is unknown to Dexo gets generic SQL handling — an accepted limit for
future third-party drivers.

### DuckDB becomes a plugin only

- The `duckdb` feature leaves `crates/dexo/Cargo.toml` and `crates/dexo-tui/Cargo.toml`,
  with every `cfg!(feature = "duckdb")`.
- `crates/dexo-driver-duckdb` stays the driver library and gains a binary,
  `dexo-driver-duckdb`, whose `main` calls `dexo_plugin_sdk::serve(DuckdbFactory)`. The
  crate is marked `dist = false` so the Dexo release does not build it.
- DuckDB tests run through the plugin executable.

### Themes

- The theme loader reads `<data_dir>/themes/*.toml` and also
  `plugins/<id>/themes/*.toml` for each enabled theme plugin.
- A plugin theme's key is `plugin:<id>/<name>`.
- If the active theme's plugin is removed or disabled, Dexo falls back to the default with
  a warning, as it already does when a theme file disappears.

### CLI and MCP

- `dexo <url>`, `dexo query` and the other commands use the same registry, so
  `duckdb:///file.duckdb` works through the plugin.
- MCP keeps accepting only the Postgres and MySQL families. Opening DuckDB to MCP is a
  separate decision, outside this spec.

## 8. The Plugins screen and the CLI

### Plugins screen

`Ctrl+G p`, listed on the top line: Workbench, Connections, Agents, Server, History,
Plugins. It follows the screens' pattern: list and detail, `/` search, filters as chips,
buttons, folding to one column under 80 columns.

- **Views**: `1 Installed` and `2 Available` (the index's plugins not installed).
- **Filter**: `t Type: all | drivers | themes`.
- **List rows**: a mark (`●` enabled, `○` disabled, `↑` update available, `+` available),
  name, kind, version, and **local** for plugins from outside the index.
- **Detail fields**: kind; installed and newest version; author; license; requires Dexo ≥;
  protocol; download size for this platform; origin (**official, signed** or **local, not
  verified**); the drivers or themes the package provides; the start of its README.
- **Item buttons**: `[i Install]`, `[u Update to 1.1.0]`, `[e Disable]` / `[e Enable]`,
  `[x Remove…]` (asks first), and for themes `[a Apply]`, which makes it the active theme.
- **List buttons**: `[r Refresh]` (fetch the index again), `[f Install from file…]`.
- **Progress**: a bar with bytes and percent while downloading; `Esc` cancels.
- **Messages**:
  - signature mismatch: "the index signature does not match: nothing was installed" — no
    override;
  - offline: "offline: showing the index from 2026-10-05".
- **Install from file** shows the trust prompt (section 5) in the detail pane; the user
  types the plugin id to confirm.
- A count beside "Plugins" on the top line shows how many updates are available, as
  "Agents 1" does for approvals.
- **Palette**: *Plugins*, *Install Plugin…*, *Check Plugin Updates*.

### CLI

```
dexo plugin list [--available] [--json]
dexo plugin info <id> [--json]
dexo plugin install <id | file | url | folder> [--version X] [--trust] [--non-interactive]
dexo plugin update [<id> | --all]
dexo plugin remove <id> --confirm
dexo plugin enable <id>
dexo plugin disable <id>
dexo plugin doctor [--json]
```

`doctor` checks each installed plugin: files against the sha256 in `state.json`, the
executable starting and answering the handshake, and compatibility with this Dexo.
`dexo doctor` includes its summary.

### Documentation

A new page, `docs/src/plugins.md` (use, install, trust, the manifest format, the protocol
for driver authors), linked from `SUMMARY.md`; a short Plugins section in the README;
`docs/src/install.md` and `docs/src/drivers.md` updated for DuckDB as a plugin.

## 9. Release of plugins

### Workflow

A new workflow, `.github/workflows/plugins-release.yml`, separate from the cargo-dist
`release.yml` (which is generated and not edited by hand). It runs on tags
`plugins/<id>-v<version>`.

- **Driver**: a matrix over Dexo's five targets (Linux x86_64 and aarch64 on
  ubuntu-22.04 runners, macOS x86_64 and aarch64, Windows x86_64).
  1. Build `dexo-driver-duckdb` in release mode natively on each runner (DuckDB needs a
     C++ toolchain; no cross-compiling), with a build cache.
  2. Check that the tag's version matches the manifest's and the crate's.
  3. Pack the `.tar.gz` (manifest, `LICENSE`, `README.md`, `bin/`), compute sha256 and
     size, and publish a GitHub release `plugins/duckdb-v1.0.0` with the artifacts.
- **Theme pack**: the same workflow without the matrix; one `.tar.gz`.
- **Index**: a final job, never run twice at once (a concurrency group):
  1. Download the current `index.json` from the `plugins-index` release.
  2. Add the new version, validated strictly by a script in the repository.
  3. Sign it with minisign, using the private key and its password from secrets.
  4. Upload `index.json` and `index.json.minisig` to `plugins-index`, replacing them.
- **PR checks**: the integration workflow builds the DuckDB plugin on Linux and runs its
  tests through the plugin path, to catch breakage before a release.

### Helper

`./publish-plugin.sh <id> <version>` mirrors `publish.sh`: it checks that `main` is clean
and in sync, sets the version in the manifest and the crate, commits, tags
`plugins/<id>-v<version>`, and pushes.

### Signing key

A one-time step by the maintainer, documented for maintainers:

1. Generate the key pair locally with `minisign -G`.
2. Store the private key and its password as GitHub Actions secrets.
3. Put the public key in Dexo's trusted key list (`crates/dexo-app/src/plugin/keys.rs`).

### First official plugins

Their sources live in the Dexo repository under `plugins/<id>/`.

- **`duckdb`** — the driver.
- **`extra-themes`** — a pack of new themes; the five presets stay built in. Initial set:
  Solarized Dark, Solarized Light, One Dark, Rosé Pine, Kanagawa and Everforest.

## 10. Testing

- **Unit**:
  - manifest parsing and validation (allowlist, id pattern, semver, kind sections);
  - index parsing and signature verification, valid and invalid, with a test key;
  - compatible-version selection (`min_dexo`, protocol, semver ordering, prereleases);
  - safe extraction: `..`, absolute paths, symlinks, hard links and unexpected files are
    refused;
  - `state.json` atomic writes and rebuild from manifests; locks, including stale ones.
- **Protocol**: round-trip of every wire type, including bytes, decimals, JSON values and
  errors.
- **Conformance**: the built-in **SQLite** driver runs through `serve` → proxy, and the
  existing SQLite driver tests pass through it. The DuckDB tests run through the plugin
  executable.
- **Process**: a crash mid-query gives a clear error; the restart budget holds; cancel
  works; a hung driver is killed after the cancel timeout; a handshake with an unsupported
  protocol is refused.
- **Install, end to end**: a local HTTP server serves an index and artifacts signed with a
  test key (`DEXO_PLUGIN_INDEX`, `DEXO_PLUGIN_TRUSTED_KEY`). Covered: install, update,
  offline cache, wrong sha256, wrong size, incompatible `min_dexo`, two installs at once.
- **TUI**: snapshots of the Plugins screen — installed, available, update, progress, trust
  prompt, signature mismatch, offline — at 80×24 and folded under 80 columns; the
  "not installed" driver in the connection form; plugin themes in the theme picker.
- **CLI**: every `dexo plugin` command against the local test index, text and `--json`.

## 11. Delivery

One release, **1.5.0**, built in this order; each step works and is testable on its own.

1. **Protocol, SDK and proxy**: serde on the API types, `dexo-plugin-protocol`,
   `dexo-plugin-sdk::serve`, `PluginFactory` and its proxy session, conformance through
   SQLite, `DEXO_DRIVER_<ID>`.
2. **DuckDB as a plugin**: the `dexo-driver-duckdb` executable, the `duckdb` feature
   removed, owned descriptors, the shared registry, "not installed" in the connection form.
3. **Packages and local install**: the manifest, `dexo plugin install <file|folder>
   --trust`, `state.json`, enable, disable and remove, plugin themes.
4. **Index and download**: signature verification, download and verification, install
   and update from the index, `dexo plugin doctor`.
5. **The Plugins screen**, the offers in context, the palette commands.
6. **Release**: `plugins-release.yml`, `publish-plugin.sh`, the signing key setup, the
   first official plugins (`duckdb`, `extra-themes`), `docs/src/plugins.md`.

## 12. Out of scope

- A community index.
- Plugin kinds beyond drivers and themes: keymaps, snippets, templates, commands,
  exporters, screens.
- DDL, security and administration in the protocol.
- DuckDB over MCP.
- Sandboxing plugin processes.
- Automatic updates.
- Windows on ARM.

## 13. Risks

- **DuckDB build time** on five platforms, macOS and Windows above all. Mitigated by the
  build cache and by rebuilding the plugin only when it changes.
- **The signing key**: if it is lost, a new key ships in a new Dexo version. Dexo trusts
  the current and the next key from the first version, so a planned rotation does not
  break anyone.
- **Windows**: `stop_on_signals` is a no-op there today. Plugin processes are stopped by
  `stop_group` (which uses `taskkill /T /F`) when Dexo exits normally; an abnormal exit can
  leave one running until it notices its stdin closed — the SDK's `serve` exits when stdin
  reaches end of file.
- **API surface**: adding serde to `dexo-driver-api` makes its types part of a public
  protocol. Protocol 1 is frozen once released; changes go through new families or
  protocol 2.
