//! DuckDB's contract through the plugin executable Dexo runs: one process for every
//! session, as in Dexo, so two sessions on one file share its database.
mod suite;

use std::sync::{Arc, OnceLock};

use dexo_app::plugin::host::{PluginHost, process_spawner};
use dexo_app::plugin::proxy::PluginFactory;
use dexo_driver_api::{ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, Session};

fn plugin() -> &'static PluginFactory {
    static PLUGIN: OnceLock<PluginFactory> = OnceLock::new();
    PLUGIN.get_or_init(|| {
        let host = PluginHost::new(
            "DuckDB",
            vec!["duckdb".into()],
            process_spawner(env!("CARGO_BIN_EXE_dexo-driver-duckdb").into()),
        );
        PluginFactory::new(DriverDescriptor::duckdb(), Arc::new(host))
    })
}

async fn connect(request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
    plugin().connect(request).await
}
