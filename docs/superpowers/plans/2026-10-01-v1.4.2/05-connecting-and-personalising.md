# 1.4.2 Section 5: Connecting and Personalising Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connecting to what is already running takes no typing, a database behind `kubectl port-forward` or a cloud proxy connects like any other, and Dexo looks and types the way its user wants.

**Architecture:** Docker discovery is a `dexo-app` module that runs the `docker` CLI (`ps`, then `inspect`) and turns each Postgres, MySQL or MariaDB container into a prefilled `NewConnection`; the connections screen lists them under their own heading. The pre-connect command is a `pre_connect` string in a profile's config, started by one `dexo_app::connect` helper that every dial goes through (TUI, CLI, MCP): it picks a free port for `${port}`, waits for the port, connects through it, and hands back the session wrapped so dropping it stops the process, plus the profile rewritten to the tunnel so side connections (counts, exports) use the same one. Themes are TOML files read by the existing `parse_theme`; five presets ship as embedded files, user files live in `<data dir>/themes/`, and a `keymap.toml` overlay is merged over the chosen profile. Settings gets a Theme row that applies as it is stepped through.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 5 (D5, D8, D6). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Config files get the setting only, never an explanatory comment block.

## Rulings made from the code

- **Docker is read, never driven.** `docker ps --format '{{json .}}'` and `docker inspect` on the ids it lists; an image whose name says `postgres`, `mysql` or `mariadb` (or a `postgres`/`mysql`/`mariadb` env var prefix) is a database. The host port is the first published binding of the image's default port. User, database and password come from `POSTGRES_*`, `MYSQL_*`/`MARIADB_*` (root when only a root password is set). No `docker` on PATH, or a daemon that does not answer within 3 s, lists nothing and says nothing. The password reaches the form's password field only; it is written to the keychain by Save, like any typed one, and never to SQLite.
- **The connections screen lists them as "Running in Docker"**, below the saved connections, each with its driver and `host:port`. Enter on one opens the New Connection form prefilled (name from the container), so nothing is saved without the user's Save. `r` refreshes the list.
- **Pre-connect is one config key, `pre_connect`**, a form field under Advanced like `password_command`. `${port}` is replaced by a free local port, and the connection then dials `127.0.0.1:${port}`; without `${port}` it waits for the profile's own host and port. The wait is 30 s; a command that exits first fails the connect with its exit status and the last line it printed on stderr. The process (and what it started) is stopped when the session closes, or when the connect fails.
- **One dial path.** `dexo_app::connect::open(factory, profile, secrets, connect_timeout)` runs the pre-connect, builds the `ConnectRequest`, connects, and returns the session (wrapped when a process is attached) with the effective profile: host and port of the tunnel, `pre_connect` removed. The TUI stores that profile for the session, so a count or an export rides the same tunnel instead of starting another.
- **Themes.** `theme` in `settings.toml` names one: `dexo` (the built-in, composed from mode and accent as today), a preset (`dracula`, `gruvbox`, `nord`, `catppuccin`, `tokyo-night`), or a user file `<data dir>/themes/<name>.toml` in the existing theme format. Settings shows a Theme row first; stepping it applies the theme at once (the preview is the app itself) and saves it. Mode and Accent belong to the Dexo theme: changing either switches back to it. A file that does not parse falls back to the built-in, with a message naming the file and line.
- **Keymap overlay.** `<data dir>/keymap.toml`, in the profiles' format, is merged over the chosen profile at start and on every profile switch: a chord bound in the overlay replaces the profile's binding in that context, `""` unbinds it, and an unknown command id or a bad chord is an error naming the file and line, the profile then used as it is.

## Tasks

### Task 1 (D5): Docker discovery
- `dexo_app::docker` (discover, parse `ps`/`inspect` JSON); connections screen section, refresh key, Enter prefills the form.
- Commit: `feat(connections): databases running in Docker are listed, ready to connect`.

### Task 2 (D8): Pre-connect command
- `dexo_app::connect` and `pre_connect` (free port, wait, process tree stop, session wrapper); TUI dial, CLI and MCP through it; form field.
- Commit: `feat(connections): a command can open the way before Dexo connects`.

### Task 3 (D6): User themes, presets and a keymap overlay
- Presets as embedded theme files; user themes listed from the data dir; the Theme row; `keymap.toml` overlay with errors naming file and line.
- Commit: `feat(settings): themes, five presets and a keymap overlay of your own`.

## Review Focus

1. No password from a container's environment reaches SQLite, TOML, logs or argv; nothing is saved without Save.
2. The pre-connect process never outlives its session, on close, on a failed connect, on quit, or on a crash of the connect; `${port}` is free when used.
3. A command line with secrets in it is never logged or shown in an error beyond what the user typed into the form.
4. A broken theme or keymap file never takes the app down or leaves it without a theme or keys.
