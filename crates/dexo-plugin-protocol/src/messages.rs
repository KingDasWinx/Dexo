//! The params and results of every method.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use dexo_driver_api::{
    CapabilityState, ConnectRequest, ConnectionSecrets, DbValue, DriverError, QueryEvent, QueryId,
    QueryRequest, TransactionState, TransportRequest,
};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitializeParams {
    pub dexo_version: String,
    pub protocols: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol: u32,
    pub driver_version: String,
    pub drivers: Vec<String>,
}

/// What `ConnectRequest` carries, secrets included: they travel only inside this
/// message on the plugin's stdin, never on its command line or in its environment.
#[derive(Clone, Serialize, Deserialize)]
pub struct SessionOpenParams {
    pub driver: String,
    pub endpoint: String,
    pub database: Option<String>,
    pub username: String,
    pub password: String,
    pub read_only: bool,
    pub transport: TransportRequest,
    pub secrets: BTreeMap<String, String>,
}

impl fmt::Debug for SessionOpenParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionOpenParams")
            .field("driver", &self.driver)
            .field("endpoint", &self.endpoint)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("read_only", &self.read_only)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl SessionOpenParams {
    pub fn new(driver: &str, request: &ConnectRequest) -> Self {
        Self {
            driver: driver.into(),
            endpoint: request.endpoint.clone(),
            database: request.database.clone(),
            username: request.username.clone(),
            password: request.secret.expose_secret().to_string(),
            read_only: request.read_only,
            transport: request.transport.clone(),
            secrets: request
                .secrets
                .iter()
                .map(|(purpose, secret)| (purpose.to_string(), secret.expose_secret().to_string()))
                .collect(),
        }
    }

    pub fn into_request(self) -> ConnectRequest {
        let mut secrets = ConnectionSecrets::default();
        for (purpose, secret) in self.secrets {
            secrets.insert(purpose, SecretString::from(secret));
        }
        ConnectRequest {
            endpoint: self.endpoint,
            database: self.database,
            username: self.username,
            secret: SecretString::from(self.password),
            read_only: self.read_only,
            transport: self.transport,
            secrets,
        }
    }
}

/// A group of methods a session answers beyond queries. Negotiated per session, so a
/// family added later needs no new protocol version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Catalog,
    Data,
    Bulk,
    Explain,
    Transactions,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionOpenResult {
    pub session: String,
    #[serde(deserialize_with = "known_only")]
    pub capabilities: Vec<CapabilityState>,
    #[serde(deserialize_with = "known_only")]
    pub families: Vec<Family>,
    #[serde(default)]
    pub server_session_id: Option<String>,
    /// The transaction state, for a session with the transactions family.
    #[serde(default)]
    pub transaction: Option<TransactionState>,
}

/// The entries this build knows. What a newer plugin adds -- a family, a capability --
/// is left out instead of failing the whole message.
fn known_only<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let values = Vec::<Value>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

/// A method's params on one session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnSession<T> {
    pub session: String,
    pub args: T,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryExecute {
    pub query: Uuid,
    pub sql: String,
    pub params: Vec<DbValue>,
    pub row_limit: u64,
    pub timeout_ms: u64,
    pub read_only: bool,
    pub mutating: bool,
}

impl QueryExecute {
    pub fn new(request: &QueryRequest) -> Self {
        Self {
            query: request.id.0,
            sql: request.sql.clone(),
            params: request.parameters.clone(),
            row_limit: request.row_limit,
            timeout_ms: u64::try_from(request.timeout.as_millis()).unwrap_or(u64::MAX),
            read_only: request.read_only,
            mutating: request.mutating,
        }
    }

    pub fn into_request(self) -> QueryRequest {
        QueryRequest {
            id: QueryId(self.query),
            sql: self.sql,
            parameters: self.params,
            row_limit: self.row_limit,
            timeout: Duration::from_millis(self.timeout_ms),
            mutating: self.mutating,
            read_only: self.read_only,
        }
    }
}

/// A page of a result. While `done` is false, `cursor` names the rest. `query/execute`
/// answers with the cursor and no events, once the driver has the query; `query/fetch`
/// answers the pages. The last page carries the session's transaction state -- a
/// statement may have changed it -- and an error ends the result.
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub events: Vec<QueryEvent>,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DriverError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionState>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub cursor: String,
}

/// What a transaction method left: the state and notice the host keeps for the
/// synchronous `state()` and `take_notice()`, and the error when it failed -- after
/// which the state is still news.
#[derive(Debug, Serialize, Deserialize)]
pub struct TxResult {
    pub state: TransactionState,
    #[serde(default)]
    pub notice: Option<String>,
    #[serde(default)]
    pub error: Option<DriverError>,
}

/// Bytes as one base64 string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bytes(#[serde(with = "dexo_driver_api::base64_bytes")] pub Vec<u8>);

/// The request a `$/cancel_request` drops.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CancelRequest {
    pub id: u64,
}
