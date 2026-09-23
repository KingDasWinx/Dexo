use std::io;
use std::path::{Path, PathBuf};

use dexo_storage::FileFingerprint;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum DocumentIoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("external change detected at {}", path.display())]
    ExternalConflict {
        path: PathBuf,
        disk: FileFingerprint,
    },
}

pub async fn save_sql_atomic(path: &Path, content: &str) -> Result<(), DocumentIoError> {
    tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        let content = content.to_string();
        move || save_sql_atomic_sync(&path, &content)
    })
    .await
    .map_err(io::Error::from)?
}

pub async fn fingerprint(path: &Path) -> Result<FileFingerprint, DocumentIoError> {
    tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        move || fingerprint_sync(&path)
    })
    .await
    .map_err(io::Error::from)?
}

pub async fn save_if_unchanged(
    path: &Path,
    expected: &FileFingerprint,
    content: &str,
) -> Result<FileFingerprint, DocumentIoError> {
    let disk = fingerprint(path).await?;
    if disk.hash != expected.hash {
        return Err(DocumentIoError::ExternalConflict {
            path: path.to_path_buf(),
            disk,
        });
    }
    save_sql_atomic(path, content).await?;
    fingerprint(path).await
}

fn save_sql_atomic_sync(path: &Path, content: &str) -> Result<(), DocumentIoError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    dexo_storage::sql_files::write_sql_file(path, content)?;
    Ok(())
}

fn fingerprint_sync(path: &Path) -> Result<FileFingerprint, DocumentIoError> {
    let bytes = std::fs::read(path)?;
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|| "0".into());
    Ok(FileFingerprint {
        mtime,
        hash: hex_sha256(&bytes),
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
