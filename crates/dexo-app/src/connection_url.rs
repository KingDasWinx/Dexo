//! `dexo postgres://user:secret@host/db`: a connection described by a URL, opened
//! without being saved.

use secrecy::SecretString;
use uuid::Uuid;

use crate::connection_profile::{ConnectionId, ConnectionProfile, SecretRef};
use crate::error::{AppError, ErrorCategory};

/// A profile that lives only as long as the session that opened it, and the URL's
/// password, which is never written anywhere.
pub struct UrlConnection {
    pub profile: ConnectionProfile,
    pub password: Option<SecretString>,
    /// Something whoever opens the connection should be told, such as that the
    /// password was given where other users can read it.
    pub warning: Option<String>,
}

impl std::fmt::Debug for UrlConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UrlConnection")
            .field("profile", &self.profile.name)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

pub fn parse(url: &str) -> Result<UrlConnection, AppError> {
    let invalid = |reason: &str| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("not a connection URL: {reason}"),
        )
    };
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| invalid("expected scheme://, such as postgres://user@host/db"))?;
    // A fragment is for the client that wrote the URL; it names nothing here.
    let rest = rest.split_once('#').map_or(rest, |(rest, _)| rest);
    let driver = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => "postgres",
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "sqlite" => return file_connection("sqlite", rest),
        "duckdb" => {
            return Err(AppError::new(
                ErrorCategory::Capability,
                "DuckDB connections arrive with the DuckDB driver",
            ));
        }
        other => return Err(invalid(&format!("unknown scheme {other}"))),
    };
    let (main, query) = rest.split_once('?').unwrap_or((rest, ""));
    let parameters = Parameters::read(query, driver).map_err(|reason| invalid(&reason))?;
    let (authority, database) = main.split_once('/').unwrap_or((main, ""));
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(userinfo) => match userinfo.split_once(':') {
            Some((user, password)) => (user, Some(password)),
            None => (userinfo, None),
        },
        None => ("", None),
    };
    let user = decode(user).map_err(|_| invalid("the user is not valid percent-encoding"))?;
    if user.is_empty() {
        return Err(invalid(
            "the URL needs a user, such as postgres://user@host/db",
        ));
    }
    let password = password
        .map(decode)
        .transpose()
        .map_err(|_| invalid("the password is not valid percent-encoding"))?
        .filter(|password| !password.is_empty())
        .map(SecretString::from);
    let database =
        decode(database).map_err(|_| invalid("the database is not valid percent-encoding"))?;
    let (host, port) =
        split_host_port(hostport).ok_or_else(|| invalid("the port is not a number"))?;
    // `%2Fvar%2Frun%2Fpostgresql` is a socket directory; `?host=` says the same.
    let host = decode(&host).map_err(|_| invalid("the host is not valid percent-encoding"))?;
    let host = match parameters.host {
        Some(host) => host,
        None if host.is_empty() => "localhost".to_string(),
        None => host,
    };
    let mut config = serde_json::json!({
        "host": host,
        "username": user,
        "database": database,
    });
    if let Some(port) = port {
        config["port"] = serde_json::json!(port);
    }
    if parameters.tls.is_some() || !parameters.tls_files.is_empty() {
        let mut tls = serde_json::json!({ "mode": parameters.tls.unwrap_or("verify_full") });
        for (key, path) in parameters.tls_files {
            tls[key] = serde_json::json!(path);
        }
        config["tls"] = tls;
    }
    let name = if database.is_empty() {
        format!("{user}@{host}")
    } else {
        format!("{user}@{host}/{database}")
    };
    Ok(UrlConnection {
        profile: temporary(name, driver, config),
        password,
        warning: None,
    })
}

/// `sqlite:///abs/path` and `sqlite://relative/path`: the rest is the file, and
/// `?mode=ro` opens it read-only.
fn file_connection(driver: &str, rest: &str) -> Result<UrlConnection, AppError> {
    let invalid = |reason: &str| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("not a connection URL: {reason}"),
        )
    };
    let rest = rest.split_once('#').map_or(rest, |(rest, _)| rest);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let parameters = Parameters::read(query, driver).map_err(|reason| invalid(&reason))?;
    let path = decode(path).map_err(|_| invalid("the path is not valid percent-encoding"))?;
    if path.is_empty() {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            format!(
                "not a connection URL: {driver}:// needs a file, such as {driver}:///path/to/file"
            ),
        ));
    }
    let mut connection = file(driver, std::path::Path::new(&path))?;
    if parameters.read_only {
        connection.profile.policy.read_only = Some(true);
    }
    Ok(connection)
}

/// A temporary connection to the file at `path`, named after it.
pub fn file(driver: &str, path: &std::path::Path) -> Result<UrlConnection, AppError> {
    let path = std::path::absolute(path).map_err(|error| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("{}: {error}", path.display()),
        )
    })?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let config = serde_json::json!({ "path": path.display().to_string() });
    Ok(UrlConnection {
        profile: temporary(name, driver, config),
        password: None,
        warning: None,
    })
}

/// The id comes from what the URL points at, its password left out: the same URL opened
/// again is the same connection, so its catalog cache is reused rather than piled up
/// under a fresh id each run, and documents bound to it last time find it.
fn temporary(name: String, driver: &str, config: serde_json::Value) -> ConnectionProfile {
    let target = format!("dexo-temporary:{driver}:{config}");
    ConnectionProfile::new(
        ConnectionId(Uuid::new_v5(&Uuid::NAMESPACE_URL, target.as_bytes())),
        None,
        name,
        driver,
        "local",
        config,
        SecretRef::new(Uuid::new_v4().to_string()),
    )
}

/// `host`, `host:port`, `[v6]` or `[v6]:port`.
fn split_host_port(hostport: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        return match after.strip_prefix(':') {
            Some(port) => Some((host.to_string(), Some(port.parse().ok()?))),
            None if after.is_empty() => Some((host.to_string(), None)),
            None => None,
        };
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), Some(port.parse().ok()?))),
        None => Some((hostport.to_string(), None)),
    }
}

/// The query parameters a Dexo URL understands. Any other is refused, not dropped: a
/// URL that says `?mode=ro` and opens the file for writing does worse than one that
/// does not parse.
#[derive(Default)]
struct Parameters {
    tls: Option<&'static str>,
    /// `sslrootcert`, `sslcert` and `sslkey`, as the TLS settings' `ca_file`,
    /// `client_cert` and `client_key`.
    tls_files: Vec<(&'static str, String)>,
    host: Option<String>,
    read_only: bool,
}

impl Parameters {
    fn read(query: &str, driver: &str) -> Result<Self, String> {
        let mut read = Self::default();
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value =
                decode(value).map_err(|_| format!("{key} is not valid percent-encoding"))?;
            let lower = value.to_ascii_lowercase().replace('-', "_");
            match (driver, key.to_ascii_lowercase().as_str()) {
                ("sqlite", "mode") => match lower.as_str() {
                    "ro" => read.read_only = true,
                    "rw" | "rwc" => {}
                    _ => return Err(format!("mode={value}: expected ro, rw or rwc")),
                },
                ("postgres" | "mysql" | "mariadb", "sslmode" | "ssl_mode" | "ssl-mode") => {
                    read.tls = Some(match lower.as_str() {
                        "disable" | "disabled" => "disable",
                        "allow" | "prefer" | "preferred" => "preferred",
                        "require" | "required" => "required",
                        "verify_ca" => "verify_ca",
                        "verify_full" | "verify_identity" => "verify_full",
                        _ => return Err(format!("{key}={value} is not a TLS mode")),
                    });
                }
                ("mysql" | "mariadb", "ssl") => {
                    read.tls = Some(match lower.as_str() {
                        "true" | "1" => "required",
                        "false" | "0" => "disable",
                        _ => return Err(format!("ssl={value}: expected true or false")),
                    });
                }
                ("postgres", "host") if !value.is_empty() => read.host = Some(value),
                ("postgres", "sslrootcert") => read.tls_files.push(("ca_file", value)),
                ("postgres", "sslcert") => read.tls_files.push(("client_cert", value)),
                ("postgres", "sslkey") => read.tls_files.push(("client_key", value)),
                // Said for the server's logs or the client's own clock; Dexo names
                // itself, keeps its own connect timeout, and always speaks UTF-8.
                (
                    "postgres",
                    "application_name" | "fallback_application_name" | "connect_timeout",
                )
                | ("postgres", "client_encoding")
                | ("mysql" | "mariadb", "charset" | "connect_timeout") => {}
                _ => {
                    return Err(format!(
                        "{key} is not a parameter Dexo reads in a {driver} URL; \
                         save a connection to set it"
                    ));
                }
            }
        }
        Ok(read)
    }
}

/// Percent-decoding, so `p%40ss` is `p@ss`. `+` is left alone: it is a space only in
/// form bodies, not in the parts of a URL a connection uses.
fn decode(text: &str) -> Result<String, ()> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Two hex digits; `from_str_radix` alone would take `%+1` as 0x01.
            let hex = text
                .get(i + 1..i + 3)
                .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .ok_or(())?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| ())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::parse;

    #[test]
    fn a_postgres_url_becomes_a_temporary_profile_and_a_password() {
        let parsed = parse("postgresql://ana:p%40ss%3Aword@db.example.com:6543/shop?sslmode=require&application_name=x").unwrap();
        assert_eq!(parsed.profile.driver, "postgres");
        assert_eq!(parsed.profile.name, "ana@db.example.com/shop");
        assert_eq!(parsed.profile.config["host"], "db.example.com");
        assert_eq!(parsed.profile.config["port"], 6543);
        assert_eq!(parsed.profile.config["database"], "shop");
        assert_eq!(parsed.profile.config["tls"]["mode"], "required");
        assert_eq!(parsed.password.unwrap().expose_secret(), "p@ss:word");
        // The same place under another password is the same connection.
        let again =
            parse("postgresql://ana:other@db.example.com:6543/shop?sslmode=require").unwrap();
        assert_eq!(again.profile.id, parsed.profile.id);
        // The password is the caller's to keep in memory; it is not in the profile.
        assert!(!parsed.profile.config.to_string().contains("p@ss"));
    }

    #[test]
    fn mysql_mariadb_and_ipv6_hosts() {
        let parsed = parse("mysql://root@[::1]/app?ssl-mode=VERIFY_IDENTITY").unwrap();
        assert_eq!(parsed.profile.driver, "mysql");
        assert_eq!(parsed.profile.config["host"], "::1");
        assert!(parsed.profile.config.get("port").is_none());
        assert_eq!(parsed.profile.config["tls"]["mode"], "verify_full");
        assert!(parsed.password.is_none());
        assert_eq!(
            parse("mariadb://u@h:3307").unwrap().profile.driver,
            "mariadb"
        );
    }

    #[test]
    fn file_urls_name_the_file() {
        let parsed = parse("sqlite:///tmp/shop%20copy.db").unwrap();
        assert_eq!(parsed.profile.driver, "sqlite");
        assert_eq!(parsed.profile.config["path"], "/tmp/shop copy.db");
        assert_eq!(parsed.profile.name, "shop copy.db");
        let relative = parse("sqlite://shop.db").unwrap();
        assert!(
            std::path::Path::new(relative.profile.config["path"].as_str().unwrap()).is_absolute()
        );
    }

    #[test]
    fn fragments_hosts_and_parameters() {
        let parsed = parse("postgres://u@%2Fvar%2Frun%2Fpostgresql/db#notes").unwrap();
        assert_eq!(parsed.profile.config["host"], "/var/run/postgresql");
        assert_eq!(parsed.profile.config["database"], "db");
        let parsed = parse("postgres://u@/db?host=/tmp/sock").unwrap();
        assert_eq!(parsed.profile.config["host"], "/tmp/sock");
        let parsed = parse("mariadb://u@h/db?ssl=true").unwrap();
        assert_eq!(parsed.profile.config["tls"]["mode"], "required");
        let parsed = parse("sqlite:///tmp/x.db?mode=ro").unwrap();
        assert_eq!(parsed.profile.policy.read_only, Some(true));
        assert_eq!(parsed.profile.config["path"], "/tmp/x.db");
    }

    #[test]
    fn what_is_not_a_connection_url_says_why() {
        for url in [
            "postgres://u%+1@h/db",
            "postgres://u@h/db?options=-c%20statement_timeout%3D0",
            "postgres://u@h/db?sslmode=sometimes",
            "sqlite:///x.db?mode=memory",
            "postgres.example.com",
            "ftp://u@h/x",
            "postgres://host/db",
            "postgres://u@h:port/db",
            "postgres://u:%zz@h/db",
            "sqlite://",
            "duckdb:///tmp/x.duckdb",
        ] {
            assert!(parse(url).is_err(), "{url}");
        }
    }
}
