# Driver development

Implement capability traits in `dexo-driver-api` (catalog, query, transactions, DDL, explain, admin, transfer). Register the factory only in the `dexo` binary. Drivers must not import TUI or MCP code.

Add contract tests under `crates/dexo-driver-<name>/tests` using `dexo-test-support` containers. Mark versions outside the CI matrix `unverified` via handshake, and detect features from capabilities, not version numbers alone.

A driver whose dependencies are heavy goes behind a cargo feature of `dexo` that registers it -- DuckDB's, `duckdb`, compiles DuckDB's engine -- and enables `dexo-tui`'s feature of the same name, which lists it in the connection form. `DriverDescriptor::for_id` knows every driver, so a build without one still reads its connections and tells the user how to get it.

The workspace's clippy and test runs build every crate with every feature, DuckDB's engine included, on each CI platform: each job spends minutes compiling its C++ before any Rust, measured at about 4 minutes of a 12-core machine's debug build. `cargo test -p <crate>` skips it locally for work that does not touch DuckDB.
