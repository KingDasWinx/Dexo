//! Whether a newer Dexo is out, and how the running copy should get it.
//!
//! GitHub answers `github.com/<repo>/releases/latest` with a redirect to the newest
//! stable tag, so one HEAD request and one header are the whole protocol: no API
//! token, no JSON, no rate limit. The network half lives with the TUI runtime; this
//! module is the part that can be tested without one.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const HOST: &str = "github.com";
pub const LATEST_PATH: &str = "/KingDasWinx/Dexo/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/KingDasWinx/Dexo/releases/latest";
/// A day between checks: often enough to hear about a release, rare enough that
/// starting Dexo ten times a day costs one request.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const CACHE_FILE: &str = "update-check.json";

pub fn head_request(current_version: &str) -> String {
    format!(
        "HEAD {LATEST_PATH} HTTP/1.1\r\nHost: {HOST}\r\nUser-Agent: dexo/{current_version}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    )
}

/// The version a `releases/latest` response redirects to, or `None` when it did not
/// redirect to a release tag (no release yet, an error page, a changed URL scheme).
pub fn latest_from_response(response: &str) -> Option<String> {
    let mut lines = response.split("\r\n");
    let status = lines.next()?.split_whitespace().nth(1)?;
    if !matches!(status, "301" | "302" | "303" | "307" | "308") {
        return None;
    }
    let location = lines.take_while(|line| !line.is_empty()).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("location")
            .then(|| value.trim())
    })?;
    version_from_location(location)
}

/// `https://github.com/KingDasWinx/Dexo/releases/tag/v1.3.0` is `1.3.0`.
pub fn version_from_location(location: &str) -> Option<String> {
    let tag = location.trim().rsplit_once("/releases/tag/")?.1;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    parse(version).map(|_| version.to_string())
}

/// Semver precedence: the numeric core decides, and a release outranks its own
/// prereleases. Anything unparsable is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => match a.0.cmp(&b.0) {
            std::cmp::Ordering::Equal => match (a.1, b.1) {
                (None, Some(_)) => true,
                (Some(pa), Some(pb)) => pa > pb,
                _ => false,
            },
            order => order.is_gt(),
        },
        _ => false,
    }
}

/// The numeric core and the prerelease suffix, if any.
type Parsed<'a> = ((u64, u64, u64), Option<&'a str>);

fn parse(version: &str) -> Option<Parsed<'_>> {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some((triple, pre))
}

/// How this copy of Dexo was installed, read off where its binary lives. Each
/// channel updates its own way, and telling a Homebrew user to rerun an installer
/// would leave two copies on the PATH.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMethod {
    Homebrew,
    Scoop,
    Winget,
    /// The shell or PowerShell installer, which puts `dexo-update` next to `dexo`.
    Installer,
    Cargo,
    /// A `.deb`, `.rpm`, `.msi`, or a downloaded archive: nothing to run, only a
    /// newer file to fetch.
    Download,
}

/// `exe` should be the resolved path (Homebrew's `/usr/local/bin/dexo` is a symlink
/// into the Cellar). `has_updater` says whether `dexo-update` sits beside it.
pub fn detect_install(exe: &Path, has_updater: bool) -> InstallMethod {
    let path = exe
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if ["/cellar/", "/homebrew/", "/linuxbrew/"]
        .iter()
        .any(|marker| path.contains(marker))
    {
        InstallMethod::Homebrew
    } else if path.contains("/scoop/") {
        InstallMethod::Scoop
    } else if path.contains("/winget/") {
        InstallMethod::Winget
    } else if has_updater {
        InstallMethod::Installer
    } else if path.contains("/.cargo/bin/") {
        InstallMethod::Cargo
    } else {
        InstallMethod::Download
    }
}

pub fn update_command(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::Homebrew => "brew upgrade dexo",
        InstallMethod::Scoop => "scoop update dexo",
        InstallMethod::Winget => "winget upgrade KingDasWinx.Dexo",
        InstallMethod::Installer => "dexo-update",
        InstallMethod::Cargo => {
            "cargo install --locked --git https://github.com/kingdaswinx/Dexo dexo"
        }
        InstallMethod::Download => RELEASES_URL,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCache {
    /// Unix seconds of the last answer from GitHub.
    pub checked_at: u64,
    pub latest: String,
}

impl UpdateCache {
    pub fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.checked_at) < CHECK_INTERVAL_SECS
    }
}

pub fn load_cache(data_dir: &Path) -> Option<UpdateCache> {
    let text = std::fs::read_to_string(data_dir.join(CACHE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_cache(data_dir: &Path, cache: &UpdateCache) -> std::io::Result<()> {
    let text = serde_json::to_string(cache).map_err(std::io::Error::other)?;
    std::fs::write(data_dir.join(CACHE_FILE), text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_redirect_to_a_tag_names_the_latest_version() {
        let response = "HTTP/1.1 302 Found\r\nServer: GitHub.com\r\nlocation: https://github.com/KingDasWinx/Dexo/releases/tag/v1.3.0\r\ncontent-length: 0\r\n\r\n";
        assert_eq!(latest_from_response(response).as_deref(), Some("1.3.0"));
    }

    #[test]
    fn anything_but_a_redirect_to_a_tag_is_no_answer() {
        assert_eq!(latest_from_response("HTTP/1.1 200 OK\r\n\r\n"), None);
        assert_eq!(
            latest_from_response(
                "HTTP/1.1 302 Found\r\nLocation: https://github.com/login\r\n\r\n"
            ),
            None
        );
        assert_eq!(latest_from_response(""), None);
        assert_eq!(
            version_from_location("https://github.com/x/y/releases/tag/nightly"),
            None
        );
    }

    #[test]
    fn versions_compare_by_semver_precedence() {
        assert!(is_newer("1.3.0", "1.2.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("2.0.0", "1.99.0"));
        assert!(is_newer("1.2.0", "1.2.0-rc.1"));
        assert!(!is_newer("1.2.0", "1.2.0"));
        assert!(!is_newer("1.2.0-rc.1", "1.2.0"));
        assert!(!is_newer("1.1.9", "1.2.0"));
        assert!(!is_newer("banana", "1.2.0"));
        assert!(!is_newer("1.2", "1.1.0"));
    }

    #[test]
    fn the_binary_path_names_the_channel_that_updates_it() {
        let cases = [
            (
                "/opt/homebrew/Cellar/dexo/1.2.0/bin/dexo",
                false,
                InstallMethod::Homebrew,
            ),
            (
                "/usr/local/Cellar/dexo/1.2.0/bin/dexo",
                false,
                InstallMethod::Homebrew,
            ),
            (
                "/home/linuxbrew/.linuxbrew/Cellar/dexo/1.2.0/bin/dexo",
                false,
                InstallMethod::Homebrew,
            ),
            (
                r"C:\Users\ana\scoop\apps\dexo\current\dexo.exe",
                false,
                InstallMethod::Scoop,
            ),
            (
                r"C:\Users\ana\AppData\Local\Microsoft\WinGet\Packages\KingDasWinx.Dexo_Microsoft.Winget.Source_8wekyb3d8bbwe\dexo.exe",
                false,
                InstallMethod::Winget,
            ),
            ("/home/ana/.cargo/bin/dexo", true, InstallMethod::Installer),
            ("/home/ana/.cargo/bin/dexo", false, InstallMethod::Cargo),
            ("/usr/bin/dexo", false, InstallMethod::Download),
            (
                r"C:\Program Files\Dexo\bin\dexo.exe",
                false,
                InstallMethod::Download,
            ),
        ];
        for (path, has_updater, method) in cases {
            assert_eq!(
                detect_install(&PathBuf::from(path), has_updater),
                method,
                "{path}"
            );
        }
        assert_eq!(update_command(InstallMethod::Homebrew), "brew upgrade dexo");
    }

    #[test]
    fn the_cache_answers_for_a_day() {
        let cache = UpdateCache {
            checked_at: 1_000,
            latest: "1.3.0".into(),
        };
        assert!(cache.is_fresh(1_000 + CHECK_INTERVAL_SECS - 1));
        assert!(!cache.is_fresh(1_000 + CHECK_INTERVAL_SECS));

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_cache(dir.path()), None);
        save_cache(dir.path(), &cache).unwrap();
        assert_eq!(load_cache(dir.path()), Some(cache));
    }
}
