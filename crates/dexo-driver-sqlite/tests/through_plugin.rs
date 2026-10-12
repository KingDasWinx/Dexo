//! The SQLite driver's contract again, with the driver behind the plugin protocol: the
//! SDK serves it and Dexo's proxy talks to it, one plugin for every session, as in Dexo.
//! What passes here, a plugin driver does as the built-in one does.
mod suite;

use std::sync::{Arc, OnceLock};

use dexo_app::plugin::host::{PluginHost, Spawned};
use dexo_app::plugin::proxy::PluginFactory;
use dexo_driver_api::{ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, Session};
use dexo_driver_sqlite::SqliteFactory;

fn plugin() -> &'static PluginFactory {
    static PLUGIN: OnceLock<PluginFactory> = OnceLock::new();
    PLUGIN.get_or_init(|| {
        let host = PluginHost::new(
            "SQLite",
            vec!["sqlite".into()],
            Box::new(|| {
                let (reader, writer) =
                    dexo_plugin_sdk::serve_in_process(vec![Arc::new(SqliteFactory)])?;
                Ok(Spawned {
                    reader: Box::new(reader),
                    writer: Box::new(writer),
                    stderr: None,
                    child: None,
                })
            }),
        );
        PluginFactory::new(DriverDescriptor::sqlite(), Arc::new(host))
    })
}

async fn connect(request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
    plugin().connect(request).await
}
