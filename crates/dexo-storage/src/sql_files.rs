use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn write_sql_file(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = temp_sibling(path);
    let write = write_and_sync(&tmp, content).and_then(|()| fs::rename(&tmp, path));
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write
}

/// Unique per write so concurrent writers to the same path cannot clobber each
/// other's temp file.
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
