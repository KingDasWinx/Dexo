//! A driver plugin for Dexo's tests: SQLite served by the SDK as driver `testbin`,
//! misbehaving as the name it runs under says, so the host's handling of each failure
//! is tested against a real process:
//! - `crash` exits when a statement mentions `crash`, saying so on stderr;
//! - `ignore-cancel` never answers a cancel;
//! - `bad-protocol` answers the handshake with protocol 99;
//! - `print-env` writes the names of its environment next to itself, then serves.

use std::io::BufReader;
use std::sync::Arc;

use dexo_driver_api::{
    CapabilityState, ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, QueryId,
    QueryRequest, QueryStream, Session,
};
use dexo_driver_sqlite::SqliteFactory;
use dexo_plugin_protocol::{Request, Response, read_line, write_line};

fn main() {
    let exe = std::env::current_exe().unwrap_or_default();
    let mode = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match mode.as_str() {
        "bad-protocol" => bad_protocol(),
        "print-env" => {
            let names: Vec<String> = std::env::vars_os()
                .map(|(name, _)| name.to_string_lossy().into_owned())
                .collect();
            let _ = std::fs::write(exe.with_extension("env"), names.join("\n"));
            serve(mode);
        }
        _ => serve(mode),
    }
}

fn serve(mode: String) {
    dexo_plugin_sdk::serve(vec![Arc::new(Testing { mode })], "0.1.0");
}

/// Answers the handshake with a protocol no Dexo speaks, then waits to be stopped.
fn bad_protocol() {
    let mut stdin = BufReader::new(std::io::stdin());
    let mut line = Vec::new();
    if let Ok(true) = read_line(&mut stdin, &mut line)
        && let Ok(request) = serde_json::from_slice::<Request>(&line)
    {
        let reply =
            serde_json::json!({"protocol": 99, "driver_version": "0.1.0", "drivers": ["testbin"]});
        let _ = write_line(&mut std::io::stdout(), &Response::ok(request.id, reply));
    }
    while let Ok(true) = read_line(&mut stdin, &mut line) {}
}

struct Testing {
    mode: String,
}

#[async_trait::async_trait]
impl ConnectionFactory for Testing {
    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor::plugin_driver("testbin", "Testbin", true, "testbin", true)
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let inner = SqliteFactory.connect(request).await?;
        Ok(Box::new(Misbehaving {
            inner,
            mode: self.mode.clone(),
        }))
    }
}

struct Misbehaving {
    inner: Box<dyn Session>,
    mode: String,
}

#[async_trait::async_trait]
impl Session for Misbehaving {
    fn capabilities(&self) -> &[CapabilityState] {
        self.inner.capabilities()
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        if self.mode == "crash" && request.sql.contains("crash") {
            eprintln!("crashing on purpose");
            std::process::exit(3);
        }
        self.inner.execute(request).await
    }

    async fn cancel(&self, query: QueryId) -> Result<(), DriverError> {
        if self.mode == "ignore-cancel" {
            std::future::pending::<()>().await;
        }
        self.inner.cancel(query).await
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        self.inner.close().await
    }
}
