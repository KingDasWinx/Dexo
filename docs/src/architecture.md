# Architecture

Workspace crates: `dexo` (binary), `dexo-app` (use cases), `dexo-tui` / `dexo-cli` / `dexo-mcp` (adapters), `dexo-driver-*`, `dexo-sql`, `dexo-storage`, `dexo-secrets`, `dexo-transport`, `dexo-runtime`.

TUI, CLI, and MCP depend on `dexo-app`. Drivers implement contracts and are wired only in `dexo`. Local SQLite is schema version 7; migrations backup the file before a destructive upgrade.

ADR: compiled-in official drivers, no plugin ABI in 1.0.
