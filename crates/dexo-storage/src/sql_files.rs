use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::AppPaths;

pub fn ensure_connection_sql_dir(
    paths: &AppPaths,
    connection_id: &str,
) -> std::io::Result<PathBuf> {
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
    let tmp = temp_sibling(path);
    let write = write_and_sync(&tmp, content).and_then(|()| fs::rename(&tmp, path));
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write
}

/// Unique per write so concurrent writers to the same path cannot clobber each
/// other's temp file. The `.tmp-*` suffix keeps it out of `list_sql_files`.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.sql");
    path.with_file_name(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()))
}

fn write_and_sync(tmp: &Path, content: &str) -> std::io::Result<()> {
    let mut file = fs::File::create(tmp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}
