# Driver development

Implement capability traits in `dexo-driver-api` (catalog, query, transactions, DDL, explain, admin, transfer). Register the factory only in the `dexo` binary. Drivers must not import TUI or MCP code.

Add contract tests under `crates/dexo-driver-<name>/tests` using `dexo-test-support` containers. Mark versions outside the CI matrix `unverified` via handshake, and detect features from capabilities, not version numbers alone.

A driver whose dependencies are heavy ships as a plugin instead of in Dexo: DuckDB's crate builds a `dexo-driver-duckdb` executable whose `main` hands its `ConnectionFactory` to `dexo_plugin_sdk::serve`. Dexo runs it through `PluginFactory`, which implements the driver traits over the plugin protocol ([Plugins](plugins.md)). `DriverDescriptor::for_id` still knows the driver, so its connections are read, and the dialect and the statement guard stay in Dexo. Its tests run twice: in process (`tests/driver.rs`) and through the executable (`tests/through_plugin.rs`). SQLite's run through the SDK and the proxy in process the same way, to keep the protocol whole.

The workspace's clippy and test runs build every crate, DuckDB's engine included, on each CI platform: each job spends minutes compiling its C++ before any Rust, measured at about 4 minutes of a 12-core machine's debug build. `cargo test -p <crate>` skips it locally for work that does not touch DuckDB.
