//! The plugin index: one JSON file and its minisign signature, published on a fixed
//! GitHub release. It is trusted only when one of Dexo's keys signed it, and every
//! artifact's sha256 comes from it -- so a download is authenticated, not only checked.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::UNIX_EPOCH;

use dexo_driver_api::DriverDescriptor;
use dexo_plugin_protocol::PROTOCOL;
use serde::{Deserialize, Serialize};

use super::manifest::{DriverInfo, Kind};
use super::state::{Origin, State};
use super::{http, install, keys, plugins_dir};
use crate::error::{AppError, ErrorCategory};

pub const DEFAULT_INDEX: &str =
    "https://github.com/KingDasWinx/Dexo/releases/download/plugins-index/index.json";
const CACHE: &str = "index-cache.json";
/// A day: the index is fetched again after it.
const FRESH: u64 = 24 * 60 * 60;
const INDEX_LIMIT: u64 = 8 * 1024 * 1024;

/// Where the index comes from and which keys may sign it.
#[derive(Clone, Debug)]
pub struct IndexSource {
    pub url: String,
    pub keys: Vec<String>,
}

impl IndexSource {
    /// The official index and keys. `DEXO_PLUGIN_INDEX` and `DEXO_PLUGIN_TRUSTED_KEY` are
    /// for tests and mirrors; a mirror's index still has to be signed.
    pub fn from_env() -> Self {
        let url = std::env::var("DEXO_PLUGIN_INDEX")
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| DEFAULT_INDEX.into());
        let mut keys: Vec<String> = keys::TRUSTED.iter().map(|key| key.to_string()).collect();
        if let Ok(extra) = std::env::var("DEXO_PLUGIN_TRUSTED_KEY")
            && !extra.trim().is_empty()
        {
            keys.push(extra.trim().to_string());
        }
        Self { url, keys }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    #[serde(default)]
    pub generated: String,
    pub plugins: Vec<IndexPlugin>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexPlugin {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: String,
    pub versions: Vec<IndexVersion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexVersion {
    pub version: String,
    pub min_dexo: String,
    #[serde(default)]
    pub driver: Option<DriverInfo>,
    pub artifacts: BTreeMap<String, Artifact>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// A verified index and when it was fetched; `offline` when the network failed and
/// this is the cache.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub index: Index,
    pub fetched_at: u64,
    pub offline: bool,
}

/// Whether one of `keys` signed `bytes`.
pub fn verify(bytes: &[u8], signature: &str, keys: &[String]) -> bool {
    let Ok(signature) = minisign_verify::Signature::decode(signature) else {
        return false;
    };
    keys.iter()
        .filter_map(|key| minisign_verify::PublicKey::from_base64(key).ok())
        .any(|key| key.verify(bytes, &signature, false).is_ok())
}

fn parse(bytes: &[u8]) -> Result<Index, String> {
    let index: Index = serde_json::from_slice(bytes)
        .map_err(|error| format!("the plugin index cannot be read: {error}"))?;
    if index.schema != 1 {
        return Err("the plugin index is newer than this Dexo reads: update Dexo".into());
    }
    Ok(index)
}

/// The index: the cache while it is under a day old (unless `refresh`), else fetched and
/// verified; when the network fails, the cache, said to be offline. A cache that does
/// not verify or read is as good as none.
pub fn load(data_dir: &Path, source: &IndexSource, refresh: bool) -> Result<Fetched, AppError> {
    let plugins = plugins_dir(data_dir);
    let cached = read_cache(&plugins, source);
    let now = install::now();
    if !refresh
        && let Some((index, at)) = &cached
        && now.saturating_sub(*at) < FRESH
    {
        return Ok(Fetched {
            index: index.clone(),
            fetched_at: *at,
            offline: false,
        });
    }
    let never = AtomicBool::new(false);
    let fetched = http::get(&source.url, INDEX_LIMIT, &mut |_, _| {}, &never).and_then(|bytes| {
        let signature = http::get(
            &format!("{}.minisig", source.url),
            64 * 1024,
            &mut |_, _| {},
            &never,
        )?;
        Ok((bytes, String::from_utf8_lossy(&signature).into_owned()))
    });
    match fetched {
        Ok((bytes, signature)) => {
            if !verify(&bytes, &signature, &source.keys) {
                return Err(AppError::new(
                    ErrorCategory::Permission,
                    "the index signature does not match: nothing was installed",
                ));
            }
            let index = parse(&bytes)
                .map_err(|message| AppError::new(ErrorCategory::Configuration, message))?;
            write_cache(&plugins, &bytes, &signature);
            Ok(Fetched {
                index,
                fetched_at: now,
                offline: false,
            })
        }
        Err(network) => match cached {
            Some((index, at)) => Ok(Fetched {
                index,
                fetched_at: at,
                offline: true,
            }),
            None => Err(AppError::new(
                ErrorCategory::Network,
                format!("the plugin index could not be fetched: {network}"),
            )),
        },
    }
}

fn read_cache(plugins: &Path, source: &IndexSource) -> Option<(Index, u64)> {
    let path = plugins.join(CACHE);
    let bytes = std::fs::read(&path).ok()?;
    let signature = std::fs::read_to_string(plugins.join(format!("{CACHE}.minisig"))).ok()?;
    if !verify(&bytes, &signature, &source.keys) {
        return None;
    }
    let at = std::fs::metadata(&path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((parse(&bytes).ok()?, at))
}

/// The signature first: an index not yet written next to it fails its check and is
/// fetched again.
fn write_cache(plugins: &Path, bytes: &[u8], signature: &str) {
    let write = |name: &str, data: &[u8]| -> std::io::Result<()> {
        let temporary = plugins.join(format!("{name}.{}", std::process::id()));
        std::fs::write(&temporary, data)?;
        std::fs::rename(&temporary, plugins.join(name))
    };
    if std::fs::create_dir_all(plugins).is_ok() {
        let _ = write(&format!("{CACHE}.minisig"), signature.as_bytes());
        let _ = write(CACHE, bytes);
    }
}

/// "offline: showing the index from 2026-10-05".
pub fn offline_note(fetched_at: u64) -> String {
    let date = chrono::DateTime::from_timestamp(i64::try_from(fetched_at).unwrap_or(0), 0)
        .map(|at| at.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    format!("offline: showing the index from {date}")
}

/// This platform as an artifact key; none where no plugin is built.
pub fn target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    })
}

/// What installing a plugin fetches: a version, and its artifact for this platform.
pub struct Choice<'a> {
    pub plugin: &'a IndexPlugin,
    pub version: &'a IndexVersion,
    pub artifact: &'a Artifact,
}

/// The newest version of `id` this Dexo runs -- `min_dexo` at most `dexo`, a protocol it
/// speaks, no prerelease unless asked for by name -- or `version` when given.
pub fn choose<'a>(
    index: &'a Index,
    id: &str,
    version: Option<&str>,
    dexo: &str,
    target: Option<&str>,
) -> Result<Choice<'a>, String> {
    let plugin = index
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or_else(|| format!("there is no plugin '{id}' in the index"))?;
    let candidates: Vec<&IndexVersion> = plugin
        .versions
        .iter()
        .filter(|entry| match version {
            Some(wanted) => entry.version == wanted,
            None => !entry.version.contains('-'),
        })
        .collect();
    if candidates.is_empty() {
        return Err(format!(
            "{} has no version {}",
            plugin.name,
            version.unwrap_or("to install")
        ));
    }
    let runs = |entry: &&IndexVersion| {
        !crate::update::is_newer(&entry.min_dexo, dexo)
            && entry
                .driver
                .as_ref()
                .is_none_or(|driver| driver.protocol == PROTOCOL)
    };
    let newest = candidates
        .iter()
        .copied()
        .filter(runs)
        .reduce(|best, entry| {
            if crate::update::is_newer(&entry.version, &best.version) {
                entry
            } else {
                best
            }
        });
    let Some(newest) = newest else {
        let needs = candidates
            .iter()
            .map(|entry| entry.min_dexo.as_str())
            .reduce(|a, b| if crate::update::is_newer(a, b) { b } else { a })
            .unwrap_or_default();
        return Err(format!(
            "no version of {} runs on Dexo {dexo}: it needs Dexo {needs} or newer",
            plugin.name
        ));
    };
    let artifact = target
        .and_then(|target| newest.artifacts.get(target))
        .or_else(|| newest.artifacts.get("any"))
        .ok_or_else(|| {
            format!(
                "{} {} has no build for {}",
                plugin.name,
                newest.version,
                target.unwrap_or(std::env::consts::OS)
            )
        })?;
    Ok(Choice {
        plugin,
        version: newest,
        artifact,
    })
}

/// The official plugins installed with a newer version this Dexo runs, as
/// `(id, installed, newest)`. A plugin from a file is updated from a file.
pub fn updates(index: &Index, state: &State, dexo: &str) -> Vec<(String, String, String)> {
    state
        .plugins
        .iter()
        .filter(|(_, installed)| installed.origin == Origin::Official)
        .filter_map(|(id, installed)| {
            let choice = choose(index, id, None, dexo, target()).ok()?;
            crate::update::is_newer(&choice.version.version, &installed.version).then(|| {
                (
                    id.clone(),
                    installed.version.clone(),
                    choice.version.version.clone(),
                )
            })
        })
        .collect()
}

/// Makes the index's drivers that are not installed known as such, so the connection
/// form lists them with an offer to install.
pub fn announce_available(index: &Index) {
    for plugin in &index.plugins {
        let Ok(choice) = choose(index, &plugin.id, None, env!("CARGO_PKG_VERSION"), target())
        else {
            continue;
        };
        let Some(driver) = &choice.version.driver else {
            continue;
        };
        for id in &driver.drivers {
            if !DriverDescriptor::may_provide(&plugin.id, id)
                || DriverDescriptor::for_id(id).is_some_and(|known| known.installed)
            {
                continue;
            }
            DriverDescriptor::announce(DriverDescriptor::plugin_driver(
                id.clone(),
                driver.display_name.clone(),
                driver.file,
                plugin.id.clone(),
                false,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Artifact, Index, IndexPlugin, IndexVersion, choose};
    use crate::plugin::manifest::{DriverInfo, Kind};

    const LINUX: &str = "x86_64-unknown-linux-gnu";

    fn version(version: &str, min_dexo: &str, protocol: u32) -> IndexVersion {
        IndexVersion {
            version: version.into(),
            min_dexo: min_dexo.into(),
            driver: Some(DriverInfo {
                protocol,
                drivers: vec!["duckdb".into()],
                display_name: "DuckDB".into(),
                dialect: None,
                file: true,
                url_schemes: vec![],
            }),
            artifacts: BTreeMap::from([(
                LINUX.to_string(),
                Artifact {
                    url: "https://example.invalid/a.tar.gz".into(),
                    sha256: "0".repeat(64),
                    size: 1,
                },
            )]),
        }
    }

    #[test]
    fn an_index_plugin_cannot_claim_a_driver_dexo_knows_as_another_plugins() {
        let mut lookalike = index(vec![version("1.0.0", "1.0.0", 1)]);
        lookalike.plugins[0].id = "lookalike".into();
        super::announce_available(&lookalike);
        let duckdb = dexo_driver_api::DriverDescriptor::for_id("duckdb").unwrap();
        assert_eq!(duckdb.plugin.as_deref(), Some("duckdb"));
    }

    fn index(versions: Vec<IndexVersion>) -> Index {
        Index {
            schema: 1,
            generated: String::new(),
            plugins: vec![IndexPlugin {
                id: "duckdb".into(),
                kind: Kind::Driver,
                name: "DuckDB".into(),
                description: String::new(),
                authors: vec![],
                license: String::new(),
                homepage: String::new(),
                versions,
            }],
        }
    }

    #[test]
    fn the_newest_version_this_dexo_runs_is_chosen() {
        let index = index(vec![
            version("1.0.0", "1.0.0", 1),
            version("1.1.0", "1.5.0", 1),
            version("1.2.0-rc.1", "1.0.0", 1),
            version("1.3.0", "1.0.0", 2),
            version("2.0.0", "9.0.0", 1),
        ]);
        let chosen = |dexo, wanted| {
            choose(&index, "duckdb", wanted, dexo, Some(LINUX))
                .map(|choice| choice.version.version.clone())
        };
        assert_eq!(chosen("1.5.0", None).unwrap(), "1.1.0");
        assert_eq!(chosen("1.4.2", None).unwrap(), "1.0.0");
        assert_eq!(chosen("1.5.0", Some("1.2.0-rc.1")).unwrap(), "1.2.0-rc.1");
        let too_new = chosen("1.5.0", Some("2.0.0")).unwrap_err();
        assert!(too_new.contains("needs Dexo 9.0.0 or newer"), "{too_new}");
        let elsewhere = choose(
            &index,
            "duckdb",
            None,
            "1.5.0",
            Some("aarch64-apple-darwin"),
        )
        .err()
        .unwrap();
        assert!(
            elsewhere.contains("has no build for aarch64-apple-darwin"),
            "{elsewhere}"
        );
        let unknown = choose(&index, "nope", None, "1.5.0", Some(LINUX))
            .err()
            .unwrap();
        assert!(unknown.contains("no plugin 'nope'"), "{unknown}");
    }
}
