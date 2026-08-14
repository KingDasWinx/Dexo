# Driver development

Implement capability traits in `dexo-driver-api` (catalog, query, transactions, DDL, explain, admin, transfer). Register the factory only in the `dexo` binary. Drivers must not import TUI or MCP code.

Add contract tests under `crates/dexo-driver-<name>/tests` using `dexo-test-support` containers. Mark versions outside the CI matrix `unverified` via handshake, and detect features from capabilities, not version numbers alone.
