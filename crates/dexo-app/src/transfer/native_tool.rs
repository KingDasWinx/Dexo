use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

pub fn prepare(
    kind: NativeToolKind,
    secret: &str,
    version: &str,
    expected_major: u32,
    dir: &Path,
) -> Result<PreparedTool, NativeToolError> {
    let found = parse_major(version).unwrap_or(0);
    if found != expected_major {
        return Err(NativeToolError::VersionMismatch {
            found: version.into(),
            expected: expected_major,
        });
    }
    let passfile = dir.join(match kind {
        NativeToolKind::PgDump | NativeToolKind::PgRestore => "pgpass",
        NativeToolKind::MysqlDump | NativeToolKind::MysqlRestore => "my.cnf",
    });
    let contents = match kind {
        NativeToolKind::PgDump | NativeToolKind::PgRestore => {
            format!("localhost:5432:*:dexo:{secret}\n")
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
    let (program, args, env): (String, Vec<String>, Vec<(String, String)>) = match kind {
        NativeToolKind::PgDump => (
            "pg_dump".into(),
            vec![
                "--no-password".into(),
                "--file".into(),
                "backup.dump".into(),
            ],
            vec![("PGPASSFILE".into(), passfile.display().to_string())],
        ),
        NativeToolKind::PgRestore => (
            "pg_restore".into(),
            vec!["--no-password".into(), "backup.dump".into()],
            vec![("PGPASSFILE".into(), passfile.display().to_string())],
        ),
        NativeToolKind::MysqlDump => (
            "mysqldump".into(),
            vec![
                "--defaults-extra-file".into(),
                passfile.display().to_string(),
            ],
            vec![],
        ),
        NativeToolKind::MysqlRestore => (
            "mysql".into(),
            vec![
                "--defaults-extra-file".into(),
                passfile.display().to_string(),
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
        kind: NativeToolKind,
        secret: &str,
        version: &str,
        expected_major: u32,
        dir: &Path,
    ) -> Result<NativeHandle, NativeToolError> {
        let prepared = prepare(kind, secret, version, expected_major, dir)?;
        let spec = ProcessSpec {
            program: prepared.program.clone(),
            args: prepared.args.clone(),
            env: prepared.env.clone(),
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
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
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
        NativeStatus, NativeToolError, NativeToolKind, NativeToolRunner, ProcessRunner,
        ProcessSpec, RunningProcess, prepare,
    };
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

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
                NativeToolKind::PgDump,
                "SUPER_SECRET_SENTINEL",
                "16.9",
                16,
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
            .start(NativeToolKind::PgDump, "SECRET", "16.9", 16, dir.path())
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
        let error = prepare(NativeToolKind::PgDump, "x", "15.4", 16, dir.path()).unwrap_err();
        assert!(matches!(error, NativeToolError::VersionMismatch { .. }));
        let mysql = prepare(NativeToolKind::MysqlDump, "SECRET", "8.4", 8, dir.path()).unwrap();
        assert!(!mysql.command_line.contains("SECRET"));
        assert!(
            mysql
                .args
                .iter()
                .any(|arg| arg.contains("defaults-extra-file"))
        );
    }
}
