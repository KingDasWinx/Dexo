//! Databases running in Docker, found through the `docker` CLI: Postgres, MySQL and
//! MariaDB containers that publish their port, with the user, database and password
//! their environment set. Docker is only read: `docker ps` and `docker inspect`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::NewConnection;

/// A container that answers as a database on this machine.
#[derive(Clone, PartialEq, Eq)]
pub struct DockerDatabase {
    pub container: String,
    pub image: String,
    /// What the connection form is filled with: name, driver, host, port, database, user.
    pub connection: NewConnection,
    /// From the container's environment, for the form's password field only.
    pub password: Option<String>,
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
/// answer within `timeout`, is no containers -- not an error worth saying.
pub fn discover(timeout: Duration) -> Vec<DockerDatabase> {
    let Some(ids) = run(&["ps", "-q", "--no-trunc"], timeout) else {
        return Vec::new();
    };
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["inspect"];
    args.extend(ids);
    run(&args, timeout)
        .map(|json| from_inspect(&json))
        .unwrap_or_default()
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
    let (username, database, password) = match driver {
        "postgres" => {
            let user = get("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
            let database = get("POSTGRES_DB").unwrap_or_else(|| user.clone());
            (user, database, get("POSTGRES_PASSWORD"))
        }
        _ => {
            // MariaDB's own names first, then the MySQL ones it also reads.
            let either = |mariadb: &str, mysql: &str| match driver {
                "mariadb" => get(mariadb).or_else(|| get(mysql)),
                _ => get(mysql),
            };
            let database = either("MARIADB_DATABASE", "MYSQL_DATABASE").unwrap_or_default();
            match either("MARIADB_USER", "MYSQL_USER") {
                Some(user) => (user, database, either("MARIADB_PASSWORD", "MYSQL_PASSWORD")),
                None => (
                    "root".into(),
                    database,
                    either("MARIADB_ROOT_PASSWORD", "MYSQL_ROOT_PASSWORD"),
                ),
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
        password,
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

/// `docker args…`'s output, or `None` when it cannot start, fails, or takes longer than
/// `timeout`; then it is stopped.
fn run(args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        let _ = sender.send(output);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::from_inspect;

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
                    db.password.as_deref(),
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
                    Some("s3cret")
                ),
                (
                    "legacy",
                    "mysql",
                    "127.0.0.1",
                    Some(3307),
                    "root",
                    "",
                    Some("root")
                ),
                (
                    "maria",
                    "mariadb",
                    "127.0.0.1",
                    Some(3308),
                    "bo",
                    "app",
                    Some("pw")
                ),
            ]
        );
        // The password never shows in a debug print.
        assert!(!format!("{:?}", found[0]).contains("s3cret"));
        assert!(from_inspect("not json").is_empty());
    }
}
