//! A command that opens the way before Dexo connects -- `kubectl port-forward`,
//! `cloud-sql-proxy`, a Teleport tunnel -- run from a profile's `pre_connect`, kept
//! running while the session lives and stopped with it.

use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dexo_driver_api::Session;

use crate::connection_profile::ConnectionProfile;
use crate::error::{AppError, ErrorCategory};

/// How long the command gets to open its port.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The profile's pre-connect command, if it has one.
pub fn command(profile: &ConnectionProfile) -> Option<&str> {
    profile
        .config
        .get("pre_connect")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
}

/// The running command. Dropping it stops the command and whatever it started.
// ponytail: a Dexo killed outright (SIGKILL) cannot stop it; PR_SET_PDEATHSIG follows the
// thread that spawned the command, not the process, so it would end tunnels at random.
pub struct PreConnectProcess {
    child: Child,
}

impl Drop for PreConnectProcess {
    fn drop(&mut self) {
        crate::process::stop_tree(&mut self.child);
    }
}

impl std::fmt::Debug for PreConnectProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreConnectProcess")
            .field("pid", &self.child.id())
            .finish()
    }
}

/// Starts the profile's pre-connect command and waits, up to `timeout`, for the port it
/// opens. Returns the profile to connect with -- `127.0.0.1` and the free port that
/// `${port}` became, `pre_connect` taken out so a second connection rides the same
/// tunnel -- and the process to keep alive with the session. A profile without a
/// command comes back as it is.
pub async fn prepare(
    profile: &ConnectionProfile,
    timeout: Duration,
) -> Result<(ConnectionProfile, Option<PreConnectProcess>), AppError> {
    if command(profile).is_none() {
        return Ok((profile.clone(), None));
    }
    let profile = profile.clone();
    tokio::task::spawn_blocking(move || start(&profile, timeout))
        .await
        .map_err(|error| AppError::new(ErrorCategory::Internal, error.to_string()))?
        .map(|(profile, process)| (profile, Some(process)))
}

fn start(
    profile: &ConnectionProfile,
    timeout: Duration,
) -> Result<(ConnectionProfile, PreConnectProcess), AppError> {
    let template = command(profile).unwrap_or_default().to_string();
    let fail = |reason: String| {
        AppError::new(
            ErrorCategory::Network,
            format!("pre-connect command `{template}` {reason}"),
        )
    };
    let mut effective = profile.clone();
    if let Some(config) = effective.config.as_object_mut() {
        config.remove("pre_connect");
    }
    let (command, target) = if template.contains("${port}") {
        let port = free_port().map_err(|error| fail(format!("found no free port: {error}")))?;
        if let Some(config) = effective.config.as_object_mut() {
            config.insert("host".into(), serde_json::json!("127.0.0.1"));
            config.insert("port".into(), serde_json::json!(port));
        }
        (
            template.replace("${port}", &port.to_string()),
            SocketAddr::from(([127, 0, 0, 1], port)),
        )
    } else {
        let host = profile
            .config
            .get("host")
            .and_then(serde_json::Value::as_str)
            .filter(|host| !host.is_empty())
            .unwrap_or("127.0.0.1");
        let port = profile
            .config
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .ok_or_else(|| {
                fail("has no ${port}, so it waits for the connection's port; set one".into())
            })?;
        let target = (host, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next())
            .ok_or_else(|| fail(format!("waits for {host}:{port}, which does not resolve")))?;
        (template.clone(), target)
    };
    let mut child = crate::process::shell(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| fail(format!("could not start: {error}")))?;
    // The last thing it said, for when it gives up before the port opens. Read on the
    // side: a command that writes more than the pipe holds would otherwise stall.
    let said = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let said = Arc::clone(&said);
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 1024];
            while let Ok(read) = stderr.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let mut said = said.lock().unwrap_or_else(|error| error.into_inner());
                said.push_str(&String::from_utf8_lossy(&buffer[..read]));
                let keep = said.len().saturating_sub(4096);
                if keep > 0 {
                    let cut = said.ceil_char_boundary(keep);
                    said.drain(..cut);
                }
            }
        });
    }
    let mut process = PreConnectProcess { child };
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect_timeout(&target, Duration::from_millis(250)).is_ok() {
            return Ok((effective, process));
        }
        if let Ok(Some(status)) = process.child.try_wait() {
            std::thread::sleep(Duration::from_millis(50));
            let said = said.lock().unwrap_or_else(|error| error.into_inner());
            let last = said.lines().rev().find(|line| !line.trim().is_empty());
            return Err(fail(match last {
                Some(line) => format!("exited ({status}) before {target} opened: {}", line.trim()),
                None => format!("exited ({status}) before {target} opened"),
            }));
        }
        if Instant::now() >= deadline {
            return Err(fail(format!(
                "did not open {target} within {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A port nothing listens on, now.
fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

/// `session`, kept together with the command that opened its way, so closing or
/// dropping the session stops the command too.
pub fn attach(session: Box<dyn Session>, process: Option<PreConnectProcess>) -> Box<dyn Session> {
    match process {
        Some(process) => Box::new(WithPreConnect { session, process }),
        None => session,
    }
}

struct WithPreConnect {
    session: Box<dyn Session>,
    process: PreConnectProcess,
}

#[async_trait::async_trait]
impl Session for WithPreConnect {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        self.session.capabilities()
    }

    async fn execute(
        &self,
        request: dexo_driver_api::QueryRequest,
    ) -> Result<dexo_driver_api::QueryStream, dexo_driver_api::DriverError> {
        self.session.execute(request).await
    }

    async fn cancel(
        &self,
        query: dexo_driver_api::QueryId,
    ) -> Result<(), dexo_driver_api::DriverError> {
        self.session.cancel(query).await
    }

    async fn close(self: Box<Self>) -> Result<(), dexo_driver_api::DriverError> {
        let WithPreConnect { session, process } = *self;
        let closed = session.close().await;
        drop(process);
        closed
    }

    fn transactions(&self) -> Option<&dyn dexo_driver_api::TransactionControl> {
        self.session.transactions()
    }

    fn catalog(&self) -> Option<&dyn dexo_driver_api::CatalogReader> {
        self.session.catalog()
    }

    fn data(&self) -> Option<&dyn dexo_driver_api::DataMutator> {
        self.session.data()
    }

    fn ddl(&self) -> Option<&dyn dexo_driver_api::DdlExecutor> {
        self.session.ddl()
    }

    fn security(&self) -> Option<&dyn dexo_driver_api::SecurityAdmin> {
        self.session.security()
    }

    fn bulk(&self) -> Option<&dyn dexo_driver_api::BulkWriter> {
        self.session.bulk()
    }

    fn explain(&self) -> Option<&dyn dexo_driver_api::ExplainProvider> {
        self.session.explain()
    }

    fn admin(&self) -> Option<&dyn dexo_driver_api::AdministrationProvider> {
        self.session.admin()
    }

    fn events(&self) -> Option<dexo_driver_api::SessionEventStream> {
        self.session.events()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use super::prepare;
    use crate::connection_profile::{ConnectionId, ConnectionProfile, SecretRef};

    fn profile(command: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            "tunnelled",
            "postgres",
            "local",
            serde_json::json!({
                "host": "db.internal", "port": 5432, "username": "u", "pre_connect": command
            }),
            SecretRef::new("ref".into()),
        )
    }

    fn running(marker: &str) -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", marker])
            .output()
            .is_ok_and(|output| !output.stdout.is_empty())
    }

    /// `${port}` becomes a free port the profile then dials on localhost; the command
    /// runs until the process is dropped, and its children with it.
    #[tokio::test]
    async fn the_command_opens_a_port_and_stops_with_its_process() {
        let marker = "dexo-pre-connect-test-7d1f";
        let command = format!(
            "python3 -c 'import socket,sys,time; s=socket.socket(); \
             s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); \
             s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(); time.sleep(60)' ${{port}} {marker}"
        );
        let (effective, process) = prepare(&profile(&command), Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(effective.config["host"], "127.0.0.1");
        let port = effective.config["port"].as_u64().unwrap();
        assert!(effective.config.get("pre_connect").is_none());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port as u16)).is_ok());
        assert!(running(marker));
        drop(process);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!running(marker), "the command outlived its process");
    }

    /// A command that gives up says why, in its own last words.
    #[tokio::test]
    async fn a_command_that_exits_first_says_what_it_said() {
        let error = prepare(
            &profile("echo 'no such service: db' >&2; exit 3 # ${port}"),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("exited"), "{error}");
        assert!(error.contains("no such service: db"), "{error}");
        let without = profile("true");
        let (same, process) = prepare(
            &ConnectionProfile {
                config: serde_json::json!({"host": "h", "port": 1}),
                ..without
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(process.is_none());
        assert_eq!(same.config["host"], "h");
    }
}
