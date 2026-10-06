use std::borrow::Cow;
use std::sync::{PoisonError, RwLock};

use secrecy::SecretString;

use crate::query::SessionEventStream;
use crate::transport::{ConnectionSecrets, TransportRequest};
use crate::{
    AdministrationProvider, BulkWriter, CapabilityState, CatalogReader, DataMutator, DdlExecutor,
    DriverError, ExplainProvider, QueryId, QueryRequest, QueryStream, SecurityAdmin,
    TransactionControl,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionOptions {
    pub tls: bool,
    pub client_certificate: bool,
    pub ssh: bool,
    pub proxy: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverDescriptor {
    pub id: Cow<'static, str>,
    pub display_name: Cow<'static, str>,
    pub default_port: u16,
    pub options: ConnectionOptions,
    /// Opens a file instead of dialling a host: no port, user, password or transport.
    pub file: bool,
    /// The plugin that brings the driver, for one Dexo does not build in.
    pub plugin: Option<Cow<'static, str>>,
    /// Whether it can connect now: a plugin's driver is known before it is installed,
    /// so the connection form can offer to install it.
    pub installed: bool,
}

/// Drivers made known while Dexo runs: installed plugins, and the index's drivers that
/// are not installed.
static ANNOUNCED: RwLock<Vec<DriverDescriptor>> = RwLock::new(Vec::new());

impl DriverDescriptor {
    pub fn postgres() -> Self {
        Self {
            id: "postgres".into(),
            display_name: "PostgreSQL".into(),
            default_port: 5432,
            options: ConnectionOptions {
                tls: true,
                client_certificate: true,
                ssh: true,
                proxy: true,
            },
            file: false,
            plugin: None,
            installed: true,
        }
    }

    pub fn mysql() -> Self {
        Self {
            id: "mysql".into(),
            display_name: "MySQL".into(),
            default_port: 3306,
            options: ConnectionOptions {
                tls: true,
                client_certificate: true,
                ssh: true,
                proxy: true,
            },
            file: false,
            plugin: None,
            installed: true,
        }
    }

    pub fn sqlite() -> Self {
        Self {
            id: "sqlite".into(),
            display_name: "SQLite".into(),
            default_port: 0,
            options: ConnectionOptions {
                tls: false,
                client_certificate: false,
                ssh: false,
                proxy: false,
            },
            file: true,
            plugin: None,
            installed: true,
        }
    }

    /// A DuckDB file, `:memory:`, or a CSV, Parquet or JSON file read as a table. Its
    /// engine is a plugin: Dexo keeps its dialect and its guard, the plugin brings it.
    pub fn duckdb() -> Self {
        Self {
            id: "duckdb".into(),
            display_name: "DuckDB".into(),
            plugin: Some("duckdb".into()),
            installed: false,
            ..Self::sqlite()
        }
    }

    /// A driver a plugin brings, as its manifest or the index describes it.
    pub fn plugin_driver(
        id: impl Into<String>,
        display_name: impl Into<String>,
        file: bool,
        plugin: impl Into<String>,
        installed: bool,
    ) -> Self {
        Self {
            id: Cow::Owned(id.into()),
            display_name: Cow::Owned(display_name.into()),
            default_port: 0,
            options: ConnectionOptions {
                tls: false,
                client_certificate: false,
                ssh: false,
                proxy: false,
            },
            file,
            plugin: Some(Cow::Owned(plugin.into())),
            installed,
        }
    }

    /// Makes `descriptor` known, in place of an earlier announcement with its id; a
    /// plugin's in place of a built-in one only when it is that plugin's to bring.
    pub fn announce(descriptor: Self) {
        if let Some(plugin) = &descriptor.plugin
            && !Self::may_provide(plugin, &descriptor.id)
        {
            return;
        }
        let mut announced = ANNOUNCED.write().unwrap_or_else(PoisonError::into_inner);
        announced.retain(|known| known.id != descriptor.id);
        announced.push(descriptor);
    }

    pub fn withdraw(id: &str) {
        ANNOUNCED
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|known| known.id != id);
    }

    /// The drivers Dexo is built knowing, in the order the form lists them.
    fn built_in() -> [Self; 5] {
        [
            Self::postgres(),
            Self::mysql(),
            Self::mariadb(),
            Self::sqlite(),
            Self::duckdb(),
        ]
    }

    /// Whether plugin `plugin` may bring driver `id`: not one Dexo has as its own, nor
    /// one it knows as another plugin's. The plugin would receive its passwords.
    pub fn may_provide(plugin: &str, id: &str) -> bool {
        Self::built_in()
            .iter()
            .all(|known| known.id != id || known.plugin.as_deref() == Some(plugin))
    }

    /// Every driver Dexo knows: the built-in ones in the order the form lists them, each
    /// as announced when it was, then the other announced ones by name.
    pub fn known() -> Vec<Self> {
        let announced = ANNOUNCED
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut known: Vec<Self> = Self::built_in()
            .into_iter()
            .map(|builtin| {
                announced
                    .iter()
                    .find(|found| found.id == builtin.id)
                    .cloned()
                    .unwrap_or(builtin)
            })
            .collect();
        let mut rest: Vec<Self> = announced
            .into_iter()
            .filter(|found| !known.iter().any(|builtin| builtin.id == found.id))
            .collect();
        rest.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        known.extend(rest);
        known
    }

    pub fn mariadb() -> Self {
        Self {
            id: "mariadb".into(),
            display_name: "MariaDB".into(),
            ..Self::mysql()
        }
    }

    /// The SQL family a driver id speaks: MariaDB speaks MySQL. Everything keyed on
    /// the dialect asks this instead of comparing ids.
    pub fn family(id: &str) -> &str {
        match id {
            "mariadb" => "mysql",
            other => other,
        }
    }

    pub fn for_id(id: &str) -> Option<Self> {
        Self::known()
            .into_iter()
            .find(|descriptor| descriptor.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub endpoint: String,
    pub database: Option<String>,
    pub username: String,
    pub secret: SecretString,
    pub read_only: bool,
    pub transport: TransportRequest,
    pub secrets: ConnectionSecrets,
}

impl ConnectRequest {
    pub fn new(
        endpoint: impl Into<String>,
        database: Option<String>,
        username: String,
        secret: SecretString,
        read_only: bool,
    ) -> Self {
        let endpoint = endpoint.into();
        let transport = TransportRequest::from_endpoint(&endpoint)
            .unwrap_or_else(|_| TransportRequest::direct(endpoint.clone(), 1));
        let secrets = ConnectionSecrets::database_password(secret.clone());
        Self {
            endpoint,
            database,
            username,
            secret,
            read_only,
            transport,
            secrets,
        }
    }
}

#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync {
    fn descriptor(&self) -> DriverDescriptor;
    fn driver_name(&self) -> String {
        self.descriptor().id.into_owned()
    }
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError>;
}

/// A database session. A wrapper -- `dexo_app::pre_connect`'s, which keeps a tunnel
/// with its session -- forwards every method: an optional one added here that it does
/// not forward reads as `None` through it.
#[async_trait::async_trait]
pub trait Session: Send + Sync {
    fn capabilities(&self) -> &[CapabilityState];
    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError>;
    async fn cancel(&self, query: QueryId) -> Result<(), DriverError>;
    async fn close(self: Box<Self>) -> Result<(), DriverError>;
    fn transactions(&self) -> Option<&dyn TransactionControl> {
        None
    }

    fn catalog(&self) -> Option<&dyn CatalogReader> {
        None
    }

    fn data(&self) -> Option<&dyn DataMutator> {
        None
    }

    fn ddl(&self) -> Option<&dyn DdlExecutor> {
        None
    }

    fn security(&self) -> Option<&dyn SecurityAdmin> {
        None
    }

    fn bulk(&self) -> Option<&dyn BulkWriter> {
        None
    }

    fn explain(&self) -> Option<&dyn ExplainProvider> {
        None
    }

    fn admin(&self) -> Option<&dyn AdministrationProvider> {
        None
    }

    /// What the server calls this session in its own list of them: a Postgres backend's
    /// pid, a MySQL connection id. It is how Dexo's own sessions are told from others'.
    fn server_session_id(&self) -> Option<String> {
        None
    }

    fn events(&self) -> Option<SessionEventStream> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::DriverDescriptor;

    #[test]
    fn an_announced_driver_is_known_and_replaces_its_earlier_self() {
        assert!(!DriverDescriptor::for_id("duckdb").unwrap().installed);
        assert!(DriverDescriptor::for_id("postgres").unwrap().installed);
        let id = "announce-test";
        DriverDescriptor::announce(DriverDescriptor::plugin_driver(
            id,
            "Announce Test",
            true,
            "announce-test",
            false,
        ));
        let found = DriverDescriptor::for_id(id).unwrap();
        assert_eq!(found.display_name, "Announce Test");
        assert!(found.file && !found.installed);
        assert_eq!(found.plugin.as_deref(), Some("announce-test"));
        DriverDescriptor::announce(DriverDescriptor::plugin_driver(
            id,
            "Announce Test",
            true,
            "announce-test",
            true,
        ));
        assert!(DriverDescriptor::for_id(id).unwrap().installed);
        assert_eq!(
            DriverDescriptor::known()
                .iter()
                .filter(|known| known.id == id)
                .count(),
            1
        );
        DriverDescriptor::withdraw(id);
        assert!(DriverDescriptor::for_id(id).is_none());
    }
}
