# Dexo 1.4.2 implementation plans

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`

The spec ships as one release in nine sections. Each section gets its own plan here, executed in order on `development`, one commit per task. A section's plan is written from the code as it stands once the previous section has landed, because each section changes what the next one builds on (the SQLite driver adds a dialect the guard has to know, B1's search engine is what B5's `/` uses, B4's diagnostics are what E5 serves).

| # | Plan | Spec items | Status |
|---|---|---|---|
| 1 | [01-safety.md](01-safety.md) | A1, A4, A5 | done |
| 2 | [02-try-it-in-seconds.md](02-try-it-in-seconds.md) | D2, D4, A2, D1, D7 | done |
| 3 | [03-editor.md](03-editor.md) | B1, B2, B3, B6, B5, B4 | done |
| 4 | [04-results.md](04-results.md) | C1, C2, A3, C3, C4 | done |
| 5 | [05-connecting-and-personalising.md](05-connecting-and-personalising.md) | D5, D8, D6 | done |
| 6 | [06-agents.md](06-agents.md) | E3, E1, E2, E4, E5 | done |
| 7 | [07-duckdb.md](07-duckdb.md) | D3 | done |
| 8 | [08-fixes.md](08-fixes.md) | F1, F2 | in progress |
| 9 | 09-launch-assets.md | README, GIF, `--help` | last |

## Release-wide constraints

Every plan copies these into its own Global Constraints.

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo deny check` when a dependency changes.
- Config files (`Cargo.toml` and friends) get the setting only, never an explanatory comment block.
- Tests: the standing preference is no tests while implementing plan tasks, validated by rust-analyzer and `cargo build`. A plan writes a test only where a safety or security property has no other way to be checked, and writes it out in full.
- Commit subjects are user-facing sentences; the CHANGELOG is generated from them.

## What later plans must honour from earlier ones

- **From 1 (safety):** a new driver enforces read-only on the server side (SQLite opens with `SQLITE_OPEN_READ_ONLY`, DuckDB with `AccessMode::ReadOnly`) or refuses to connect a read-only profile. A new `dexo_sql::Dialect` variant gets an arm in `is_read` and `destructive`, and `screens::editor::editor_dialect` maps its driver id.
- **From 1:** a temporary connection (A2) has no saved profile; `run_policy` in `update.rs` then fails closed (destructive statements confirmed), which is the intended behaviour, not a bug to work around.
- **From 2:** F1 validates EXPLAIN on SQLite, and on DuckDB once 7 has landed.
- **From 3:** B5's `/ n N` uses B1's engine; E5 serves B4's diagnostics.
