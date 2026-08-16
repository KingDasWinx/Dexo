# Changelog

## 1.1.0

Functional completion of the approved workbench: live schema/diff/transfer/explain, administration, settings, recovery, and policy-enforced multi-connection MCP.

- Driver-specific DDL planning, protected apply, and live/saved/file schema diff
- Streaming import/export, secure native backup/restore, EXPLAIN save/compare
- Live admin views, durable settings, crash recovery, and local diagnostics
- MCP connection router, expiring grants, and production fixture prohibition
- Release artifacts include checksums and an SBOM derived from the lockfile

## 1.0.0

First production release of Dexo: PostgreSQL and MySQL workbench, CLI, and local MCP server.

- Local-first SQLite state (schema v7) with keychain secrets
- TUI workbench, schema diff, transfer, explain, and admin
- MCP stdio adapter with grants and sanitized audit
- Release gates: fmt, clippy, tests, deny, fuzz smoke, performance budgets
