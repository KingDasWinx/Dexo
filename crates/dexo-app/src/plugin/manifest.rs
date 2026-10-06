//! `dexo-plugin.toml`: what a plugin is and what it provides, read without running any
//! of it -- so the connection form can list a driver that is not installed.

use std::path::{Path, PathBuf};

use dexo_driver_api::DriverDescriptor;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Driver,
    Theme,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Driver => "driver",
            Kind::Theme => "theme",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub kind: Kind,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: String,
    pub min_dexo: String,
    #[serde(default)]
    pub driver: Option<DriverSection>,
    #[serde(default)]
    pub theme: Option<ThemeSection>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSection {
    pub protocol: u32,
    pub drivers: Vec<String>,
    pub display_name: String,
    #[serde(default)]
    pub dialect: Option<String>,
    #[serde(default)]
    pub file: bool,
    #[serde(default)]
    pub url_schemes: Vec<String>,
    pub executable: String,
}

/// What a driver plugin provides, as the index lists it. Unknown keys are allowed here:
/// a newer index may say more than this Dexo reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DriverInfo {
    pub protocol: u32,
    pub drivers: Vec<String>,
    pub display_name: String,
    #[serde(default)]
    pub dialect: Option<String>,
    #[serde(default)]
    pub file: bool,
    #[serde(default)]
    pub url_schemes: Vec<String>,
}

impl DriverSection {
    pub fn info(&self) -> DriverInfo {
        DriverInfo {
            protocol: self.protocol,
            drivers: self.drivers.clone(),
            display_name: self.display_name.clone(),
            dialect: self.dialect.clone(),
            file: self.file,
            url_schemes: self.url_schemes.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeSection {
    pub themes: Vec<String>,
}

/// Reads and checks a manifest. Strict: an unknown key is an error, so a typo does not
/// pass for a setting.
pub fn parse(text: &str) -> Result<Manifest, String> {
    let manifest: Manifest =
        toml::from_str(text).map_err(|error| format!("dexo-plugin.toml: {}", error.message()))?;
    manifest.validate()?;
    Ok(manifest)
}

/// The manifest of the plugin in `dir`, when it reads.
pub fn read(dir: &Path) -> Option<Manifest> {
    parse(&std::fs::read_to_string(dir.join("dexo-plugin.toml")).ok()?).ok()
}

/// `^[a-z0-9][a-z0-9-]{1,31}$`: an id names a folder, so nothing in it can climb out.
pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=32).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        let fail = |what: String| Err(format!("dexo-plugin.toml: {what}"));
        if self.schema != 1 {
            return fail(format!(
                "schema {} is newer than this Dexo reads",
                self.schema
            ));
        }
        if !valid_id(&self.id) {
            return fail(format!(
                "'{}' is not a plugin id: 2 to 32 lowercase letters, digits and '-'",
                self.id
            ));
        }
        for (field, version) in [("version", &self.version), ("min_dexo", &self.min_dexo)] {
            if !crate::update::is_version(version) {
                return fail(format!("{field} '{version}' is not a version like 1.2.3"));
            }
        }
        match (self.kind, &self.driver, &self.theme) {
            (Kind::Driver, Some(driver), None) => {
                if driver.drivers.is_empty() || !driver.drivers.iter().all(|id| valid_id(id)) {
                    return fail("[driver] drivers must name at least one driver id".into());
                }
                let executable = &driver.executable;
                if !executable.starts_with("bin/")
                    || executable.matches('/').count() != 1
                    || !super::archive::safe_relative(executable)
                {
                    return fail("[driver] executable must be a file directly under bin/".into());
                }
                Ok(())
            }
            (Kind::Theme, None, Some(theme)) => {
                if theme.themes.is_empty()
                    || !theme
                        .themes
                        .iter()
                        .all(|path| path.starts_with("themes/") && path.ends_with(".toml"))
                {
                    return fail(
                        "[theme] themes must be files under themes/ ending in .toml".into(),
                    );
                }
                Ok(())
            }
            (Kind::Driver, ..) => {
                fail("a driver plugin has a [driver] section and no [theme] section".into())
            }
            (Kind::Theme, ..) => {
                fail("a theme plugin has a [theme] section and no [driver] section".into())
            }
        }
    }

    /// The executable in the plugin's folder `dir`, `.exe` added on Windows.
    pub fn executable(&self, dir: &Path) -> Option<PathBuf> {
        self.driver.as_ref().map(|driver| {
            dir.join(format!(
                "{}{}",
                driver.executable,
                std::env::consts::EXE_SUFFIX
            ))
        })
    }

    /// The drivers this plugin brings, as Dexo lists them once it is installed.
    pub fn descriptors(&self) -> Vec<DriverDescriptor> {
        self.driver
            .iter()
            .flat_map(|driver| {
                driver.drivers.iter().map(|id| {
                    DriverDescriptor::plugin_driver(
                        id.clone(),
                        driver.display_name.clone(),
                        driver.file,
                        self.id.clone(),
                        true,
                    )
                })
            })
            .collect()
    }
}

/// What an installed plugin in `dir` provides: its driver ids, or its themes' names.
pub fn provides(manifest: &Manifest, dir: &Path) -> Vec<String> {
    if let Some(driver) = &manifest.driver {
        return driver.drivers.clone();
    }
    let mut names: Vec<String> = std::fs::read_dir(dir.join("themes"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Kind, parse};

    const DRIVER: &str = r#"
schema = 1
id = "duckdb"
kind = "driver"
version = "1.0.0"
name = "DuckDB"
description = "Open DuckDB files, CSV, Parquet and JSON"
authors = ["KingDasWinx"]
license = "MIT OR Apache-2.0"
homepage = "https://github.com/KingDasWinx/Dexo"
min_dexo = "1.5.0"

[driver]
protocol = 1
drivers = ["duckdb"]
display_name = "DuckDB"
dialect = "duckdb"
file = true
url_schemes = ["duckdb"]
executable = "bin/dexo-driver-duckdb"
"#;

    const THEME: &str = r#"
schema = 1
id = "extra-themes"
kind = "theme"
version = "1.0.0"
name = "Extra themes"
min_dexo = "1.5.0"

[theme]
themes = ["themes/*.toml"]
"#;

    #[test]
    fn a_driver_and_a_theme_pack_parse() {
        let driver = parse(DRIVER).unwrap();
        assert_eq!((driver.id.as_str(), driver.kind), ("duckdb", Kind::Driver));
        let descriptors = driver.descriptors();
        assert_eq!(descriptors.len(), 1);
        assert!(descriptors[0].file && descriptors[0].installed);
        assert_eq!(descriptors[0].plugin.as_deref(), Some("duckdb"));
        let exe = driver.executable(Path::new("plugins/duckdb")).unwrap();
        assert!(exe.ends_with(format!(
            "dexo-driver-duckdb{}",
            std::env::consts::EXE_SUFFIX
        )));
        assert_eq!(parse(THEME).unwrap().kind, Kind::Theme);
    }

    #[test]
    fn a_manifest_not_exactly_right_is_refused() {
        let cases = [
            (
                DRIVER.replace("schema = 1", "schema = 1\nsurprise = true"),
                "unknown field",
            ),
            (
                DRIVER.replace("id = \"duckdb\"", "id = \"../evil\""),
                "is not a plugin id",
            ),
            (
                DRIVER.replace("id = \"duckdb\"", "id = \"D\""),
                "is not a plugin id",
            ),
            (
                DRIVER.replace("version = \"1.0.0\"", "version = \"one\""),
                "version 'one'",
            ),
            (
                DRIVER.replace("min_dexo = \"1.5.0\"", "min_dexo = \"1.5\""),
                "min_dexo '1.5'",
            ),
            (DRIVER.replace("schema = 1", "schema = 2"), "schema 2"),
            (
                format!("{DRIVER}\n[theme]\nthemes = [\"themes/a.toml\"]\n"),
                "no [theme] section",
            ),
            (
                THEME.replace("kind = \"theme\"", "kind = \"driver\""),
                "a driver plugin has a [driver] section",
            ),
            (
                DRIVER.replace("bin/dexo-driver-duckdb", "../dexo-driver-duckdb"),
                "executable must be a file directly under bin/",
            ),
            (
                THEME.replace("themes/*.toml", "colors/a.toml"),
                "themes must be files under themes/",
            ),
        ];
        for (text, expected) in cases {
            let error = parse(&text).unwrap_err();
            assert!(error.contains(expected), "{expected}: {error}");
        }
    }
}
