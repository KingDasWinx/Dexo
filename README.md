<div align="center">
  <h1>Dexo</h1>
  <p>A terminal database workbench with guardrails for AI agents.</p>

  <p>
    <a href="https://github.com/kingdaswinx/Dexo/releases/latest"><img src="https://img.shields.io/github/v/release/kingdaswinx/Dexo" alt="Release"></a>
    <img src="https://img.shields.io/badge/rust-1.93-orange" alt="MSRV 1.93">
    <a href="https://github.com/kingdaswinx/Dexo/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/kingdaswinx/Dexo/ci.yml?branch=main" alt="CI"></a>
    <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="MIT OR Apache-2.0">
  </p>
</div>

```sh
brew install kingdaswinx/tap/dexo   # or any install below
dexo --demo                         # a sample shop, nothing to connect to
```

Dexo is a keyboard-driven workbench for PostgreSQL, MySQL, MariaDB and SQLite, with DuckDB as a build option: a terminal UI, a command line and an MCP server for AI agents. A `DELETE` without `WHERE` on production waits for you to type the connection's name, a read-only connection is read-only on the server too, and an agent writes only through a grant you make. Everything stays on your machine.

<div align="center">
  <img src="assets/guardrails.gif" width="100%" alt="Dexo holding a DELETE without WHERE on the production connection shop-prod: a wrong name runs nothing, the full name runs it and four rows are deleted; then an AI agent's UPDATE waits in Agent Activity until it is approved">
</div>

<div align="center">
  <img src="assets/demo.gif" width="100%" alt="Dexo demo: connecting, browsing a table, writing SQL with autocomplete, opening a record, and switching to the light theme">
</div>

## Features

- **Workbench**: catalog tree, SQL editor with Vim mode and live diagnostics, results grid and a command palette.
- **Data**: insert and delete rows in the grid with a review before anything is written, filter and sort, import, export, backup and restore.
- **Schema**: object forms with a DDL preview, and schema diff between databases, snapshots and files.
- **Query plans**: EXPLAIN drawn as a tree; on Postgres with hypopg, try an index before building it.
- **Connections**: TLS, SSH tunnels, proxies, password managers, and databases found in Docker.
- **AI agents**: an MCP server with read-only profiles, allowlists, timed write grants and approval for each write.
- **Command line**: query, export, import, explain and diff from scripts, with the same guardrails.
- **Local-first**: no telemetry; passwords stay in the operating system's keychain or your password manager.

## Screenshots

<p align="center">
  <img src="assets/screenshots/table-data.webp" width="100%" alt="Browsing a table"><br>
  <sub>Browse a table: the grid pages on demand.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/record.webp" width="100%" alt="Record detail"><br>
  <sub><kbd>Enter</kbd> on a row shows every field.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/palette.webp" width="100%" alt="Command palette"><br>
  <sub><kbd>Ctrl</kbd>+<kbd>P</kbd> reaches every command.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/actions.webp" width="100%" alt="Node actions"><br>
  <sub><kbd>a</kbd> on a tree node lists what it can do.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/connection-form.webp" width="100%" alt="Connection form"><br>
  <sub>TLS, SSH and proxies under advanced options.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/help.webp" width="100%" alt="Keybindings"><br>
  <sub><kbd>F1</kbd> lists every key.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/workbench-light.webp" width="100%" alt="Light theme"><br>
  <sub>The light theme, with the violet accent.</sub>
</p>

<p align="center">
  <img src="assets/screenshots/settings-light.webp" width="100%" alt="Settings"><br>
  <sub>Theme, accent, keymap and more, applied at once.</sub>
</p>

## Installation

**Homebrew** (macOS, Linux)

```sh
brew install kingdaswinx/tap/dexo
```

**Scoop** (Windows)

```powershell
scoop bucket add dexo https://github.com/KingDasWinx/scoop-bucket
scoop install dexo
```

**Windows installer or portable** — from the [latest release](https://github.com/kingdaswinx/Dexo/releases/latest), `dexo-x86_64-pc-windows-msvc.msi` installs Dexo under Program Files and adds it to `PATH`; `dexo-x86_64-pc-windows-msvc.exe` runs as is, without installing. Both are unsigned, so Windows SmartScreen may ask for confirmation.

**Installer scripts**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/kingdaswinx/Dexo/releases/latest/download/dexo-installer.sh | sh
```

```powershell
irm https://github.com/kingdaswinx/Dexo/releases/latest/download/dexo-installer.ps1 | iex
```

**Debian, Ubuntu, Fedora** — download the `.deb` or `.rpm` from the [latest release](https://github.com/kingdaswinx/Dexo/releases/latest) (requires glibc 2.35 or later):

```sh
sudo apt install ./dexo_*_amd64.deb
sudo dnf install ./dexo-*.x86_64.rpm
```

**Arch Linux** — the community-maintained [`dexo-bin`](https://aur.archlinux.org/packages/dexo-bin) package in the AUR:

```sh
yay -S dexo-bin    # or paru -S dexo-bin
```

**From source** (Rust 1.93 or later)

```sh
cargo install --locked --git https://github.com/kingdaswinx/Dexo dexo
```

The release binaries leave DuckDB out: its engine is large. Build it in with the `duckdb` feature:

```sh
cargo install --locked --git https://github.com/kingdaswinx/Dexo dexo --features duckdb
```

Every release also ships archives for each platform, SHA-256 checksums, and a CycloneDX SBOM. See the [install guide](docs/src/install.md) for details.

## Getting started

```sh
dexo                                         # start the workbench
dexo postgres://user@localhost:5432/shop     # open a database from its URL, without saving it
dexo connections add --name local --driver postgres --host 127.0.0.1 --username postgres --database postgres
dexo query --connection local --sql "select version()"
```

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>Enter</kbd> or <kbd>Ctrl</kbd>+<kbd>J</kbd> | Run the statement under the cursor (<kbd>Ctrl</kbd>+<kbd>J</kbd> wherever the terminal sends <kbd>Ctrl</kbd>+<kbd>Enter</kbd> as <kbd>Enter</kbd>, as tmux does) |
| <kbd>F1</kbd> | Every key, for the keymap in use (Default, Vim or Emacs) |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

## AI agents

```sh
dexo mcp profile create --name assistant
dexo mcp profile set --name assistant --connection local --query-mode raw-read
dexo mcp allow --profile assistant --selector 'postgres.public.*'
dexo mcp profile enable --name assistant --confirm
dexo mcp setup --client claude-code --profile assistant   # or codex, cursor, claude-desktop
```

The agent reads only what the profile allows. To let it write, make a grant for a while; with `--ask`, each write waits for your approval on the Agents screen (<kbd>Ctrl</kbd>+<kbd>G</kbd> <kbd>a</kbd>). See the [MCP guide](docs/src/mcp.md).

## Compatibility

| Database | Tested versions |
| --- | --- |
| PostgreSQL | 14.18, 16.9, 17.5 |
| MySQL | 8.0.42, 8.4.5, 9.3.0 |
| MariaDB | 10.11, 11.4 |
| SQLite | 3.53.2, built in |
| DuckDB | 1.5.6, built in with the `duckdb` feature |

Linux, macOS and Windows.

## Privacy

No telemetry. Passwords live in the operating system's keychain or come from your password manager, never from Dexo's files. Once a day Dexo asks GitHub for the latest release, to tell you about updates; turn it off under Settings → Updates or with `DEXO_NO_UPDATE_CHECK=1`. Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Documentation

[User guide](docs/src/SUMMARY.md) · [Changelog](CHANGELOG.md) · [Contributing](CONTRIBUTING.md) · [Bug report form](https://forms.gle/gw1i6tGgVsJsgCxGA)

## License

MIT or Apache 2.0, at your option.
