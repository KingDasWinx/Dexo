//! Plugin packages for tests, built on the spot: theme packs, driver plugins around a
//! test executable, and the `.tar.gz` of either.

use std::path::{Path, PathBuf};

/// A theme, in the format Dexo reads.
pub const THEME: &str = "name = \"Ocean\"\nmode = \"dark\"\n[roles]\nfocus = \"#00aaff\"\n";

/// A manifest: a theme pack, or a driver plugin when `executable` names its program.
pub fn manifest(id: &str, version: &str, min_dexo: &str, executable: Option<&str>) -> String {
    let (kind, section) = match executable {
        Some(executable) => (
            "driver",
            format!(
                "[driver]\nprotocol = 1\ndrivers = [\"{id}\"]\ndisplay_name = \"Testbin\"\nfile = true\nexecutable = \"bin/{executable}\"\n"
            ),
        ),
        None => (
            "theme",
            "[theme]\nthemes = [\"themes/*.toml\"]\n".to_string(),
        ),
    };
    format!(
        "schema = 1\nid = \"{id}\"\nkind = \"{kind}\"\nversion = \"{version}\"\nname = \"{id}\"\nauthors = [\"Tests\"]\nmin_dexo = \"{min_dexo}\"\n\n{section}"
    )
}

/// A theme pack's folder in `dir`, with one theme, `ocean`.
pub fn theme_folder(dir: &Path, id: &str, version: &str) -> PathBuf {
    let root = dir.join(format!("{id}-{version}"));
    std::fs::create_dir_all(root.join("themes")).unwrap();
    std::fs::write(
        root.join("dexo-plugin.toml"),
        manifest(id, version, "1.0.0", None),
    )
    .unwrap();
    std::fs::write(root.join("themes/ocean.toml"), THEME).unwrap();
    root
}

/// A driver plugin's folder in `dir`: its program a copy of `executable`, named `name`
/// -- the test plugin picks how it behaves by the name it runs under.
pub fn driver_folder(
    dir: &Path,
    id: &str,
    version: &str,
    executable: &Path,
    name: &str,
) -> PathBuf {
    let root = dir.join(format!("{id}-{version}-{name}"));
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(
        root.join("dexo-plugin.toml"),
        manifest(id, version, "1.0.0", Some(name)),
    )
    .unwrap();
    std::fs::copy(
        executable,
        root.join("bin")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    root
}

/// `folder` as a plugin's `.tar.gz`.
pub fn pack(folder: &Path) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for file in files(folder) {
        let name = file
            .strip_prefix(folder)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        builder.append_path_with_name(&file, name).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

/// A minisign key pair made for one test, signing as the maintainer's key signs the index.
pub struct TestKey {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}

impl TestKey {
    pub fn new() -> Self {
        let minisign::KeyPair { pk, sk } =
            minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Self { pk, sk }
    }

    /// The public key, as `DEXO_PLUGIN_TRUSTED_KEY` takes it.
    pub fn public(&self) -> String {
        self.pk.to_base64()
    }

    /// `data`'s `.minisig`.
    pub fn sign(&self, data: &[u8]) -> String {
        minisign::sign(
            Some(&self.pk),
            &self.sk,
            std::io::Cursor::new(data),
            None,
            None,
        )
        .unwrap()
        .into_string()
    }
}

impl Default for TestKey {
    fn default() -> Self {
        Self::new()
    }
}

/// A plain HTTP server on localhost: GET of a path answers its bytes, any other 404.
/// It runs until the test process ends.
pub struct HttpServer {
    base: String,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl HttpServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let served = files.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let served = served.clone();
                std::thread::spawn(move || answer(stream, &served));
            }
        });
        Self { base, files }
    }

    pub fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.files.lock().unwrap().insert(path.into(), body.into());
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// What `path` serves now.
    pub fn bytes(&self, path: &str) -> Vec<u8> {
        self.files.lock().unwrap()[path].clone()
    }

    /// A URL nothing answers at: a port bound and let go.
    pub fn dead_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}/index.json")
    }
}

fn answer(mut stream: TcpStream, files: &Mutex<HashMap<String, Vec<u8>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) <= 2 {
            break;
        }
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
    let body = files.lock().unwrap().get(&path).cloned();
    let _ = match body {
        Some(body) => write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .and_then(|()| stream.write_all(&body)),
        None => stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    };
}

/// One version of a plugin as the index lists it, its artifact `bytes` at `url`; for a
/// driver plugin, `driver` names the driver it provides.
pub fn version_entry(
    version: &str,
    min_dexo: &str,
    target: &str,
    url: &str,
    bytes: &[u8],
    driver: Option<&str>,
) -> serde_json::Value {
    let sha256: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut artifacts = serde_json::Map::new();
    artifacts.insert(
        target.to_string(),
        serde_json::json!({"url": url, "sha256": sha256, "size": bytes.len()}),
    );
    let mut entry =
        serde_json::json!({"version": version, "min_dexo": min_dexo, "artifacts": artifacts});
    if let Some(driver) = driver {
        entry["driver"] = serde_json::json!({"protocol": 1, "drivers": [driver], "display_name": "Testbin", "file": true});
    }
    entry
}

/// An index of `plugins`, each `(id, kind, versions)`.
pub fn index_json(plugins: &[(&str, &str, Vec<serde_json::Value>)]) -> String {
    let plugins: Vec<serde_json::Value> = plugins
        .iter()
        .map(|(id, kind, versions)| {
            serde_json::json!({"id": id, "kind": kind, "name": id, "description": format!("The {id} plugin"),
                "authors": ["Tests"], "license": "MIT", "homepage": "", "versions": versions})
        })
        .collect();
    serde_json::json!({"schema": 1, "generated": "2026-10-06T00:00:00Z", "plugins": plugins})
        .to_string()
}
