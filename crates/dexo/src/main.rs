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
    run_dispatch(Args::parse(), registry, || Ok(dexo_tui::run()?))
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
}
