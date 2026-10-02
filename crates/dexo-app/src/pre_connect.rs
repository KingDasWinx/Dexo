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

/// The running command. Dropping it stops the command and everything in its process
/// group; Dexo leaving on a signal stops it too (see [`crate::process::stop_on_signals`]).
// ponytail: a Dexo killed outright (SIGKILL) cannot stop it; PR_SET_PDEATHSIG follows the
// thread that spawned the command, not the process, so it would end tunnels at random.
pub struct PreConnectProcess {
    child: Mutex<Child>,
    command: String,
    /// The last thing it said on stderr.
    said: Arc<Mutex<String>>,
    /// Set once the stderr reader has seen the end.
    heard: Arc<std::sync::atomic::AtomicBool>,
}

impl PreConnectProcess {
    /// Why the command is no longer running, when it is not: it ended, or was stopped,
    /// and the tunnel with it.
    pub fn stopped(&self) -> Option<String> {
        let status = self
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.try_wait().ok().flatten())?;
        Some(format!(
            "pre-connect command `{}` stopped ({status}){}",
            self.command,
            self.last_words()
        ))
    }

    /// `: <the last line it printed on stderr>`, waiting a moment for the reader to have
    /// it all.
    fn last_words(&self) -> String {
        let deadline = Instant::now() + Duration::from_millis(500);
        while !self.heard.load(std::sync::atomic::Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let said = self
            .said
            .lock()
            .map(|said| said.clone())
            .unwrap_or_default();
        said.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| format!(": {}", line.trim()))
            .unwrap_or_default()
    }
}

impl Drop for PreConnectProcess {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            crate::process::stop_group(&mut child);
        }
    }
}

impl std::fmt::Debug for PreConnectProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreConnectProcess").finish_non_exhaustive()
    }
}

/// Starts the profile's pre-connect command and waits, up to `timeout`, for the port it
/// opens. Returns the profile to connect with -- `127.0.0.1` and the free port that
/// `${port}` became, the TLS name kept as the real host's, `pre_connect` taken out so a
/// second connection rides the same tunnel -- and the process to keep alive with the
/// session. A profile without a command comes back as it is. Dropping the future while
/// it waits stops the command.
pub async fn prepare(
    profile: &ConnectionProfile,
    timeout: Duration,
) -> Result<(ConnectionProfile, Option<PreConnectProcess>), AppError> {
    if command(profile).is_none() {
        return Ok((profile.clone(), None));
    }
    let profile = profile.clone();
    let cancel = CancelOnDrop(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let cancelled = Arc::clone(&cancel.0);
    tokio::task::spawn_blocking(move || start(&profile, timeout, &cancelled))
        .await
        .map_err(|error| AppError::new(ErrorCategory::Internal, error.to_string()))?
        .map(|(profile, process)| (profile, Some(process)))
}

struct CancelOnDrop(Arc<std::sync::atomic::AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn start(
    profile: &ConnectionProfile,
    timeout: Duration,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(ConnectionProfile, PreConnectProcess), AppError> {
    let template = command(profile).unwrap_or_default().to_string();
    let fail = |reason: String| {
        AppError::new(
            ErrorCategory::Network,
            format!("pre-connect command `{template}` {reason}"),
        )
    };
    // Where the connection would have gone, read the way the connection reads it.
    let (host, port, routed) =
        crate::connection_profile::dial_target(&profile.config, &profile.driver)?;
    if routed {
        return Err(fail(
            "cannot be combined with an SSH tunnel or a proxy; use one or the other".into(),
        ));
    }
    let mut effective = profile.clone();
    let (command, target) = if template.contains("${port}") {
        let local = free_port().map_err(|error| fail(format!("found no free port: {error}")))?;
        if let Some(config) = effective.config.as_object_mut() {
            config.remove("endpoint");
            config.insert("host".into(), serde_json::json!("127.0.0.1"));
            config.insert("port".into(), serde_json::json!(local));
            // Verified TLS still checks the certificate against the real host.
            if let Some(tls) = config.get_mut("tls").and_then(|tls| tls.as_object_mut())
                && tls
                    .get("server_name")
                    .is_none_or(serde_json::Value::is_null)
            {
                tls.insert("server_name".into(), serde_json::json!(host));
            }
        }
        (
            template.replace("${port}", &local.to_string()),
            SocketAddr::from(([127, 0, 0, 1], local)),
        )
    } else {
        let target = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next())
            .ok_or_else(|| fail(format!("waits for {host}:{port}, which does not resolve")))?;
        (template.clone(), target)
    };
    if let Some(config) = effective.config.as_object_mut() {
        config.remove("pre_connect");
    }
    let mut shell = crate::process::shell(&command);
    crate::process::detach(&mut shell);
    let mut child = shell
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| fail(format!("could not start: {error}")))?;
    crate::process::register(child.id());
    let said = Arc::new(Mutex::new(String::new()));
    let heard = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Read on the side: a command that writes more than the pipe holds would otherwise
    // stall. The last 4 KB are kept, for when it gives up.
    if let Some(mut stderr) = child.stderr.take() {
        let said = Arc::clone(&said);
        let heard = Arc::clone(&heard);
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
            heard.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    } else {
        heard.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let process = PreConnectProcess {
        child: Mutex::new(child),
        command: template.clone(),
        said,
        heard,
    };
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fail("was stopped: the connect was given up".into()));
        }
        if let Some(stopped) = process.stopped() {
            return Err(AppError::new(
                ErrorCategory::Network,
                format!("{stopped}, before {target} opened"),
            ));
        }
        if TcpStream::connect_timeout(&target, Duration::from_millis(250)).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(fail(format!(
                "did not open {target} within {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // A command that put itself in the background -- `ssh -f`, `&` -- has left nothing
    // for Dexo to stop when the session ends; it is refused rather than left behind.
    std::thread::sleep(Duration::from_millis(300));
    if process.stopped().is_some() {
        return Err(fail(
            "went to the background; it has to keep running in the foreground (drop -f or &), so Dexo can stop it with the session".into(),
        ));
    }
    Ok((effective, process))
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
        // A tunnel that died says so, rather than leaving the session to time out on it.
        if let Some(stopped) = self.process.stopped() {
            return Err(dexo_driver_api::DriverError::new(
                dexo_driver_api::DriverErrorCategory::Transport,
                stopped,
            ));
        }
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

    /// Whether the command is gone, given a loaded machine a few seconds to reap it.
    fn gone(marker: &str) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while running(marker) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
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
        assert!(gone(marker), "the command outlived its process");
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
        assert!(error.contains("stopped (exit status: 3)"), "{error}");
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

    fn listener(marker: &str) -> String {
        format!(
            "python3 -c 'import socket,sys,time; s=socket.socket(); \
             s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); \
             s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(); time.sleep(60)' ${{port}} {marker}"
        )
    }

    /// A command that goes to the background is refused, and what it left in its group
    /// is stopped with it.
    #[tokio::test]
    async fn a_command_that_goes_to_the_background_is_refused() {
        let marker = "dexo-pre-connect-bg-31c9";
        let error = prepare(
            &profile(&format!("{} & sleep 0.2", listener(marker))),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("background") || error.contains("stopped"),
            "{error}"
        );
        assert!(gone(marker), "what the command left running outlived it");
    }

    /// A profile routed through SSH is refused; a port written as text is read the way
    /// the connection reads it; verified TLS keeps checking the real host's name.
    #[tokio::test]
    async fn routes_ports_and_tls_read_like_the_connection() {
        let mut routed = profile("true ${port}");
        routed.config["ssh"] = serde_json::json!({"host": "bastion", "port": 22, "username": "u"});
        let error = prepare(&routed, Duration::from_secs(1))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("SSH tunnel or a proxy"), "{error}");

        let listening = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut textual = profile("sleep 30 # dexo-pre-connect-text-port");
        textual.config["host"] = serde_json::json!("127.0.0.1");
        textual.config["port"] =
            serde_json::json!(listening.local_addr().unwrap().port().to_string());
        let (_, process) = prepare(&textual, Duration::from_secs(5)).await.unwrap();
        drop(process);

        let mut verified = profile(&listener("dexo-pre-connect-tls-77a0"));
        verified.config["tls"] = serde_json::json!({"mode": "verify_full"});
        let (effective, process) = prepare(&verified, Duration::from_secs(10)).await.unwrap();
        assert_eq!(effective.config["tls"]["server_name"], "db.internal");
        drop(process);
    }

    /// A tunnel that died is what the session's next query says.
    #[tokio::test]
    async fn a_session_whose_tunnel_died_says_so() {
        let marker = "dexo-pre-connect-died-a41e";
        let (_, process) = prepare(&profile(&listener(marker)), Duration::from_secs(10))
            .await
            .unwrap();
        let session = super::attach(Box::new(dexo_test_support::FakeSession::default()), process);
        assert!(
            session
                .execute(dexo_driver_api::QueryRequest::read("select 1", 1))
                .await
                .is_ok()
        );
        let _ = std::process::Command::new("pkill")
            .args(["-f", marker])
            .status();
        std::thread::sleep(Duration::from_millis(300));
        let error = session
            .execute(dexo_driver_api::QueryRequest::read("select 1", 1))
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("stopped"), "{error}");
    }

    /// A connect given up while the command starts stops the command.
    #[tokio::test]
    async fn a_connect_given_up_stops_its_command() {
        let marker = "dexo-pre-connect-cancel-5e2b";
        let slow = profile(&format!("sleep 30 {marker} # ${{port}}"));
        let pending = prepare(&slow, Duration::from_secs(20));
        let _ = tokio::time::timeout(Duration::from_millis(400), pending).await;
        assert!(gone(marker), "the command outlived the connect");
    }
}
