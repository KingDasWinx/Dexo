use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SqlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// One read-only statement: SELECT, WITH … SELECT, VALUES, TABLE, or EXPLAIN without ANALYZE.
    pub sql: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataInsertInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`; a bare name uses the connection's default database and schema.
    pub target: String,
    /// Column name to value for the new row.
    pub values: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataUpdateInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`.
    pub target: String,
    /// Primary-key (or unique) column to value, naming exactly one row.
    pub identity: Map<String, Value>,
    /// Column name to new value.
    pub values: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataDeleteInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`.
    pub target: String,
    /// Primary-key (or unique) column to value, naming exactly one row.
    pub identity: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataSqlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// The table the grant was issued for. Every table the statement touches must be inside the grant.
    pub target: String,
    /// One INSERT, UPDATE or DELETE.
    pub sql: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DdlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// The object the grant was issued for.
    pub target: String,
    /// One CREATE TABLE, ALTER TABLE, DROP, CREATE INDEX or CREATE VIEW.
    pub sql: String,
    /// Required for destructive DDL: repeat `target` exactly to confirm.
    pub confirm_target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdminActionInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this action. Retrying with the same id and payload returns the first result.
    pub operation_id: String,
    /// Server session to act on, from `admin_list_sessions`.
    pub session_id: String,
    /// Required to terminate a session: repeat `session_id` exactly to confirm.
    pub confirm_target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogListInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// `id` of the node whose children to list, from an earlier catalog_list or catalog_search; omit for the roots.
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogSearchInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Part of a table, view or column name.
    pub query: String,
    /// At most this many hits (default 50, at most 200).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObjectInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Qualified name such as `public.orders` or `db.public.orders`; a bare name uses the connection's default database and schema.
    pub name: String,
}
