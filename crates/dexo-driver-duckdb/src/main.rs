//! The DuckDB plugin: the driver, served on stdin and stdout to the Dexo that started it.

fn main() {
    dexo_plugin_sdk::serve(
        vec![std::sync::Arc::new(dexo_driver_duckdb::DuckdbFactory)],
        env!("CARGO_PKG_VERSION"),
    );
}
