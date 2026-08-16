use std::sync::Arc;

use clap::Parser;
use dexo_app::DriverRegistry;
use dexo_cli::args::Args;
use dexo_cli::run::run_dispatch;
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;

fn main() -> anyhow::Result<()> {
    init_tracing();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(PostgresFactory));
    registry.register(Arc::new(MysqlFactory));
    let tui_registry = registry.clone();
    run_dispatch(Args::parse(), registry, move || {
        Ok(dexo_tui::run(tui_registry)?)
    })
}

fn init_tracing() {
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
    };
    let log_dir = dexo_storage::AppPaths::discover()
        .map(|paths| paths.data_dir.join("logs"))
        .unwrap_or_else(|_| std::env::temp_dir().join("dexo-logs"));
    if let Ok(file) =
        dexo_app::diagnostic_service::SizeRotatingWriter::open(&log_dir, "dexo", 1_048_576, 5)
    {
        let (writer, guard) = tracing_appender::non_blocking(file);
        let _ = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_env_filter(filter())
            .try_init();
        // ponytail: keep the non-blocking worker for process lifetime.
        std::mem::forget(guard);
    } else {
        let _ = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter())
            .try_init();
    }
}
