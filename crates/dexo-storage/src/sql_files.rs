use std::fs;
use std::path::{Path, PathBuf};

use crate::AppPaths;

pub fn ensure_connection_sql_dir(paths: &AppPaths, connection_id: &str) -> std::io::Result<PathBuf> {
    let dir = paths.data_dir.join("sql").join(connection_id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn ensure_console_sql(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("console.sql");
    if !path.exists() {
        fs::write(&path, b"")?;
    }
    Ok(path)
}

pub fn list_sql_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn write_sql_file(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("sql.tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
}
