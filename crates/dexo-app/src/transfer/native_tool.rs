use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

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
    pub sanitized_log: String,
    pub status: NativeStatus,
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

fn sibling_part_file(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "dump".into());
    name.push(".part");
    path.with_file_name(name)
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
                "--file".into(),
                path,
                request.database.clone(),
            ],
            vec![("PGPASSFILE".into(), passfile.display().to_string())],
        ),
        NativeToolKind::PgRestore => (
            "pg_restore".into(),
            vec![
                "--no-password".into(),
                "--host".into(),
                request.host.clone(),
                "--port".into(),
                port,
                "--username".into(),
                request.username.clone(),
                "--dbname".into(),
                request.database.clone(),
                path,
            ],
            vec![("PGPASSFILE".into(), passfile.display().to_string())],
        ),
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

#[async_trait::async_trait]
pub trait RunningProcess: Send {
    async fn cancel(&mut self) -> Result<(), NativeToolError>;
    async fn wait(&mut self) -> Result<NativeStatus, NativeToolError>;
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
        let stdout_persist = match request.kind {
            NativeToolKind::MysqlDump => {
                let tmp = sibling_part_file(&request.path);
                Some((tmp, request.path.clone()))
            }
            _ => None,
        };
        let stdin = match request.kind {
            NativeToolKind::MysqlRestore => Some(request.path.clone()),
            _ => None,
        };
        let stdout = stdout_persist.as_ref().map(|(tmp, _)| tmp.clone());
        let prepared = prepare(&request, version, dir)?;
        let spec = ProcessSpec {
            program: prepared.program.clone(),
            args: prepared.args.clone(),
            env: prepared.env.clone(),
            stdin,
            stdout,
        };
        let child = self.process.spawn(spec).await?;
        Ok(NativeHandle {
            command_line: prepared.command_line.clone(),
            secret_file: prepared
                .passfile
                .clone()
                .unwrap_or_else(|| dir.join("secret")),
            child: Mutex::new(Some(child)),
            cancelled: Mutex::new(false),
            prepared,
            stdout_persist,
        })
    }
}

pub struct NativeHandle {
    pub command_line: String,
    secret_file: PathBuf,
    child: Mutex<Option<Box<dyn RunningProcess>>>,
    cancelled: Mutex<bool>,
    #[allow(dead_code)]
    prepared: PreparedTool,
    stdout_persist: Option<(PathBuf, PathBuf)>,
}

impl NativeHandle {
    pub fn secret_file(&self) -> &Path {
        &self.secret_file
    }

    pub async fn cancel(&self) -> Result<(), NativeToolError> {
        let child = self.child.lock().expect("child").take();
        if let Some(mut child) = child {
            child.cancel().await?;
            *self.cancelled.lock().expect("cancelled") = true;
            *self.child.lock().expect("child") = Some(child);
        }
        self.cleanup_secret();
        Ok(())
    }

    pub async fn outcome(&self) -> Result<NativeRunResult, NativeToolError> {
        let child = self.child.lock().expect("child").take();
        let cancelled = *self.cancelled.lock().expect("cancelled");
        let status = if let Some(mut child) = child {
            let status = child.wait().await?;
            if cancelled {
                NativeStatus::Cancelled
            } else {
                status
            }
        } else if cancelled {
            NativeStatus::Cancelled
        } else {
            NativeStatus::Failed
        };
        self.cleanup_secret();
        if cancelled {
            if let Some((tmp, _)) = &self.stdout_persist {
                let _ = std::fs::remove_file(tmp);
            }
        } else if status == NativeStatus::Succeeded
            && let Some((tmp, dest)) = &self.stdout_persist
        {
            std::fs::rename(tmp, dest).map_err(|error| NativeToolError::Io(error.to_string()))?;
        }
        Ok(NativeRunResult {
            command_line: self.command_line.clone(),
            sanitized_log: format!("status={status:?} {}", self.command_line),
            status,
        })
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

pub struct TokioProcessRunner;

#[async_trait::async_trait]
impl ProcessRunner for TokioProcessRunner {
    async fn spawn(&self, spec: ProcessSpec) -> Result<Box<dyn RunningProcess>, NativeToolError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command.args(&spec.args);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        command.kill_on_drop(true);
        if let Some(path) = &spec.stdin {
            let file = std::fs::File::open(path)
                .map_err(|error| NativeToolError::Io(error.to_string()))?;
            command.stdin(Stdio::from(file));
        }
        if let Some(path) = &spec.stdout {
            let file = std::fs::File::create(path)
                .map_err(|error| NativeToolError::Io(error.to_string()))?;
            command.stdout(Stdio::from(file));
        } else {
            command.stdout(Stdio::piped());
        }
        command.stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|error| NativeToolError::Io(error.to_string()))?;
        Ok(Box::new(TokioChild { child: Some(child) }))
    }
}

struct TokioChild {
    child: Option<tokio::process::Child>,
}

#[async_trait::async_trait]
impl RunningProcess for TokioChild {
    async fn cancel(&mut self) -> Result<(), NativeToolError> {
        if let Some(child) = &mut self.child {
            let _ = child.kill().await;
        }
        Ok(())
    }

    async fn wait(&mut self) -> Result<NativeStatus, NativeToolError> {
        let Some(mut child) = self.child.take() else {
            return Ok(NativeStatus::Failed);
        };
        match child.wait().await {
            Ok(status) if status.success() => Ok(NativeStatus::Succeeded),
            Ok(_) => Ok(NativeStatus::Failed),
            Err(error) => Err(NativeToolError::Io(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeStatus, NativeToolError, NativeToolKind, NativeToolRequest, NativeToolRunner,
        ProcessRunner, ProcessSpec, RunningProcess, prepare,
    };
    use secrecy::SecretString;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

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
        cancelled: Arc<Mutex<bool>>,
        secret: PathBuf,
    }

    struct RecordingChild {
        cancelled: Arc<Mutex<bool>>,
        secret: PathBuf,
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
            }))
        }
    }

    #[async_trait::async_trait]
    impl RunningProcess for RecordingChild {
        async fn cancel(&mut self) -> Result<(), NativeToolError> {
            *self.cancelled.lock().expect("cancel") = true;
            let _ = std::fs::remove_file(&self.secret);
            Ok(())
        }

        async fn wait(&mut self) -> Result<NativeStatus, NativeToolError> {
            if *self.cancelled.lock().expect("cancel") {
                Ok(NativeStatus::Cancelled)
            } else {
                Ok(NativeStatus::Succeeded)
            }
        }
    }

    #[tokio::test]
    async fn password_never_appears_in_arguments_or_logs() {
        let dir = tempfile::tempdir().unwrap();
        let runner = NativeToolRunner::new(RecordingRunner {
            cancelled: Arc::new(Mutex::new(false)),
            secret: dir.path().join("pgpass"),
        });
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
        assert!(!handle.secret_file().exists());
    }

    #[tokio::test]
    async fn cancellation_kills_the_child_and_removes_secret_material() {
        let dir = tempfile::tempdir().unwrap();
        let cancelled = Arc::new(Mutex::new(false));
        let runner = NativeToolRunner::new(RecordingRunner {
            cancelled: Arc::clone(&cancelled),
            secret: dir.path().join("pgpass"),
        });
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
        assert!(*cancelled.lock().unwrap());
        let _ = Path::new(".");
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
        assert!(dump.args.iter().any(|arg| arg == dest.to_str().unwrap()));
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
}
