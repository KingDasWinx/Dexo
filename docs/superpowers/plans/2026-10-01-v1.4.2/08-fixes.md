# 1.4.2 Section 8: Fixes Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per fix. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** EXPLAIN works end to end on every driver, in the TUI and the CLI, and the bugs reported during earlier work are gone.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 8 (F1, F2). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

## Tasks

### F1: EXPLAIN end to end
- Run the estimated and the analyzed plan of a join, an aggregate, a write and a statement with parameters, from the CLI (`dexo explain`, `--analyze --confirm`, every `--format`) and from the TUI (F7, Shift+F7, the tree and the text views, Try index), on Postgres 16, MySQL 8.4, MariaDB 11.4, SQLite and DuckDB. Each failure found is its own fix and commit.

### F2: Pending bugs
- Inspect: `role "…" does not exist` (the object's name passed as a role) -- fixed with the section 6 review; verified here.
- Postgres `citext` and other extension types whose binary form is their text show as text, not hex.
- The welcome logo renders in its colours.
- The schema editor form has the Submit/Cancel footer every dialog has.
- The keybindings modal scrolls with PageUp/PageDown (and Home/End).
- Single-line inputs move and delete by words that keep accented letters.
- Also found in use: the editor's horizontal scroll is not reset after a line shrinks; a new document's suggested name can repeat an open one's; Ctrl+A does not select the text of a single-line input; text typed while the new-document name prompt is open lands in the name.

## Review Focus

1. Every EXPLAIN path, on every driver, gives a plan or a clear refusal; none leaves a change behind.
2. Decoding a type by its text never shows a binary type as garbage.
3. The input fixes hold for every single-line input (connection form, prompts, palette, bars).
