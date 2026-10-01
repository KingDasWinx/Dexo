use std::sync::Arc;

use clap::Parser;
use dexo_app::DriverRegistry;
use dexo_cli::args::Args;
use dexo_cli::args::TuiStart;
use dexo_cli::run::{TuiRunner, run_dispatch, temporary_connection};
use dexo_driver_mysql::{MariadbFactory, MysqlFactory};
use dexo_driver_postgres::PostgresFactory;

fn main() -> anyhow::Result<()> {
    init_tracing();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(PostgresFactory));
    registry.register(Arc::new(MysqlFactory));
    registry.register(Arc::new(MariadbFactory));
    let tui_registry = registry.clone();
    run_dispatch(Args::parse(), registry, Workbench(tui_registry))
}

/// Starts the TUI the way the command line asked: plain, or connected to a URL.
struct Workbench(DriverRegistry);

impl TuiRunner for Workbench {
    fn run(self, start: TuiStart) -> anyhow::Result<()> {
        let startup = match start {
            TuiStart::Workbench => dexo_tui::Startup::Workbench,
            TuiStart::Url {
                url,
                password_prompt,
            } => {
                dexo_tui::Startup::Temporary(Box::new(temporary_connection(&url, password_prompt)?))
            }
        };
        Ok(dexo_tui::run(self.0, startup)?)
    }
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
            .with_ansi(false)
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
