//! Databases running in Docker, found through the `docker` CLI: Postgres, MySQL and
//! MariaDB containers that publish their port, with the user, database and password
//! their environment set. Docker is only read: `docker ps` and `docker inspect`.

use std::process::Command;
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};

use crate::NewConnection;

/// A container that answers as a database on this machine.
#[derive(Clone)]
pub struct DockerDatabase {
    pub container: String,
    pub image: String,
    /// What the connection form is filled with: name, driver, host, port, database, user.
    pub connection: NewConnection,
    /// From the container's environment, for the form's password field only.
    pub password: Option<SecretString>,
    /// The container lets its user in without a password.
    pub passwordless: bool,
}

impl PartialEq for DockerDatabase {
    fn eq(&self, other: &Self) -> bool {
        let password = |database: &Self| {
            database
                .password
                .as_ref()
                .map(|password| password.expose_secret().to_string())
        };
        self.container == other.container
            && self.image == other.image
            && self.connection == other.connection
            && self.passwordless == other.passwordless
            && password(self) == password(other)
    }
}

impl std::fmt::Debug for DockerDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerDatabase")
            .field("container", &self.container)
            .field("image", &self.image)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .finish_non_exhaustive()
    }
}

/// The running database containers. No `docker` on PATH, or a daemon that does not
/// answer within `timeout` -- all its calls together -- is no containers, not an error
/// worth saying.
pub fn discover(timeout: Duration) -> Vec<DockerDatabase> {
    discover_with("docker", timeout)
}

fn discover_with(docker: &str, timeout: Duration) -> Vec<DockerDatabase> {
    let deadline = Instant::now() + timeout;
    let Some(ids) = run(docker, &["ps", "-q", "--no-trunc"], deadline) else {
        return Vec::new();
    };
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["inspect"];
    args.extend(ids);
    let Some(json) = run(docker, &args, deadline) else {
        return Vec::new();
    };
    let found = from_inspect(&json);
    if found.is_empty() {
        return found;
    }
    let daemon = daemon_host(docker, deadline);
    found
        .into_iter()
        .map(|mut database| {
            if let Some(daemon) = &daemon
                && database.connection.host == "127.0.0.1"
            {
                database.connection.host = daemon.clone();
            }
            database
        })
        .collect()
}

/// The host a remote daemon runs on -- `DOCKER_HOST`, else the current context's
/// endpoint -- where its containers publish their ports; `None` for a local one.
fn daemon_host(docker: &str, deadline: Instant) -> Option<String> {
    let endpoint = match std::env::var("DOCKER_HOST") {
        Ok(host) if !host.trim().is_empty() => host,
        _ => run(
            docker,
            &[
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ],
            deadline,
        )?,
    };
    remote_host(endpoint.trim())
}

/// `tcp://10.0.0.5:2376` or `ssh://me@build-box` is that host; a socket or pipe is
/// this machine.
fn remote_host(endpoint: &str) -> Option<String> {
    let (scheme, rest) = endpoint.split_once("://")?;
    if !matches!(scheme, "tcp" | "ssh" | "http" | "https") {
        return None;
    }
    let authority = rest.split('/').next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next()?,
        None => host.rsplit_once(':').map_or(host, |(host, _)| host),
    };
    (!host.is_empty() && !matches!(host, "localhost" | "127.0.0.1" | "::1"))
        .then(|| host.to_string())
}

/// `docker inspect`'s JSON array, read for databases.
pub fn from_inspect(json: &str) -> Vec<DockerDatabase> {
    let Ok(serde_json::Value::Array(containers)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    containers.iter().filter_map(database).collect()
}

fn database(container: &serde_json::Value) -> Option<DockerDatabase> {
    let name = container
        .get("Name")?
        .as_str()?
        .trim_start_matches('/')
        .to_string();
    let image = container.pointer("/Config/Image")?.as_str()?.to_string();
    let env: Vec<(String, String)> = container
        .pointer("/Config/Env")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let get = |key: &str| {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
    };
    let has_prefix = |prefix: &str| env.iter().any(|(name, _)| name.starts_with(prefix));
    let lower = image.to_lowercase();
    // MariaDB first: its images take the MYSQL_ variables too.
    let driver = if lower.contains("mariadb") || has_prefix("MARIADB_") {
        "mariadb"
    } else if lower.contains("mysql") || has_prefix("MYSQL_") {
        "mysql"
    } else if lower.contains("postgres") || lower.contains("postgis") || has_prefix("POSTGRES_") {
        "postgres"
    } else {
        return None;
    };
    let inner_port = if driver == "postgres" { 5432 } else { 3306 };
    let (host, port) = published(container, inner_port)?;
    let yes = |key: &str| {
        get(key).is_some_and(|value| matches!(value.to_lowercase().as_str(), "yes" | "true" | "1"))
    };
    let (username, database, password, passwordless) = match driver {
        "postgres" => {
            let user = get("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
            let database = get("POSTGRES_DB").unwrap_or_else(|| user.clone());
            let trusted = get("POSTGRES_HOST_AUTH_METHOD").as_deref() == Some("trust");
            (user, database, get("POSTGRES_PASSWORD"), trusted)
        }
        _ => {
            // MariaDB's own names first, then the MySQL ones it also reads.
            let either = |mariadb: &str, mysql: &str| match driver {
                "mariadb" => get(mariadb).or_else(|| get(mysql)),
                _ => get(mysql),
            };
            // A container made with only a root password has no database of its own; the
            // server's `mysql` schema is always there to connect to.
            let database =
                either("MARIADB_DATABASE", "MYSQL_DATABASE").unwrap_or_else(|| "mysql".into());
            match either("MARIADB_USER", "MYSQL_USER") {
                Some(user) => (
                    user,
                    database,
                    either("MARIADB_PASSWORD", "MYSQL_PASSWORD"),
                    false,
                ),
                None => {
                    let empty = match driver {
                        "mariadb" => {
                            yes("MARIADB_ALLOW_EMPTY_ROOT_PASSWORD")
                                || yes("MYSQL_ALLOW_EMPTY_PASSWORD")
                        }
                        _ => yes("MYSQL_ALLOW_EMPTY_PASSWORD"),
                    };
                    (
                        "root".into(),
                        database,
                        either("MARIADB_ROOT_PASSWORD", "MYSQL_ROOT_PASSWORD"),
                        empty,
                    )
                }
            }
        }
    };
    Some(DockerDatabase {
        container: name.clone(),
        image,
        connection: NewConnection {
            name,
            driver: driver.into(),
            host,
            port: Some(port),
            database,
            username,
            ..NewConnection::default()
        },
        passwordless: passwordless && password.is_none(),
        password: password.map(SecretString::from),
    })
}

/// Where the container's `inner` port is published on this machine: the first binding,
/// any address read as localhost.
fn published(container: &serde_json::Value, inner: u16) -> Option<(String, u16)> {
    let bindings = container
        .pointer(&format!("/NetworkSettings/Ports/{inner}~1tcp"))?
        .as_array()?;
    bindings.iter().find_map(|binding| {
        let port = binding.get("HostPort")?.as_str()?.parse().ok()?;
        let host = match binding.get("HostIp").and_then(serde_json::Value::as_str) {
            None | Some("" | "0.0.0.0" | "::") => "127.0.0.1".to_string(),
            Some(address) => address.to_string(),
        };
        Some((host, port))
    })
}

/// `docker args…`'s output, or `None` when it cannot start, fails, or is still running
/// at `deadline`; then it is stopped, with whatever it started.
fn run(docker: &str, args: &[&str], deadline: Instant) -> Option<String> {
    let mut command = Command::new(docker);
    command.args(args).stderr(std::process::Stdio::null());
    crate::process::detach(&mut command);
    match crate::process::run_until(command, deadline, crate::process::stop_group).ok()? {
        crate::process::Ran::Exited(status, Some(output)) if status.success() => {
            String::from_utf8(output).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::{from_inspect, remote_host};

    /// What `docker inspect` says of a Postgres, a MySQL with only a root password, a
    /// MariaDB with a user, and a container that publishes nothing or is not a database.
    #[test]
    fn database_containers_become_prefilled_connections() {
        let json = r#"[
          {"Name": "/shop-pg", "Config": {"Image": "postgres:16",
             "Env": ["POSTGRES_USER=ana", "POSTGRES_PASSWORD=s3cret", "POSTGRES_DB=shop", "PATH=/bin"]},
           "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "5433"},
                                                     {"HostIp": "::", "HostPort": "5433"}]}}},
          {"Name": "/legacy", "Config": {"Image": "mysql:8.4", "Env": ["MYSQL_ROOT_PASSWORD=root"]},
           "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "127.0.0.1", "HostPort": "3307"}],
                                         "33060/tcp": null}}},
          {"Name": "/maria", "Config": {"Image": "mariadb:11.4",
             "Env": ["MARIADB_USER=bo", "MARIADB_PASSWORD=pw", "MARIADB_DATABASE=app"]},
           "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "", "HostPort": "3308"}]}}},
          {"Name": "/open-my", "Config": {"Image": "mysql:8.4", "Env": ["MYSQL_ALLOW_EMPTY_PASSWORD=yes"]},
           "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3309"}]}}},
          {"Name": "/trusting-pg", "Config": {"Image": "postgres:16", "Env": ["POSTGRES_HOST_AUTH_METHOD=trust"]},
           "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "5434"}]}}},
          {"Name": "/internal-pg", "Config": {"Image": "postgres:16", "Env": []},
           "NetworkSettings": {"Ports": {"5432/tcp": null}}},
          {"Name": "/web", "Config": {"Image": "nginx", "Env": []},
           "NetworkSettings": {"Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]}}}
        ]"#;
        let found = from_inspect(json);
        let summary: Vec<_> = found
            .iter()
            .map(|db| {
                (
                    db.container.as_str(),
                    db.connection.driver.as_str(),
                    db.connection.host.as_str(),
                    db.connection.port,
                    db.connection.username.as_str(),
                    db.connection.database.as_str(),
                    db.password
                        .as_ref()
                        .map(|password| password.expose_secret().to_string()),
                    db.passwordless,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "shop-pg",
                    "postgres",
                    "127.0.0.1",
                    Some(5433),
                    "ana",
                    "shop",
                    Some("s3cret".to_string()),
                    false
                ),
                (
                    "legacy",
                    "mysql",
                    "127.0.0.1",
                    Some(3307),
                    "root",
                    "mysql",
                    Some("root".to_string()),
                    false
                ),
                (
                    "maria",
                    "mariadb",
                    "127.0.0.1",
                    Some(3308),
                    "bo",
                    "app",
                    Some("pw".to_string()),
                    false
                ),
                (
                    "open-my",
                    "mysql",
                    "127.0.0.1",
                    Some(3309),
                    "root",
                    "mysql",
                    None,
                    true
                ),
                (
                    "trusting-pg",
                    "postgres",
                    "127.0.0.1",
                    Some(5434),
                    "postgres",
                    "postgres",
                    None,
                    true
                ),
            ]
        );
        // The password never shows in a debug print.
        assert!(!format!("{:?}", found[0]).contains("s3cret"));
        assert!(from_inspect("not json").is_empty());
    }

    /// A remote daemon's containers publish on its host; a socket is this machine.
    #[test]
    fn a_remote_daemon_is_where_its_ports_are() {
        assert_eq!(
            remote_host("tcp://10.0.0.5:2376").as_deref(),
            Some("10.0.0.5")
        );
        assert_eq!(
            remote_host("ssh://me@build-box").as_deref(),
            Some("build-box")
        );
        assert_eq!(
            remote_host("ssh://me@build-box:2222").as_deref(),
            Some("build-box")
        );
        assert_eq!(
            remote_host("tcp://[fd00::5]:2376").as_deref(),
            Some("fd00::5")
        );
        assert_eq!(remote_host("unix:///var/run/docker.sock"), None);
        assert_eq!(remote_host("npipe:////./pipe/docker_engine"), None);
        assert_eq!(remote_host("tcp://localhost:2375"), None);
    }

    /// A daemon that hangs is given up on within the time asked, all calls together,
    /// and what the `docker` it ran started is stopped with it.
    #[cfg(unix)]
    #[test]
    fn a_hanging_daemon_is_given_up_on() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("docker");
        std::fs::write(&shim, "#!/bin/sh\nsleep 98761 &\nsleep 98761\n").unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        let found = super::discover_with(
            shim.to_str().unwrap(),
            std::time::Duration::from_millis(500),
        );
        assert!(found.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::process::Command::new("pgrep")
            .args(["-f", "sleep 98761"])
            .output()
            .is_ok_and(|output| !output.stdout.is_empty())
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the shim's sleeps outlived discovery"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
