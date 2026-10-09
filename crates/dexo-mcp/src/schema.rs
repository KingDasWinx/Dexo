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
pub struct ExplainInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// One read-only statement: SELECT, WITH … SELECT, VALUES or TABLE.
    pub sql: String,
    /// Indexes to plan with as if they were built, without building them -- each one
    /// `CREATE INDEX ON table (columns)`. Postgres with the hypopg extension only; they
    /// exist for this plan alone.
    #[serde(default)]
    pub hypothetical_indexes: Vec<String>,
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
pub struct DataImportInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this import. Retrying with the same id and the same file returns the first result instead of importing twice.
    pub operation_id: String,
    /// The table the rows go into, such as `public.customers`; it must be one the profile sees.
    pub target: String,
    /// The data file, by its path inside the project folder the server was started in: .csv, .tsv, .json (an array of objects) or .jsonl, at most 32 MiB.
    pub file: String,
    /// csv, tsv, json or jsonl; the file's extension says when left out.
    pub format: Option<String>,
    /// A file column into a table column, as `source=target`; `source=` leaves it out. Each column goes to the table's column of its own name otherwise.
    pub mapping: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataSqlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// What the audit names this change by; the tables it touches when left out. Every table the SQL touches must be one the profile sees.
    pub target: Option<String>,
    /// One or more INSERT, UPDATE, DELETE or (on Postgres) TRUNCATE statements separated by `;`, run as one transaction: all of them, or none if one fails. One approval covers the whole script. Pass this or `file`.
    pub sql: Option<String>,
    /// A .sql file to run instead of `sql`, by its path inside the project folder the server was started in, at most 1 MiB. It is read once: what is approved is what runs.
    pub file: Option<String>,
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
    /// Server session to act on, from `admin_list_sessions`, listed while this profile may end sessions.
    pub session_id: String,
    /// Required to terminate a session: repeat `session_id` exactly to confirm.
    pub confirm_target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogListInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// `id` of the node whose children to list, from an earlier catalog_list; omit for the roots.
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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct OverviewInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// The schema to describe (on MySQL, the database); every one the profile sees when left out.
    pub schema: Option<String>,
    /// Count each table's rows with count(*) instead of the server's estimate: exact, but it reads every row, 200 tables at most.
    pub exact_counts: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataReadInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Table or view to read, qualified like object_describe.
    pub table: String,
    /// Row offset of the page (default 0); use the `next_offset` of the previous page.
    pub offset: Option<u64>,
    /// Rows per page (default 100, capped by the profile's max_rows).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DiffInput {
    /// Connection the snapshots were taken of; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Name of the older snapshot, saved with `dexo schema snapshot --connection C --name N`.
    pub from_snapshot: String,
    /// Name of the newer snapshot.
    pub to_snapshot: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdminListInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
}
