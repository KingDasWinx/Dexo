mod demo;

use std::sync::Arc;

use clap::Parser;
use dexo_app::DriverRegistry;
use dexo_cli::args::TuiStart;
use dexo_cli::args::{Args, Command, McpCommand};
use dexo_cli::run::{TuiRunner, run_dispatch, temporary_connection};
use dexo_driver_mysql::{MariadbFactory, MysqlFactory};
use dexo_driver_postgres::PostgresFactory;
use dexo_driver_sqlite::SqliteFactory;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    quiet_on_closed_pipe(&args);
    init_tracing();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(PostgresFactory));
    registry.register(Arc::new(MysqlFactory));
    registry.register(Arc::new(MariadbFactory));
    registry.register(Arc::new(SqliteFactory));
    match dexo_storage::AppPaths::discover() {
        Ok(paths) => dexo_app::plugin::register_installed(&registry, &paths.data_dir),
        Err(_) => dexo_app::plugin::register_dev_overrides(&registry),
    }
    let tui_registry = registry.clone();
    run_dispatch(args, registry, Workbench(tui_registry))
}

/// `dexo … | head` panicked on the closed pipe: a command that prints and exits takes the
/// default disposition, which ends it quietly as other CLIs end. The workbench, `mcp serve`
/// and `mcp doctor --probe` keep Rust's, since they write to pipes of their own -- a
/// driver's, a probed server's -- where a closed one must be an error to report.
// ponytail: a plugin driver that dies mid-write ends a one-shot command without a word;
// handle EPIPE where output is written if that ever matters.
fn quiet_on_closed_pipe(args: &Args) {
    let writes_its_own_pipes = matches!(
        &args.command,
        None | Some(Command::Mcp {
            command: McpCommand::Serve { .. } | McpCommand::Doctor { probe: true, .. }
        })
    );
    #[cfg(unix)]
    if !writes_its_own_pipes {
        // SAFETY: SIG_DFL is a valid disposition for SIGPIPE, set before any thread runs.
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }
    #[cfg(not(unix))]
    let _ = writes_its_own_pipes;
}

/// Starts the TUI the way the command line asked: plain, or connected to a URL.
struct Workbench(DriverRegistry);

impl TuiRunner for Workbench {
    fn run(self, start: TuiStart) -> anyhow::Result<()> {
        let startup = match start {
            TuiStart::Workbench => dexo_tui::Startup::Workbench,
            TuiStart::Demo => dexo_tui::Startup::Temporary(Box::new(demo::create()?)),
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
