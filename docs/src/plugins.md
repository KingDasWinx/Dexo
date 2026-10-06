# Plugins

Plugins add to an installed Dexo without rebuilding it. There are two kinds:

- **Drivers**: a database engine, run as a program of its own. DuckDB is one.
- **Themes**: packs of colour themes.

## Using plugins

Open **Plugins** with `Ctrl+G p`, or from the command palette (*Go to Plugins*, *Install Plugin…*, *Check Plugin Updates*).

- **Installed** lists what you have. **Available** lists what Dexo's plugin index offers that you do not have. Tab switches between them; `t` filters by type; `/` searches.
- A row's mark says its state: `●` enabled, `○` disabled, `↑` an update is available, `+` available to install. **local** marks a plugin installed from a file.
- The buttons act on the pick: `i` installs, `u` updates, `e` enables or disables, `a` applies a theme pack's first theme, `x` removes after asking. `r` fetches the index again; `f` installs from a file.
- While a download runs, a bar shows its progress, and Esc cancels it.
- A count beside **Plugins** on the top line says how many updates are waiting.

The same from the command line:

```sh
dexo plugin list [--available] [--json]
dexo plugin info duckdb [--json]
dexo plugin install duckdb [--version 1.0.0]
dexo plugin update duckdb        # or --all
dexo plugin disable duckdb       # keeps it installed, its drivers and themes left out
dexo plugin enable duckdb
dexo plugin remove duckdb --confirm
dexo plugin doctor [--json]
```

`dexo doctor` includes a one-line summary of the plugins.

A saved connection whose driver is a plugin you do not have stays saved. Opening it offers to install the plugin, and once it is in, the connection opens. The connection form lists such a driver as "DuckDB · not installed", and Test or Save offers to install it there.

## Where plugins come from

Dexo's plugin index is a file published with every plugin release, signed with the maintainer's key. Dexo checks that signature against the keys built into it before it reads the index. Every download is then checked against the size and SHA-256 the index lists for it. A plugin from the index is marked **official, signed**.

Nothing is downloaded on its own. The index is fetched when you open the Plugins screen, when you run a `dexo plugin` command, and by the daily update check, which follows the **Check for updates** setting and `DEXO_NO_UPDATE_CHECK`. A fetched index is kept for a day. Offline, Dexo shows the last one it had and says from when.

If the signature does not match, Dexo says "the index signature does not match: nothing was installed", and installs nothing. There is no way past it: a mirror must serve an index signed by a key Dexo trusts.

## Installing from a file, a URL or a folder

```sh
dexo plugin install ./duckdb-1.0.0-x86_64-unknown-linux-gnu.tar.gz --trust
dexo plugin install https://example.com/my-themes-1.0.0.tar.gz --trust
dexo plugin install ./plugins/my-themes --trust     # a folder, for developing a plugin
```

Nothing vouches for a plugin that does not come from the index, so Dexo shows its id, author and kind before it goes on. A driver plugin is a program that **will receive the passwords of the connections that use it**. The command line installs one only with `--trust`; the Plugins screen (`f`) asks you to type the plugin's id. Such plugins are marked **local, not verified** everywhere.

## Where plugins live

Under Dexo's data folder (`DEXO_DATA_HOME` moves it), in `plugins/`:

- `state.json`: what is installed, its version, origin and checksums;
- `index-cache.json` and `index-cache.json.minisig`: the last verified index;
- one folder per plugin.

An install unpacks into `plugins/.staging/` and moves into place at the end: a failure at any step leaves what was installed before. Two Dexos at once take turns through `plugins/.locks/`.

## A driver plugin's process

The process starts with the first connection that needs it and serves every connection of that plugin. It stops a minute after the last one closes, and when Dexo exits. It gets no `DEXO_*` variable, and only `PATH`, the home and temporary folders and the locale from your environment (and `SystemRoot` on Windows). Connection passwords reach it inside the connection request, never in its arguments or environment.

If it crashes, the open connections say "the DuckDB plugin stopped" with the last thing it logged, and the next connection starts it again, at most three times a minute.

Dexo keeps the SQL dialect, the statement guard and the production confirmations. They run before anything reaches a plugin, so a plugin driver cannot skip them.

## Environment variables

| Variable | What it does |
|---|---|
| `DEXO_PLUGIN_INDEX` | Fetches the index from this URL instead of Dexo's: a mirror, or a test. The signature is still required. |
| `DEXO_PLUGIN_TRUSTED_KEY` | Trusts one more minisign public key (its base64 line), for a mirror's or a test's index. |
| `DEXO_DRIVER_<ID>` | Runs this executable as plugin `<ID>` (uppercased, `-` as `_`) instead of the installed one: `DEXO_DRIVER_DUCKDB=target/debug/dexo-driver-duckdb`. For developing a driver. |

## The package format

A plugin is a `.tar.gz` holding `dexo-plugin.toml`, optionally `LICENSE` and `README.md`, and either the driver's executable directly under `bin/` or the theme files directly under `themes/`. Anything else is refused, as are links, absolute paths and `..`. A driver ships one archive per platform, `<id>-<version>-<target>.tar.gz`; a theme pack ships one, `<id>-<version>.tar.gz`.

```toml
schema = 1                      # the manifest format's version
id = "duckdb"                   # 2 to 32 of a-z, 0-9 and -, starting with a letter or digit
kind = "driver"                 # "driver" or "theme"
version = "1.0.0"
name = "DuckDB"
description = "Open DuckDB files, CSV, Parquet and JSON"
authors = ["KingDasWinx"]
license = "MIT OR Apache-2.0"
homepage = "https://github.com/KingDasWinx/Dexo"
min_dexo = "1.5.0"              # the oldest Dexo it runs on

[driver]
protocol = 1                    # the plugin protocol version it speaks
drivers = ["duckdb"]            # the connection driver ids it provides
display_name = "DuckDB"
dialect = "duckdb"              # recorded; Dexo's dialect handling keys on the driver id
file = true                     # a file database: the form asks for a path, not a host
url_schemes = ["duckdb"]
executable = "bin/dexo-driver-duckdb"   # .exe is added on Windows

# or, for a theme pack:
# [theme]
# themes = ["themes/*.toml"]
```

An unknown key is an error, and so is a driver id Dexo has as its own (`postgres`, `mysql`, `mariadb`, `sqlite`), `duckdb` from any plugin but `duckdb`, or one another installed plugin brings: a plugin never takes over another driver's connections. A theme file takes the format of [a theme of your own](workbench.md); a plugin's theme is saved in the settings as `plugin:<id>/<name>`.

## Writing a driver plugin

A driver plugin is any program that speaks the plugin protocol on stdin and stdout. In Rust, the SDK turns a `ConnectionFactory` into one:

```rust
fn main() {
    dexo_plugin_sdk::serve(
        vec![std::sync::Arc::new(my_driver::MyFactory)],
        env!("CARGO_PKG_VERSION"),
    );
}
```

`serve` returns when stdin closes. While you work on it, run it with `DEXO_DRIVER_<ID>` pointing at your build.

### The protocol

JSON-RPC 2.0, one message per line. Dexo sends requests, and the plugin answers each by its `id`, in any order. stdout carries only the protocol; stderr is the plugin's log, whose last lines Dexo shows when it fails. A line over 64 MB is an error.

1. **Handshake.** `initialize` with `{"dexo_version": "1.5.0", "protocols": [1]}`, answered within 10 seconds with `{"protocol": 1, "driver_version": "1.0.0", "drivers": ["duckdb"]}`: a protocol it was offered, and every driver its manifest declares.
2. **Sessions.** `session/open` carries the connection request, password and other secrets included, and answers `{"session", "capabilities", "families", "server_session_id"?, "transaction"?}`. `families` lists what the session answers beyond queries: `catalog`, `data`, `bulk`, `explain`, `transactions`, `cancel`. `session/close` ends it.
3. **Calls on a session** take `{"session": "s1", "args": ...}`:

| Family | Methods |
|---|---|
| query | `query/execute`, `query/fetch`, `query/close`, `query/cancel` |
| catalog | `catalog/list_children`, `catalog/object`, `catalog/ddl`, `catalog/dependencies`, `catalog/dependents`, `catalog/foreign_keys`, `catalog/relations_named`, `catalog/databases` |
| data | `data/fetch`, `data/fetch_value`, `data/apply`, `data/table_columns`, `data/estimate_rows` |
| bulk | `bulk/insert_batch` |
| explain | `explain/explain` |
| transactions | `tx/begin`, `tx/commit`, `tx/rollback`, `tx/savepoint`, `tx/rollback_to`, `tx/release_savepoint`: each answers `{"state", "notice"?, "error"?}` |

A result comes in pages Dexo asks for. `query/execute` answers as soon as the driver has the query, with a cursor and no events, so a cancel sent next reaches it. `query/fetch {"cursor"}` answers each page: up to 500 rows, 500 other events or about 4 MB. The last page has `"done": true`, the session's transaction state, and an error if the query failed. `query/close` drops a cursor that is not finished.

`query/cancel` is answered while a query runs; a plugin that does not answer it within 5 seconds is stopped. When Dexo stops waiting for any request, it sends `$/cancel_request {"id": <its id>}`, which needs no answer: the plugin drops that work, as a driver drops a future nobody polls.

Values travel tagged, `{"t": "i64", "v": 1}`; bytes are base64. A driver's error travels in the JSON-RPC error's `data`, with its category, code, position and hints. The types and method names are in the `dexo-plugin-protocol` crate.

## For maintainers

### The signing key

Done once, by the maintainer, on their own machine:

1. `minisign -G -p dexo-plugins.pub -s dexo-plugins.key`, with a strong password. Make a second pair the same way, `dexo-plugins-next`, and keep it offline: it is the next key.
2. In the repository's GitHub settings, store the contents of `dexo-plugins.key` as the secret `PLUGINS_MINISIGN_KEY`, and its password as `PLUGINS_MINISIGN_PASSWORD`.
3. Put both public keys (the second line of each `.pub`) in `crates/dexo-app/src/plugin/keys.rs`.

The private keys never enter the repository, a chat or an issue. To rotate, sign with the next key, and ship a Dexo that lists a new next key.

### Releasing a plugin

From a clean, pushed `main`: `./publish-plugin.sh duckdb 1.0.1`. It sets the version in `plugins/duckdb/dexo-plugin.toml` and the driver's crate, commits, and pushes the tag `plugins/duckdb-v1.0.1`. `plugins-release.yml` then builds the driver on each platform, publishes a release with the archives, adds them to the index with `scripts/plugin-index.py`, signs it, and replaces it on the `plugins-index` release.
