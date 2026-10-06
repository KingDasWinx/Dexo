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
