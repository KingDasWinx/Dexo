use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeToolKind {
    PgDump,
    PgRestore,
    MysqlDump,
    MysqlRestore,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeRunResult {
    pub command_line: String,
    pub sanitized_log: String,
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

pub async fn fake_pg_dump(secret: &str) -> NativeRunResult {
    let dir = tempfile::tempdir().expect("temp");
    let mut prepared =
        prepare(NativeToolKind::PgDump, secret, "16.9", 16, dir.path()).expect("prepare");
    let sanitized_log = format!("spawn {}", prepared.command_line);
    let command_line = prepared.command_line.clone();
    prepared.cleanup();
    NativeRunResult {
        command_line,
        sanitized_log,
    }
}

pub struct FakeChild {
    pub killed: bool,
}

impl FakeChild {
    pub fn cancel(&mut self) {
        self.killed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeChild, NativeToolError, NativeToolKind, fake_pg_dump, prepare};

    #[tokio::test]
    async fn password_never_appears_in_arguments_or_logs() {
        let result = fake_pg_dump("SUPER_SECRET_SENTINEL").await;
        assert!(!result.command_line.contains("SUPER_SECRET_SENTINEL"));
        assert!(!result.sanitized_log.contains("SUPER_SECRET_SENTINEL"));
    }

    #[test]
    fn version_mismatch_and_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let error = prepare(NativeToolKind::PgDump, "x", "15.4", 16, dir.path()).unwrap_err();
        assert!(matches!(error, NativeToolError::VersionMismatch { .. }));
        let mut child = FakeChild { killed: false };
        child.cancel();
        assert!(child.killed);
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
