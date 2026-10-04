use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeToolKind {
    PgDump,
    PgRestore,
    MysqlDump,
    MysqlRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRunResult {
    pub command_line: String,
    /// What the tool said on its error output, last lines, with the password masked.
    pub sanitized_log: String,
    pub status: NativeStatus,
    /// `pg_restore` finishes with "errors ignored on restore: N" when it went on past
    /// statements the server refused. The data is there; the count is what to tell.
    pub ignored_errors: Option<u32>,
}

#[derive(Debug)]
pub struct PreparedTool {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub command_line: String,
    passfile: Option<PathBuf>,
}

impl Drop for PreparedTool {
    fn drop(&mut self) {
        if let Some(path) = &self.passfile {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl PreparedTool {
    pub fn cleanup(&mut self) {
        if let Some(path) = self.passfile.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NativeToolError {
    #[error("native tool version {found} is incompatible with major {expected}")]
    VersionMismatch { found: String, expected: u32 },
    #[error("{0}")]
    Io(String),
    /// The program is not installed, or not on the PATH.
    #[error("{} was not found on this computer. Install the {} client tools, then try again.", .program, family(.program))]
    ToolMissing { program: String },
}

/// The product a client tool belongs to, for the message that says to install it.
fn family(program: &str) -> &'static str {
    if program.starts_with("mysql") {
        "MySQL"
    } else {
        "PostgreSQL"
    }
}

pub fn parse_major(version: &str) -> Option<u32> {
    version
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
}

#[derive(Clone, Debug)]
pub struct NativeToolRequest {
    pub kind: NativeToolKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub path: PathBuf,
    pub secret: SecretString,
    pub expected_major: u32,
}

/// An empty file only its owner reads: a backup holds the data, and a file the tool makes
/// for itself takes whatever the process's umask lets others see.
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn sibling_part_file(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "dump".into());
    name.push(".part");
    path.with_file_name(name)
}

/// A backup named `.sql` is a script a person can read; any other name is a PostgreSQL
/// archive, which `pg_restore` can pick tables out of.
pub fn backup_is_plain_sql(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
}

/// What a PostgreSQL backup file is, which decides the tool that reads it back: archives
/// go to `pg_restore`, a script to `psql`. Read from the content, not the name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresBackup {
    Archive,
    PlainSql,
}

pub fn postgres_backup_kind(path: &Path) -> PostgresBackup {
    use std::io::Read;
    if path.is_dir() {
        return PostgresBackup::Archive;
    }
    let mut head = [0u8; 512];
    let read = std::fs::File::open(path).and_then(|mut file| file.read(&mut head));
    match read {
        Ok(read) => {
            let head = &head[..read];
            // A custom archive starts with PGDMP, a tar one has its magic at 257.
            if head.starts_with(b"PGDMP") || head.get(257..262) == Some(b"ustar") {
                PostgresBackup::Archive
            } else {
                PostgresBackup::PlainSql
            }
        }
        // Unreadable: the tool says why in its own words.
        Err(_) => PostgresBackup::Archive,
    }
}

pub fn prepare(
    request: &NativeToolRequest,
    version: &str,
    dir: &Path,
) -> Result<PreparedTool, NativeToolError> {
    // ponytail: expected_major 0 skips the check until Session exposes server version.
    if request.expected_major != 0 {
        let found = parse_major(version).unwrap_or(0);
        if found != request.expected_major {
            return Err(NativeToolError::VersionMismatch {
                found: version.into(),
                expected: request.expected_major,
            });
        }
    }
    let secret = request.secret.expose_secret();
    let passfile = dir.join(match request.kind {
        NativeToolKind::PgDump | NativeToolKind::PgRestore => "pgpass",
        NativeToolKind::MysqlDump | NativeToolKind::MysqlRestore => "my.cnf",
    });
    let contents = match request.kind {
        NativeToolKind::PgDump | NativeToolKind::PgRestore => {
            format!(
                "{}:{}:*:{}:{secret}\n",
                request.host, request.port, request.username
            )
        }
        NativeToolKind::MysqlDump | NativeToolKind::MysqlRestore => {
            format!("[client]\npassword={secret}\n")
        }
    };
    std::fs::write(&passfile, contents).map_err(|error| NativeToolError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&passfile, std::fs::Permissions::from_mode(0o600));
    }
    let path = request.path.display().to_string();
    let port = request.port.to_string();
    let (program, args, env): (String, Vec<String>, Vec<(String, String)>) = match request.kind {
        // Written beside the backup and moved into place once it is whole: a backup that
        // fails or is stopped leaves the file that was there, and never half a file.
        NativeToolKind::PgDump => (
            "pg_dump".into(),
            vec![
                "--no-password".into(),
                "--host".into(),
                request.host.clone(),
                "--port".into(),
                port,
                "--username".into(),
                request.username.clone(),
                "--format".into(),
                if backup_is_plain_sql(&request.path) {
                    "plain"
                } else {
                    "custom"
                }
                .into(),
                "--file".into(),
                sibling_part_file(&request.path).display().to_string(),
                request.database.clone(),
            ],
            vec![("PGPASSFILE".into(), passfile.display().to_string())],
        ),
        NativeToolKind::PgRestore => {
            let connection = [
                "--no-password".to_string(),
                "--host".into(),
                request.host.clone(),
                "--port".into(),
                port,
                "--username".into(),
                request.username.clone(),
                "--dbname".into(),
                request.database.clone(),
            ];
            let env = vec![("PGPASSFILE".into(), passfile.display().to_string())];
            match postgres_backup_kind(&request.path) {
                PostgresBackup::Archive => (
                    "pg_restore".into(),
                    connection.into_iter().chain([path]).collect(),
                    env,
                ),
                // A script goes on past a statement the server refuses, as `pg_restore`
                // does -- a newer pg_dump writes settings an older server has not heard
                // of -- and `outcome` counts what it went past.
                PostgresBackup::PlainSql => (
                    "psql".into(),
                    connection
                        .into_iter()
                        .chain(["--quiet".into(), "--file".into(), path])
                        .collect(),
                    env,
                ),
            }
        }
        NativeToolKind::MysqlDump => (
            "mysqldump".into(),
            vec![
                "--defaults-extra-file".into(),
                passfile.display().to_string(),
                "--host".into(),
                request.host.clone(),
                "--port".into(),
                port,
                "--user".into(),
                request.username.clone(),
                request.database.clone(),
            ],
            vec![],
        ),
        NativeToolKind::MysqlRestore => (
            "mysql".into(),
            vec![
                "--defaults-extra-file".into(),
                passfile.display().to_string(),
                "--host".into(),
                request.host.clone(),
                "--port".into(),
                port,
                "--user".into(),
                request.username.clone(),
                request.database.clone(),
            ],
            vec![],
        ),
    };
    let command_line = std::iter::once(program.as_str())
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(PreparedTool {
        program,
        args,
        env,
        command_line,
        passfile: Some(passfile),
    })
}

#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<PathBuf>,
    pub stdout: Option<PathBuf>,
}

#[async_trait::async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn spawn(&self, spec: ProcessSpec) -> Result<Box<dyn RunningProcess>, NativeToolError>;
}

/// A process under way. `cancel` and `wait` take `&self` because they run at the same
/// time: one task waits for the end while the person's Cancel arrives from another.
#[async_trait::async_trait]
pub trait RunningProcess: Send + Sync {
    /// Stops the process. A `wait` under way ends as `Cancelled`.
    async fn cancel(&self) -> Result<(), NativeToolError>;
    async fn wait(&self) -> Result<NativeStatus, NativeToolError>;
    /// The last lines the process wrote on its error output.
    fn log(&self) -> String {
        String::new()
    }
}

pub struct NativeToolRunner<R: ProcessRunner> {
    process: R,
}

impl<R: ProcessRunner> NativeToolRunner<R> {
    pub fn new(process: R) -> Self {
        Self { process }
    }

    pub async fn start(
        &self,
        request: NativeToolRequest,
        version: &str,
        dir: &Path,
    ) -> Result<NativeHandle, NativeToolError> {
        let part = sibling_part_file(&request.path);
        // The dump is written beside its place and moved there when it is whole.
        let persist = matches!(
            request.kind,
            NativeToolKind::PgDump | NativeToolKind::MysqlDump
        )
        .then(|| (part.clone(), request.path.clone()));
        let stdin = match request.kind {
            NativeToolKind::MysqlRestore => Some(request.path.clone()),
            _ => None,
        };
        let stdout = (request.kind == NativeToolKind::MysqlDump).then_some(part);
        let prepared = prepare(&request, version, dir)?;
        // pg_dump opens the file it is given; made here, it is private from the first byte.
        if request.kind == NativeToolKind::PgDump
            && let Some((part, _)) = &persist
        {
            create_private(part).map_err(|error| NativeToolError::Io(error.to_string()))?;
        }
        let spec = ProcessSpec {
            program: prepared.program.clone(),
            args: prepared.args.clone(),
            env: prepared.env.clone(),
            stdin,
            stdout,
        };
        let child = match self.process.spawn(spec).await {
            Ok(child) => child,
            Err(error) => {
                // A tool that never started leaves no half-made file behind it.
                if let Some((part, _)) = &persist {
                    let _ = std::fs::remove_file(part);
                }
                return Err(error);
            }
        };
        Ok(NativeHandle {
            command_line: prepared.command_line.clone(),
            secret_file: prepared
                .passfile
                .clone()
                .unwrap_or_else(|| dir.join("secret")),
            child,
            cancelled: AtomicBool::new(false),
            kind: request.kind,
            secret: request.secret,
            prepared,
            persist,
        })
    }
}

pub struct NativeHandle {
    pub command_line: String,
    secret_file: PathBuf,
    child: Box<dyn RunningProcess>,
    cancelled: AtomicBool,
    kind: NativeToolKind,
    secret: SecretString,
    #[allow(dead_code)]
    prepared: PreparedTool,
    /// A backup's file as it is written, and where it goes once it is whole.
    persist: Option<(PathBuf, PathBuf)>,
}

impl NativeHandle {
    pub fn secret_file(&self) -> &Path {
        &self.secret_file
    }

    /// How much of a backup is written so far.
    pub fn written_bytes(&self) -> u64 {
        self.persist
            .as_ref()
            .and_then(|(part, _)| std::fs::metadata(part).ok())
            .map_or(0, |metadata| metadata.len())
    }

    /// Stops the tool. Safe to call while `outcome` is waiting for it.
    pub async fn cancel(&self) -> Result<(), NativeToolError> {
        self.cancelled.store(true, Ordering::Release);
        let stopped = self.child.cancel().await;
        self.cleanup_secret();
        stopped
    }

    pub async fn outcome(&self) -> Result<NativeRunResult, NativeToolError> {
        let waited = self.child.wait().await;
        self.cleanup_secret();
        let mut status = match waited {
            Ok(status) => status,
            Err(error) => {
                self.drop_partial_file();
                return Err(error);
            }
        };
        if self.cancelled.load(Ordering::Acquire) {
            status = NativeStatus::Cancelled;
        }
        let log = scrub(&self.child.log(), self.secret.expose_secret());
        let ignored_errors = match (self.kind, status) {
            // pg_restore exits 1 when it went on past errors it was told to ignore.
            (NativeToolKind::PgRestore, NativeStatus::Failed) => ignored_errors(&log),
            // psql, running a script, exits 0 whatever the statements did.
            (NativeToolKind::PgRestore, NativeStatus::Succeeded) => {
                Some(script_errors(&log)).filter(|errors| *errors > 0)
            }
            _ => None,
        };
        if ignored_errors.is_some() {
            status = NativeStatus::Succeeded;
        }
        match (&self.persist, status) {
            (Some((part, dest)), NativeStatus::Succeeded) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(part, std::fs::Permissions::from_mode(0o600));
                }
                std::fs::rename(part, dest)
                    .map_err(|error| NativeToolError::Io(error.to_string()))?;
            }
            _ => self.drop_partial_file(),
        }
        Ok(NativeRunResult {
            command_line: self.command_line.clone(),
            sanitized_log: log,
            status,
            ignored_errors,
        })
    }

    fn drop_partial_file(&self) {
        if let Some((part, _)) = &self.persist {
            let _ = std::fs::remove_file(part);
        }
    }

    fn cleanup_secret(&self) {
        let _ = std::fs::remove_file(&self.secret_file);
    }
}

impl Drop for NativeHandle {
    fn drop(&mut self) {
        self.cleanup_secret();
    }
}

/// The N of pg_restore's closing line `pg_restore: warning: errors ignored on restore: 1`.
fn ignored_errors(log: &str) -> Option<u32> {
    log.lines().find_map(|line| {
        line.split_once("errors ignored on restore:")
            .and_then(|(_, count)| count.trim().parse().ok())
    })
}

/// The statements of a script `psql` reported as errors: `psql:file.sql:12: ERROR:  ...`.
fn script_errors(log: &str) -> u32 {
    log.lines().filter(|line| line.contains("ERROR:")).count() as u32
}

/// `log` with the password, if the tool ever echoed it, masked.
fn scrub(log: &str, secret: &str) -> String {
    if secret.is_empty() {
        log.to_string()
    } else {
        log.replace(secret, "***")
    }
}

pub struct TokioProcessRunner;

/// Lines of error output kept: what a person reads is the last of it.
const LOG_LINES: usize = 60;

#[async_trait::async_trait]
impl ProcessRunner for TokioProcessRunner {
    async fn spawn(&self, spec: ProcessSpec) -> Result<Box<dyn RunningProcess>, NativeToolError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command.args(&spec.args);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        command.kill_on_drop(true);
        // Never the terminal's: a tool that reads it would take the keys the TUI is
        // waiting for, and one that asks for a password would hang on a prompt.
        match &spec.stdin {
            Some(path) => {
                let file = std::fs::File::open(path)
                    .map_err(|error| NativeToolError::Io(format!("{}: {error}", path.display())))?;
                command.stdin(Stdio::from(file));
            }
            None => {
                command.stdin(Stdio::null());
            }
        }
        match &spec.stdout {
            Some(path) => {
                let file = create_private(path)
                    .map_err(|error| NativeToolError::Io(format!("{}: {error}", path.display())))?;
                command.stdout(Stdio::from(file));
            }
            None => {
                command.stdout(Stdio::null());
            }
        }
        command.stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                // The file the output was to go to is made before the tool starts.
                if let Some(path) = &spec.stdout {
                    let _ = std::fs::remove_file(path);
                }
                return Err(if error.kind() == std::io::ErrorKind::NotFound {
                    NativeToolError::ToolMissing {
                        program: spec.program.clone(),
                    }
                } else {
                    NativeToolError::Io(format!("{}: {error}", spec.program))
                });
            }
        };
        let log = Arc::new(Mutex::new(VecDeque::new()));
        // Read as it comes: a tool whose error output nobody reads stops at a full pipe.
        let reader = child.stderr.take().map(|stderr| {
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).split(b'\n');
                while let Ok(Some(line)) = lines.next_segment().await {
                    let mut log = log
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if log.len() == LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(String::from_utf8_lossy(&line).trim_end().to_string());
                }
            })
        });
        Ok(Box::new(TokioChild {
            child: tokio::sync::Mutex::new(child),
            stop: tokio::sync::Notify::new(),
            log,
            reader: tokio::sync::Mutex::new(reader),
        }))
    }
}

struct TokioChild {
    /// Held by `wait` for as long as the process runs.
    child: tokio::sync::Mutex<tokio::process::Child>,
    stop: tokio::sync::Notify,
    log: Arc<Mutex<VecDeque<String>>>,
    reader: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[async_trait::async_trait]
impl RunningProcess for TokioChild {
    async fn cancel(&self) -> Result<(), NativeToolError> {
        // A permit is kept if nobody is waiting yet, so a cancel is never lost.
        self.stop.notify_one();
        Ok(())
    }

    async fn wait(&self) -> Result<NativeStatus, NativeToolError> {
        let mut child = self.child.lock().await;
        let status = tokio::select! {
            status = child.wait() => Some(status),
            () = self.stop.notified() => {
                let _ = child.kill().await;
                None
            }
        };
        // The last of the error output is written as the process ends.
        if let Some(reader) = self.reader.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
        }
        match status {
            None => Ok(NativeStatus::Cancelled),
            Some(Ok(status)) if status.success() => Ok(NativeStatus::Succeeded),
            Some(Ok(_)) => Ok(NativeStatus::Failed),
            Some(Err(error)) => Err(NativeToolError::Io(error.to_string())),
        }
    }

    fn log(&self) -> String {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeStatus, NativeToolError, NativeToolKind, NativeToolRequest, NativeToolRunner,
        PostgresBackup, ProcessRunner, ProcessSpec, RunningProcess, TokioProcessRunner,
        backup_is_plain_sql, ignored_errors, postgres_backup_kind, prepare, script_errors,
    };
    use secrecy::SecretString;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn dump_request(secret: &str, path: PathBuf) -> NativeToolRequest {
        NativeToolRequest {
            kind: NativeToolKind::PgDump,
            host: "localhost".into(),
            port: 5432,
            database: "dexo".into(),
            username: "dexo".into(),
            path,
            secret: SecretString::from(secret),
            expected_major: 16,
        }
    }

    struct RecordingRunner {
        cancelled: Arc<AtomicBool>,
        secret: PathBuf,
        log: &'static str,
        status: NativeStatus,
    }

    struct RecordingChild {
        cancelled: Arc<AtomicBool>,
        secret: PathBuf,
        log: &'static str,
        status: NativeStatus,
    }

    #[async_trait::async_trait]
    impl ProcessRunner for RecordingRunner {
        async fn spawn(
            &self,
            spec: ProcessSpec,
        ) -> Result<Box<dyn RunningProcess>, NativeToolError> {
            assert!(!spec.args.iter().any(|arg| arg.contains("SECRET")));
            Ok(Box::new(RecordingChild {
                cancelled: Arc::clone(&self.cancelled),
                secret: self.secret.clone(),
                log: self.log,
                status: self.status,
            }))
        }
    }

    #[async_trait::async_trait]
    impl RunningProcess for RecordingChild {
        async fn cancel(&self) -> Result<(), NativeToolError> {
            self.cancelled.store(true, Ordering::SeqCst);
            let _ = std::fs::remove_file(&self.secret);
            Ok(())
        }

        async fn wait(&self) -> Result<NativeStatus, NativeToolError> {
            if self.cancelled.load(Ordering::SeqCst) {
                Ok(NativeStatus::Cancelled)
            } else {
                Ok(self.status)
            }
        }

        fn log(&self) -> String {
            self.log.into()
        }
    }

    fn recording(dir: &Path, log: &'static str, status: NativeStatus) -> RecordingRunner {
        RecordingRunner {
            cancelled: Arc::new(AtomicBool::new(false)),
            secret: dir.join("pgpass"),
            log,
            status,
        }
    }

    #[tokio::test]
    async fn password_never_appears_in_arguments_or_logs() {
        let dir = tempfile::tempdir().unwrap();
        let runner = NativeToolRunner::new(recording(
            dir.path(),
            "FATAL: password authentication failed: SUPER_SECRET_SENTINEL",
            NativeStatus::Failed,
        ));
        let handle = runner
            .start(
                dump_request("SUPER_SECRET_SENTINEL", dir.path().join("out.dump")),
                "16.9",
                dir.path(),
            )
            .await
            .unwrap();
        assert!(!handle.command_line.contains("SUPER_SECRET_SENTINEL"));
        let result = handle.outcome().await.unwrap();
        assert!(!result.sanitized_log.contains("SUPER_SECRET_SENTINEL"));
        assert!(result.sanitized_log.contains("authentication failed"));
        assert!(!handle.secret_file().exists());
    }

    #[tokio::test]
    async fn cancellation_kills_the_child_and_removes_secret_material() {
        let dir = tempfile::tempdir().unwrap();
        let runner = NativeToolRunner::new(recording(dir.path(), "", NativeStatus::Succeeded));
        let handle = runner
            .start(
                dump_request("SECRET", dir.path().join("out.dump")),
                "16.9",
                dir.path(),
            )
            .await
            .unwrap();
        handle.cancel().await.unwrap();
        assert!(!handle.secret_file().exists());
        assert_eq!(
            handle.outcome().await.unwrap().status,
            NativeStatus::Cancelled
        );
    }

    #[test]
    fn version_mismatch_and_mysql_option_file() {
        let dir = tempfile::tempdir().unwrap();
        let error = prepare(
            &dump_request("x", dir.path().join("out.dump")),
            "15.4",
            dir.path(),
        )
        .unwrap_err();
        assert!(matches!(error, NativeToolError::VersionMismatch { .. }));
        let mysql = prepare(
            &NativeToolRequest {
                kind: NativeToolKind::MysqlDump,
                host: "localhost".into(),
                port: 3306,
                database: "dexo".into(),
                username: "dexo".into(),
                path: dir.path().join("out.sql"),
                secret: SecretString::from("SECRET"),
                expected_major: 8,
            },
            "8.4",
            dir.path(),
        )
        .unwrap();
        assert!(!mysql.command_line.contains("SECRET"));
        assert!(
            mysql
                .args
                .iter()
                .any(|arg| arg.contains("defaults-extra-file"))
        );
    }

    #[test]
    fn dump_and_restore_use_request_path_not_hardcoded_backup() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("orders.dump");
        let dump = prepare(&dump_request("SECRET", dest.clone()), "16.9", dir.path()).unwrap();
        // The dump is written beside its place and moved there once it is whole.
        assert!(
            dump.args
                .iter()
                .any(|arg| arg == dir.path().join("orders.dump.part").to_str().unwrap())
        );
        assert!(!dump.args.iter().any(|arg| arg == "backup.dump"));
        let restore = prepare(
            &NativeToolRequest {
                kind: NativeToolKind::PgRestore,
                host: "db.example".into(),
                port: 5433,
                database: "app".into(),
                username: "owner".into(),
                path: dest.clone(),
                secret: SecretString::from("SECRET"),
                expected_major: 16,
            },
            "16.9",
            dir.path(),
        )
        .unwrap();
        assert!(restore.args.iter().any(|arg| arg == dest.to_str().unwrap()));
        assert!(restore.args.iter().any(|arg| arg == "db.example"));
        assert!(!restore.command_line.contains("SECRET"));
    }

    fn restore_request(path: PathBuf) -> NativeToolRequest {
        NativeToolRequest {
            kind: NativeToolKind::PgRestore,
            ..dump_request("x", path)
        }
    }

    /// Dexo's backup and Dexo's restore agree: what `Native Backup` writes -- a script
    /// for a `.sql` name, an archive for any other -- is what `Native Restore` reads,
    /// each with the tool that reads it.
    #[test]
    fn a_backup_is_restored_with_the_tool_that_reads_its_format() {
        let dir = tempfile::tempdir().unwrap();
        let program_of = |request: NativeToolRequest| {
            prepare(&request, "16.9", dir.path())
                .unwrap()
                .program
                .clone()
        };
        let sql = dir.path().join("shop.sql");
        let dump = dump_request("x", sql.clone());
        let plain = prepare(&dump, "16.9", dir.path()).unwrap();
        assert!(
            plain
                .args
                .windows(2)
                .any(|pair| pair == ["--format", "plain"])
        );
        let archive = prepare(
            &dump_request("x", dir.path().join("shop.dump")),
            "16.9",
            dir.path(),
        )
        .unwrap();
        assert!(
            archive
                .args
                .windows(2)
                .any(|pair| pair == ["--format", "custom"])
        );

        std::fs::write(
            &sql,
            "-- PostgreSQL database dump\nCREATE TABLE t (a int);\n",
        )
        .unwrap();
        assert_eq!(postgres_backup_kind(&sql), PostgresBackup::PlainSql);
        assert_eq!(program_of(restore_request(sql.clone())), "psql");

        let custom = dir.path().join("shop.dump");
        std::fs::write(&custom, b"PGDMP\x01\x0e\x00rest").unwrap();
        assert_eq!(postgres_backup_kind(&custom), PostgresBackup::Archive);
        assert_eq!(program_of(restore_request(custom)), "pg_restore");

        // A directory-format backup is an archive too, whatever it is called.
        assert_eq!(postgres_backup_kind(dir.path()), PostgresBackup::Archive);
        assert!(backup_is_plain_sql(Path::new("a/Backup.SQL")));
        assert!(!backup_is_plain_sql(Path::new("a/backup.dump")));
    }

    /// A newer pg_dump writes `SET transaction_timeout`, which an older server refuses:
    /// the script restore goes on past it, as pg_restore does, and says how many.
    #[tokio::test]
    async fn a_script_restore_goes_on_past_errors_and_counts_them() {
        let dir = tempfile::tempdir().unwrap();
        let sql = dir.path().join("shop.sql");
        std::fs::write(&sql, "CREATE TABLE t (a int);\n").unwrap();
        let restore = prepare(&restore_request(sql.clone()), "16.9", dir.path()).unwrap();
        assert_eq!(restore.program, "psql");
        assert!(!restore.args.iter().any(|arg| arg == "ON_ERROR_STOP=1"));
        let log = "psql:shop.sql:7: ERROR:  unrecognized configuration parameter \"transaction_timeout\"\npsql:shop.sql:9: ERROR:  relation \"t\" already exists";
        assert_eq!(script_errors(log), 2);
        let runner = NativeToolRunner::new(recording(dir.path(), log, NativeStatus::Succeeded));
        let handle = runner
            .start(restore_request(sql), "16.9", dir.path())
            .await
            .unwrap();
        let result = handle.outcome().await.unwrap();
        assert_eq!(result.status, NativeStatus::Succeeded);
        assert_eq!(result.ignored_errors, Some(2));
    }

    #[test]
    fn pg_restore_counts_the_errors_it_went_on_past() {
        let log = "pg_restore: error: could not execute query: ERROR:  unrecognized configuration parameter \"transaction_timeout\"\npg_restore: warning: errors ignored on restore: 1";
        assert_eq!(ignored_errors(log), Some(1));
        assert_eq!(ignored_errors("pg_restore: error: connection failed"), None);
    }

    #[tokio::test]
    async fn restore_that_ignored_errors_succeeded_and_says_how_many() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("shop.dump");
        std::fs::write(&source, b"PGDMP").unwrap();
        let runner = NativeToolRunner::new(recording(
            dir.path(),
            "pg_restore: warning: errors ignored on restore: 2",
            NativeStatus::Failed,
        ));
        let handle = runner
            .start(restore_request(source), "16.9", dir.path())
            .await
            .unwrap();
        let result = handle.outcome().await.unwrap();
        assert_eq!(result.status, NativeStatus::Succeeded);
        assert_eq!(result.ignored_errors, Some(2));
    }

    #[tokio::test]
    async fn a_failed_backup_keeps_the_file_that_was_there() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("shop.dump");
        std::fs::write(&dest, b"the earlier backup").unwrap();
        std::fs::write(dir.path().join("shop.dump.part"), b"half").unwrap();
        let runner = NativeToolRunner::new(recording(dir.path(), "boom", NativeStatus::Failed));
        let handle = runner
            .start(dump_request("x", dest.clone()), "16.9", dir.path())
            .await
            .unwrap();
        let result = handle.outcome().await.unwrap();
        assert_eq!(result.status, NativeStatus::Failed);
        assert_eq!(std::fs::read(&dest).unwrap(), b"the earlier backup");
        assert!(!dir.path().join("shop.dump.part").exists());
    }

    #[tokio::test]
    async fn a_missing_tool_says_to_install_it_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("out.sql");
        let error = TokioProcessRunner
            .spawn(ProcessSpec {
                program: "dexo-no-such-tool".into(),
                args: Vec::new(),
                env: Vec::new(),
                stdin: None,
                stdout: Some(output.clone()),
            })
            .await
            .err()
            .expect("the tool does not exist");
        assert!(matches!(error, NativeToolError::ToolMissing { .. }));
        assert!(!output.exists());
        let mysql = NativeToolError::ToolMissing {
            program: "mysqldump".into(),
        };
        assert_eq!(
            mysql.to_string(),
            "mysqldump was not found on this computer. Install the MySQL client tools, then try again."
        );
    }

    /// The process is stopped while another task waits for it: the case the UI is in.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_stops_a_process_that_is_being_waited_for() {
        let child = TokioProcessRunner
            .spawn(ProcessSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "echo started >&2; sleep 30".into()],
                env: Vec::new(),
                stdin: None,
                stdout: None,
            })
            .await
            .unwrap();
        let child = Arc::new(child);
        let waiting = tokio::spawn({
            let child = Arc::clone(&child);
            async move { child.wait().await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        child.cancel().await.unwrap();
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .expect("the wait ended")
            .unwrap()
            .unwrap();
        assert_eq!(status, NativeStatus::Cancelled);
        assert!(child.log().contains("started"));
    }

    /// The tool's own words are kept, from a real process.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_tools_error_output_is_kept() {
        let child = TokioProcessRunner
            .spawn(ProcessSpec {
                program: "sh".into(),
                args: vec!["-c".into(), "echo 'no such table' >&2; exit 3".into()],
                env: Vec::new(),
                stdin: None,
                stdout: None,
            })
            .await
            .unwrap();
        assert_eq!(child.wait().await.unwrap(), NativeStatus::Failed);
        assert_eq!(child.log(), "no such table");
    }
}
