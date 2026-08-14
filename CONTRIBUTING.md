# Contributing

Dexo is a local-first terminal database workbench. Keep changes small and inside the crate that owns the behavior.

## Setup

- Rust 1.93 (MSRV in workspace `Cargo.toml`)
- Optional: Docker for driver contract tests

## Checks

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -j 4
```

On Linux, `cargo deny check` is required. Do not add git dependencies.

## Rules

- Secrets never go in SQLite, TOML, argv, logs, or panic reports.
- TUI/CLI/MCP go through `dexo-app`. Drivers do not import UI crates.
- Do not run `npm run dev`. Do not add git worktrees unless a maintainer asks.
