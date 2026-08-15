use std::fs;
use std::path::PathBuf;

use uuid::Uuid;

pub struct SpoolFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub total: u64,
}

pub fn spool_bytes(dir: &std::path::Path, bytes: &[u8]) -> std::io::Result<SpoolFile> {
    fs::create_dir_all(dir)?;
    let id = Uuid::new_v4();
    let path = dir.join(format!("{id}.bin"));
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(SpoolFile {
        id,
        path,
        total: bytes.len() as u64,
    })
}

pub fn delete_spool(file: &SpoolFile) {
    let _ = fs::remove_file(&file.path);
}

pub fn delete_partial(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("tmp") {
            let _ = fs::remove_file(path);
        }
    }
}
