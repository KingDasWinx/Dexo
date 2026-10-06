//! Plugin packages, unpacked: nothing let out of the plugin's folder, and only the
//! files a plugin may ship let in.

use std::io::Read;
use std::path::{Component, Path};

use super::manifest::{Kind, Manifest};

/// More than this unpacked is a compression bomb, not a plugin.
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

/// Unpacks a `.tar.gz` into `into`. Refused: a symlink, a hard link, an absolute path, a
/// `..`, and any file a plugin does not ship.
pub fn unpack(package: impl Read, into: &Path) -> Result<(), String> {
    unpack_within(package, into, MAX_UNPACKED)
}

/// A stream that fails past `left` bytes: what `tar` reads whole into memory -- a long
/// name, an extended header -- is capped with the rest.
struct Capped<R> {
    inner: R,
    left: u64,
    limit: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read as u64 > self.left {
            return Err(std::io::Error::other(format!(
                "the package unpacks to more than {} MB",
                self.limit / (1024 * 1024)
            )));
        }
        self.left -= read as u64;
        Ok(read)
    }
}

fn unpack_within(package: impl Read, into: &Path, limit: u64) -> Result<(), String> {
    let unreadable = |error: std::io::Error| format!("the package cannot be read: {error}");
    let mut archive = tar::Archive::new(Capped {
        inner: flate2::read::GzDecoder::new(package),
        left: limit,
        limit,
    });
    let mut total = 0u64;
    for entry in archive.entries().map_err(unreadable)? {
        let mut entry = entry.map_err(unreadable)?;
        let name = entry
            .path()
            .map_err(unreadable)?
            .to_string_lossy()
            .replace('\\', "/");
        let name = name
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        if name.is_empty() || name == "." {
            continue;
        }
        if !safe_relative(&name) {
            return Err(format!(
                "the package holds '{name}', outside the plugin's folder"
            ));
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if !matches!(name.as_str(), "bin" | "themes") {
                return Err(format!(
                    "the package holds a folder '{name}', which a plugin does not ship"
                ));
            }
            std::fs::create_dir_all(into.join(&name)).map_err(|error| error.to_string())?;
            continue;
        }
        if !kind.is_file() {
            return Err(format!(
                "the package holds '{name}', which is not a plain file"
            ));
        }
        if !allowed(&name) {
            return Err(format!(
                "the package holds '{name}', which a plugin does not ship"
            ));
        }
        total += entry.header().size().unwrap_or(0);
        if total > limit {
            return Err(format!(
                "the package unpacks to more than {} MB",
                limit / (1024 * 1024)
            ));
        }
        let target = into.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file = std::fs::File::create(&target)
            .map_err(|error| format!("{}: {error}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| format!("{}: {error}", target.display()))?;
        #[cfg(unix)]
        if name.starts_with("bin/") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// The files a plugin ships: its manifest, licence and readme, and a program directly
/// under `bin/` or themes directly under `themes/`.
pub(crate) fn allowed(name: &str) -> bool {
    matches!(name, "dexo-plugin.toml" | "LICENSE" | "README.md")
        || (name.starts_with("bin/") && name.matches('/').count() == 1)
        || (name.starts_with("themes/")
            && name.ends_with(".toml")
            && name.matches('/').count() == 1)
}

/// A relative path with no `..`, no root, no drive and no stream: nothing in it climbs
/// out of the folder it is joined to, on any system.
pub fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(':')
        && !name.contains('\\')
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// What a package holds against its manifest: a driver's executable is there and it
/// ships no themes; a theme pack ships themes and no program.
pub fn check_kind(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    match manifest.kind {
        Kind::Driver => {
            if dir.join("themes").exists() {
                return Err("a driver plugin ships no themes".into());
            }
            let executable = manifest.executable(dir).unwrap_or_default();
            if !executable.is_file() {
                let shown = executable.strip_prefix(dir).unwrap_or(&executable);
                return Err(format!("the package has no {}", shown.display()));
            }
        }
        Kind::Theme => {
            if dir.join("bin").exists() {
                return Err("a theme plugin ships no programs".into());
            }
            let themes = std::fs::read_dir(dir.join("themes"))
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.path().extension().is_some_and(|x| x == "toml"))
                        .count()
                })
                .unwrap_or(0);
            if themes == 0 {
                return Err("the theme pack has no themes".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unpack;

    const FILE: tar::EntryType = tar::EntryType::Regular;

    /// A `.tar.gz` of `entries`, their names written raw as a hostile package's would be:
    /// `tar`'s own `set_path` refuses `..` and absolute paths.
    fn package(entries: &[(&str, tar::EntryType)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, kind) in entries {
            let mut header = tar::Header::new_gnu();
            header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_entry_type(*kind);
            header.set_size(1);
            header.set_mode(0o644);
            if matches!(kind, tar::EntryType::Symlink | tar::EntryType::Link) {
                header.set_link_name("dexo-plugin.toml").unwrap();
                header.set_size(0);
            }
            header.set_cksum();
            let data: &[u8] = if header.size().unwrap() == 1 {
                b"x"
            } else {
                b""
            };
            builder.append(&header, data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn a_plugin_unpacks_with_a_runnable_executable() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = package(&[
            ("dexo-plugin.toml", FILE),
            ("README.md", FILE),
            ("LICENSE", FILE),
            ("bin/tool", FILE),
            ("themes/a.toml", FILE),
        ]);
        unpack(&bytes[..], dir.path()).unwrap();
        assert!(dir.path().join("themes/a.toml").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("bin/tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn a_package_that_unpacks_past_the_cap_stops_at_it() {
        // A long name is read whole into memory: the stream is capped, not only the files.
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..13].copy_from_slice(b"././@LongLink");
        header.set_entry_type(tar::EntryType::GNULongName);
        header.set_size(64 * 1024);
        header.set_cksum();
        builder.append(&header, &vec![b'a'; 64 * 1024][..]).unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let error = super::unpack_within(&bytes[..], dir.path(), 16 * 1024).unwrap_err();
        assert!(error.contains("unpacks to more than"), "{error}");
    }

    #[test]
    fn what_could_escape_or_surprise_is_refused() {
        for (name, kind) in [
            ("../evil", FILE),
            ("/etc/evil", FILE),
            ("bin/../../evil", FILE),
            ("C:evil", FILE),
            ("src/lib.rs", FILE),
            ("bin/sub/tool", FILE),
            ("themes/a.txt", FILE),
            ("bin/link", tar::EntryType::Symlink),
            ("bin/hard", tar::EntryType::Link),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let bytes = package(&[(name, kind)]);
            assert!(
                unpack(&bytes[..], dir.path()).is_err(),
                "{name} was let through"
            );
        }
    }
}
