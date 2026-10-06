//! A plugin's process: started on its first connection and shared by all its sessions,
//! stopped a minute after the last one closes, and restarted after a crash -- not more
//! than three times a minute.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dexo_driver_api::{DriverError, DriverErrorCategory};
use dexo_plugin_protocol::{InitializeParams, InitializeResult, PROTOCOL, method};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::lock;
use super::rpc::{CallError, Closed, RpcClient};

pub const HANDSHAKE: Duration = Duration::from_secs(10);
pub const IDLE: Duration = Duration::from_secs(60);
pub const CANCEL: Duration = Duration::from_secs(5);
const RESTARTS: usize = 3;
const LOG_LINES: usize = 20;

/// A started plugin: its pipes, its log, and its process when it is one.
pub struct Spawned {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub stderr: Option<Box<dyn Read + Send>>,
    pub child: Option<Child>,
}

/// How a plugin is started: a process in Dexo, a thread in tests.
pub type Spawner = Box<dyn Fn() -> std::io::Result<Spawned> + Send + Sync>;

/// One run of a plugin.
pub struct Live {
    pub rpc: Arc<RpcClient>,
    log: Arc<Mutex<VecDeque<String>>>,
    /// Hangs up when stderr ends: what a crash said is read before it is reported.
    log_done: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    child: Mutex<Option<Child>>,
    /// Stopped on purpose -- idle, removed, Dexo leaving -- rather than crashed.
    stopped: AtomicBool,
}

impl Live {
    fn start(spawned: Spawned) -> Self {
        let log: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let (done, log_done) = std::sync::mpsc::channel::<()>();
        if let Some(stderr) = spawned.stderr {
            let log = log.clone();
            std::thread::spawn(move || {
                let _done = done;
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { return };
                    let mut log = lock(&log);
                    if log.len() == LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(line);
                }
            });
        }
        Self {
            rpc: RpcClient::start(spawned.reader, spawned.writer),
            log,
            log_done: Mutex::new(Some(log_done)),
            child: Mutex::new(spawned.child),
            stopped: AtomicBool::new(false),
        }
    }

    /// The plugin's last line of log, once its stderr had a moment to end.
    fn last_log(&self) -> Option<String> {
        if let Some(done) = lock(&self.log_done).take() {
            let _ = done.recv_timeout(Duration::from_millis(500));
        }
        lock(&self.log).back().cloned()
    }

    /// Stops the process, when there is one, and waits for it.
    fn reap(&self) {
        if let Some(mut child) = lock(&self.child).take() {
            crate::process::stop_group(&mut child);
        }
    }
}

pub struct PluginHost {
    name: String,
    drivers: Vec<String>,
    spawn: Spawner,
    idle: Duration,
    starting: tokio::sync::Mutex<()>,
    state: Mutex<HostState>,
}

#[derive(Default)]
struct HostState {
    live: Option<Arc<Live>>,
    sessions: usize,
    /// Bumped by every open and close: an idle timer acts only if nothing happened since.
    ticket: u64,
    crashes: VecDeque<Instant>,
}

impl PluginHost {
    /// `name` is what messages call the plugin; `drivers` what its handshake must list.
    pub fn new(name: impl Into<String>, drivers: Vec<String>, spawn: Spawner) -> Self {
        Self {
            name: name.into(),
            drivers,
            spawn,
            idle: IDLE,
            starting: tokio::sync::Mutex::new(()),
            state: Mutex::default(),
        }
    }

    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The running process, started -- and its handshake checked -- when there is none.
    pub async fn live(&self) -> Result<Arc<Live>, DriverError> {
        let _one_at_a_time = self.starting.lock().await;
        let previous = lock(&self.state).live.clone();
        if let Some(previous) = previous {
            if previous.rpc.closed().is_none() {
                return Ok(previous);
            }
            if !previous.stopped.load(Ordering::SeqCst) {
                let now = Instant::now();
                let mut state = lock(&self.state);
                state
                    .crashes
                    .retain(|at| now.duration_since(*at) < Duration::from_secs(60));
                if state.crashes.len() >= RESTARTS {
                    return Err(DriverError::new(
                        DriverErrorCategory::Network,
                        format!(
                            "the {} plugin stopped {RESTARTS} times in a minute: try again in a minute",
                            self.name
                        ),
                    ));
                }
                state.crashes.push_back(now);
            }
            previous.reap();
            lock(&self.state).live = None;
        }
        let spawned = (self.spawn)().map_err(|error| {
            DriverError::new(
                DriverErrorCategory::Configuration,
                format!("the {} plugin did not start: {error}", self.name),
            )
        })?;
        let live = Arc::new(Live::start(spawned));
        if let Err(why) = self.handshake(&live).await {
            self.stop_live(&live, &why);
            return Err(DriverError::new(
                DriverErrorCategory::Internal,
                format!("the {} plugin is broken: {why}", self.name),
            ));
        }
        lock(&self.state).live = Some(live.clone());
        Ok(live)
    }

    async fn handshake(&self, live: &Live) -> Result<(), String> {
        let offered = InitializeParams {
            dexo_version: env!("CARGO_PKG_VERSION").into(),
            protocols: vec![PROTOCOL],
        };
        let reply: InitializeResult = match live
            .rpc
            .call(method::INITIALIZE, offered, Some(HANDSHAKE))
            .await
        {
            Ok(reply) => reply,
            Err(CallError::TimedOut) => {
                return Err("it did not answer the handshake within 10 seconds".into());
            }
            Err(CallError::Remote(error)) => return Err(error.message),
            Err(CallError::Closed(closed)) => {
                return Err(live.last_log().unwrap_or(closed.why));
            }
        };
        if reply.protocol != PROTOCOL {
            return Err(format!(
                "it speaks protocol {}, and this Dexo speaks {PROTOCOL}",
                reply.protocol
            ));
        }
        if let Some(missing) = self
            .drivers
            .iter()
            .find(|driver| !reply.drivers.contains(driver))
        {
            return Err(format!(
                "it does not provide the {missing} driver its manifest declares"
            ));
        }
        Ok(())
    }

    /// A call on `live`, its failures in the words a session's error uses.
    pub async fn ask<R: DeserializeOwned>(
        &self,
        live: &Live,
        method: &str,
        params: impl Serialize,
        timeout: Option<Duration>,
    ) -> Result<R, DriverError> {
        match live.rpc.call(method, params, timeout).await {
            Ok(reply) => Ok(reply),
            Err(CallError::Remote(error)) => Err(error.into_driver_error()),
            Err(CallError::Closed(closed)) => Err(self.failure(live, &closed)),
            Err(CallError::TimedOut) => Err(DriverError::new(
                DriverErrorCategory::Timeout,
                format!("the {} plugin did not answer in time", self.name),
            )),
        }
    }

    /// "the DuckDB plugin stopped: <why>" -- for a crash, the last thing it logged.
    fn failure(&self, live: &Live, closed: &Closed) -> DriverError {
        let why = if live.stopped.load(Ordering::SeqCst) || closed.violation {
            closed.why.clone()
        } else {
            live.last_log().unwrap_or_else(|| closed.why.clone())
        };
        let category = if closed.violation {
            DriverErrorCategory::Internal
        } else {
            DriverErrorCategory::Network
        };
        DriverError::new(category, format!("the {} plugin stopped: {why}", self.name))
    }

    fn stop_live(&self, live: &Live, why: &str) {
        live.stopped.store(true, Ordering::SeqCst);
        live.rpc.close(Closed {
            why: why.into(),
            violation: false,
        });
        live.reap();
    }

    /// Kills `live` now -- a driver that did not answer a cancel in time -- failing its
    /// sessions with `why`. The next connection starts it again.
    pub fn kill(&self, live: &Live, why: &str) {
        self.stop_live(live, why);
    }

    /// Stops the process for good -- removed, disabled, updated -- failing its sessions
    /// with `why`.
    pub fn stop(&self, why: &str) {
        let live = lock(&self.state).live.take();
        if let Some(live) = live {
            self.stop_live(&live, why);
        }
    }

    pub fn session_opened(&self) {
        let mut state = lock(&self.state);
        state.sessions += 1;
        state.ticket += 1;
    }

    /// With no session left, the process stops after the idle time unless one opens
    /// meanwhile.
    pub fn session_closed(self: &Arc<Self>) {
        let ticket = {
            let mut state = lock(&self.state);
            state.sessions = state.sessions.saturating_sub(1);
            state.ticket += 1;
            if state.sessions > 0 {
                return;
            }
            state.ticket
        };
        let host = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(host.idle);
            let idle = {
                let state = lock(&host.state);
                state.sessions == 0 && state.ticket == ticket
            };
            if idle {
                host.stop("it was idle");
            }
        });
    }
}

/// Starts `executable` as a plugin: a process group of its own, stopped with Dexo, and
/// an environment with nothing of Dexo's in it -- the connection's secrets go in the
/// `session/open` message, never in a variable.
pub fn process_spawner(executable: std::path::PathBuf) -> Spawner {
    Box::new(move || {
        let mut command = std::process::Command::new(&executable);
        crate::process::detach(&mut command);
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_clear()
            .envs(plugin_env(std::env::vars_os()));
        let mut child = command.spawn()?;
        crate::process::register(child.id());
        let missing = || std::io::Error::other("the plugin's pipes were not opened");
        Ok(Spawned {
            reader: Box::new(child.stdout.take().ok_or_else(missing)?),
            writer: Box::new(child.stdin.take().ok_or_else(missing)?),
            stderr: child
                .stderr
                .take()
                .map(|stderr| Box::new(stderr) as Box<dyn Read + Send>),
            child: Some(child),
        })
    })
}

/// The variables a plugin gets: where programs and home are, the temporary directory,
/// the locale, and on Windows `SystemRoot`, without which programs fail to start.
pub fn plugin_env(
    vars: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    vars.filter(|(name, _)| {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        matches!(
            upper.as_str(),
            "PATH"
                | "HOME"
                | "USERPROFILE"
                | "TMPDIR"
                | "TMP"
                | "TEMP"
                | "LANG"
                | "LANGUAGE"
                | "SYSTEMROOT"
        ) || upper.starts_with("LC_")
    })
    .collect()
}

/// `DEXO_DRIVER_<ID>` runs a local build in place of plugin `<ID>`, for developing one.
pub fn dev_override(plugin: &str) -> Option<std::path::PathBuf> {
    let name = format!(
        "DEXO_DRIVER_{}",
        plugin.to_ascii_uppercase().replace('-', "_")
    );
    std::env::var_os(name)
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}
