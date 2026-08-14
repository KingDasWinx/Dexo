use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const SECRET_SENTINEL: &str = "SUPER_SECRET_SENTINEL";

#[derive(Clone)]
pub struct OpaqueSecret(#[allow(dead_code)] String);

impl OpaqueSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Debug for OpaqueSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticBundle {
    pub versions: String,
    pub capabilities: String,
    pub config: String,
    pub log_tail: String,
    pub preview: String,
}

impl DiagnosticBundle {
    pub fn assemble(
        versions: String,
        capabilities: String,
        config: String,
        log_tail: String,
    ) -> Self {
        let config = redact_text(&config);
        let log_tail = redact_text(&log_tail);
        let preview = format!(
            "versions:\n{versions}\n\ncapabilities:\n{capabilities}\n\nconfig:\n{config}\n\nlogs:\n{log_tail}"
        );
        Self {
            versions,
            capabilities,
            config,
            log_tail,
            preview,
        }
    }

    pub fn write_zip(&self, path: &Path) -> io::Result<()> {
        let preview = format!(
            "Local diagnostic preview. Dexo never uploads this bundle automatically.\n\n{}",
            self.preview
        );
        let files = [
            ("versions.txt", self.versions.as_bytes()),
            ("capabilities.txt", self.capabilities.as_bytes()),
            ("config.redacted.toml", self.config.as_bytes()),
            ("logs.tail.txt", self.log_tail.as_bytes()),
            ("PREVIEW.txt", preview.as_bytes()),
        ];
        write_store_zip(path, &files)
    }
}

pub fn redact_text(text: &str) -> String {
    let mut out = text.replace(SECRET_SENTINEL, "[redacted]");
    for needle in [
        "password=",
        "Password=",
        "secret=",
        "api_key=",
        "postgres://",
        "mysql://",
    ] {
        out = redact_assignment(&out, needle);
    }
    out
}

fn redact_assignment(text: &str, needle: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(idx) = rest.find(needle) {
        out.push_str(&rest[..idx]);
        out.push_str(needle);
        out.push_str("[redacted]");
        rest = &rest[idx + needle.len()..];
        if needle.ends_with('=') {
            let skip = rest
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .unwrap_or(rest.len());
            rest = &rest[skip..];
        } else {
            let skip = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            rest = &rest[skip..];
        }
    }
    out.push_str(rest);
    out
}

pub struct SizeRotatingWriter {
    dir: PathBuf,
    stem: String,
    max_bytes: u64,
    max_files: usize,
    file: File,
}

impl SizeRotatingWriter {
    pub fn open(
        dir: impl Into<PathBuf>,
        stem: &str,
        max_bytes: u64,
        max_files: usize,
    ) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{stem}.log"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            dir,
            stem: stem.into(),
            max_bytes,
            max_files: max_files.max(1),
            file,
        })
    }

    fn current_path(&self) -> PathBuf {
        self.dir.join(format!("{}.log", self.stem))
    }

    fn rotate_if_needed(&mut self, extra: u64) -> io::Result<()> {
        let len = self.file.metadata()?.len();
        if len.saturating_add(extra) <= self.max_bytes {
            return Ok(());
        }
        self.file.flush()?;
        self.file = File::create(self.dir.join(format!("{}.rotating", self.stem)))?;
        let _ = fs::remove_file(
            self.dir
                .join(format!("{}.{}.log", self.stem, self.max_files)),
        );
        for index in (2..=self.max_files).rev() {
            let from = self.dir.join(format!("{}.{}.log", self.stem, index - 1));
            let to = self.dir.join(format!("{}.{}.log", self.stem, index));
            if from.exists() {
                let _ = fs::rename(&from, &to);
            }
        }
        let current = self.current_path();
        if current.exists() {
            let _ = fs::rename(&current, self.dir.join(format!("{}.1.log", self.stem)));
        }
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(current)?;
        let _ = fs::remove_file(self.dir.join(format!("{}.rotating", self.stem)));
        Ok(())
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_if_needed(buf.len() as u64)?;
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub fn log_tail(path: &Path, max_bytes: usize) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len() as usize;
    if len > max_bytes {
        file.seek_end_minus(max_bytes)?;
    }
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(buf)
}

trait SeekEndMinus: Read {
    fn seek_end_minus(&mut self, n: usize) -> io::Result<()>;
}

impl SeekEndMinus for File {
    fn seek_end_minus(&mut self, n: usize) -> io::Result<()> {
        use std::io::Seek;
        let len = self.metadata()?.len();
        let pos = len.saturating_sub(n as u64);
        self.seek(io::SeekFrom::Start(pos))?;
        Ok(())
    }
}

fn write_store_zip(path: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    // ponytail: store-only ZIP; switch to the zip crate if compression is required.
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut offset = 0u32;
    for (name, data) in files {
        let name_bytes = name.as_bytes();
        let local_off = offset;
        write_u32(&mut out, 0x04034b50);
        write_u16(&mut out, 20);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, crc32(data));
        write_u32(&mut out, data.len() as u32);
        write_u32(&mut out, data.len() as u32);
        write_u16(&mut out, name_bytes.len() as u16);
        write_u16(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);
        offset = out.len() as u32;

        write_u32(&mut central, 0x02014b50);
        write_u16(&mut central, 20);
        write_u16(&mut central, 20);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, crc32(data));
        write_u32(&mut central, data.len() as u32);
        write_u32(&mut central, data.len() as u32);
        write_u16(&mut central, name_bytes.len() as u16);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, 0);
        write_u32(&mut central, local_off);
        central.extend_from_slice(name_bytes);
    }
    let central_off = out.len() as u32;
    out.extend_from_slice(&central);
    write_u32(&mut out, 0x06054b50);
    write_u16(&mut out, 0);
    write_u16(&mut out, 0);
    write_u16(&mut out, files.len() as u16);
    write_u16(&mut out, files.len() as u16);
    write_u32(&mut out, central.len() as u32);
    write_u32(&mut out, central_off);
    write_u16(&mut out, 0);
    fs::write(path, out)
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xEDB88320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

pub fn contains_sentinel(bytes: &[u8]) -> bool {
    bytes
        .windows(SECRET_SENTINEL.len())
        .any(|window| window == SECRET_SENTINEL.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticBundle, OpaqueSecret, SECRET_SENTINEL, SizeRotatingWriter, contains_sentinel,
        redact_text,
    };
    use std::io::Write;

    #[test]
    fn redacts_connection_and_password() {
        let text =
            format!("password={SECRET_SENTINEL} postgres://user:{SECRET_SENTINEL}@localhost/db");
        let redacted = redact_text(&text);
        assert!(!redacted.contains(SECRET_SENTINEL));
        assert!(!format!("{:?}", OpaqueSecret::new(SECRET_SENTINEL)).contains(SECRET_SENTINEL));
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn rotating_writer_keeps_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = SizeRotatingWriter::open(dir.path(), "dexo", 16, 3).unwrap();
        for _ in 0..20 {
            writer.write_all(b"0123456789abcdef").unwrap();
        }
        writer.flush().unwrap();
        let count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .ok()
                    .and_then(|e| e.path().extension().map(|ext| ext == "log"))
                    .unwrap_or(false)
            })
            .count();
        assert!(count <= 4, "count={count}");
    }

    #[test]
    fn zip_bundle_has_no_sentinel_and_no_upload() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = DiagnosticBundle::assemble(
            "dexo 0.1.0".into(),
            "depth=none unicode=false".into(),
            format!("password={SECRET_SENTINEL}\ntheme=\"dark\"\n"),
            format!("connected password={SECRET_SENTINEL}\n"),
        );
        assert!(!bundle.preview.contains(SECRET_SENTINEL));
        assert!(bundle.preview.contains("versions:"));
        let zip_path = dir.path().join("diag.zip");
        bundle.write_zip(&zip_path).unwrap();
        let bytes = std::fs::read(&zip_path).unwrap();
        assert!(!contains_sentinel(&bytes));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("never uploads"));
        assert!(text.contains("versions.txt"));
    }
}
