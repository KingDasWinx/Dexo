//! A whole schema in one read: each table and view the profile sees, with its columns,
//! defaults, keys, constraints and row count. An agent asked object_describe one table at
//! a time, and wrote information_schema queries and a UNION of count(*) for the rest.

use std::time::Duration;

use dexo_driver_api::{DbValue, QueryEvent, QueryRequest, Session, TransactionMode};
use dexo_sql::Dialect;
use futures_util::StreamExt;
use serde::Serialize;

use crate::data::display_value;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::connection::McpConnection;
use crate::mcp::policy::Decision;
use crate::mcp::selector::ObjectRef;
use crate::mcp::service::McpService;
use crate::query_service::map_driver_error;

/// The most catalog rows one overview reads.
const MAX_CATALOG_ROWS: u64 = 50_000;
/// The most tables exact counts read every row of.
pub const MAX_EXACT_COUNTS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TableOverview {
    /// `database.schema.table` on Postgres, `database.table` on MySQL.
    pub name: String,
    /// table, view, materialized view, partitioned table, foreign table.
    pub kind: String,
    /// What the server's statistics say; none when it has not counted.
    pub estimated_rows: Option<i64>,
    /// Counted with count(*), when asked for.
    pub rows: Option<i64>,
    pub columns: Vec<ColumnOverview>,
    /// `orders_pkey PRIMARY KEY (id)`, `FOREIGN KEY (customer_id) REFERENCES customers(id)`.
    pub constraints: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ColumnOverview {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub nullable: bool,
    pub default: Option<String>,
}

/// What the overview read.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Overview {
    pub tables: Vec<TableOverview>,
    /// The catalog had more than one overview reads: name a schema.
    pub truncated: bool,
}

impl McpService {
    /// Every table and view of `schema` (all of them when none) the profile sees, read
    /// inside `BEGIN READ ONLY … ROLLBACK`. The catalog queries are Dexo's own, and the
    /// schema is matched here, not put into them. A foreign key to a table the profile
    /// does not see is left out, so its name does not reveal it.
    pub async fn overview(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        schema: Option<&str>,
        exact_counts: bool,
    ) -> Result<Overview, AppError> {
        let transactions = session.transactions().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Capability,
                "this connection cannot open a read-only transaction",
            )
        })?;
        transactions
            .begin(TransactionMode::ReadOnly)
            .await
            .map_err(map_driver_error)?;
        let outcome = self
            .read_overview(session, connection, schema, exact_counts)
            .await;
        let rolled_back = transactions.rollback().await.map_err(map_driver_error);
        let overview = outcome?;
        rolled_back?;
        Ok(overview)
    }

    async fn read_overview(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        schema: Option<&str>,
        exact_counts: bool,
    ) -> Result<Overview, AppError> {
        let (columns_sql, constraints_sql) = match connection.dialect {
            Dialect::Postgres => (POSTGRES_COLUMNS, POSTGRES_CONSTRAINTS),
            Dialect::Mysql => (MYSQL_COLUMNS, MYSQL_CONSTRAINTS),
            _ => {
                return Err(AppError::new(
                    ErrorCategory::Capability,
                    "the overview reads Postgres and MySQL catalogs",
                ));
            }
        };
        let policy = self.policy();
        let path_of = |schema_name: &str, table: &str| -> ObjectRef {
            match connection.dialect {
                Dialect::Postgres => ObjectRef {
                    path: connection
                        .database
                        .iter()
                        .cloned()
                        .chain([schema_name.to_string(), table.to_string()])
                        .collect(),
                },
                _ => ObjectRef {
                    path: vec![schema_name.to_string(), table.to_string()],
                },
            }
        };
        let wanted = |schema_name: &str| schema.is_none_or(|wanted| wanted == schema_name);
        let seen = |schema_name: &str, table: &str| {
            policy.decide(&path_of(schema_name, table)) == Decision::Allow
        };
        let (column_rows, mut truncated) = self.catalog_rows(session, columns_sql).await?;
        // (schema, table) of each overview, to count and to attach constraints to.
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut tables: Vec<TableOverview> = Vec::new();
        for row in &column_rows {
            let [
                schema_name,
                table,
                kind,
                estimate,
                column,
                type_name,
                nullable,
                default,
            ] = row.as_slice()
            else {
                continue;
            };
            let (Some(schema_name), Some(table)) = (text(schema_name), text(table)) else {
                continue;
            };
            if !wanted(&schema_name) || !seen(&schema_name, &table) {
                continue;
            }
            if keys.last() != Some(&(schema_name.clone(), table.clone())) {
                keys.push((schema_name.clone(), table.clone()));
                tables.push(TableOverview {
                    name: path_of(&schema_name, &table).to_string(),
                    kind: kind_words(&text(kind).unwrap_or_default()).into(),
                    estimated_rows: text(estimate)
                        .and_then(|rows| rows.parse::<i64>().ok())
                        .filter(|rows| *rows >= 0),
                    ..TableOverview::default()
                });
            }
            let table = tables.last_mut().expect("pushed above");
            table.columns.push(ColumnOverview {
                name: text(column).unwrap_or_default(),
                type_name: text(type_name).unwrap_or_default(),
                nullable: is_true(nullable),
                default: text(default),
            });
        }
        let (constraint_rows, cut) = self.catalog_rows(session, constraints_sql).await?;
        truncated |= cut;
        for row in &constraint_rows {
            let [schema_name, table, definition, to_schema, to_table] = row.as_slice() else {
                continue;
            };
            let (Some(schema_name), Some(table), Some(definition)) =
                (text(schema_name), text(table), text(definition))
            else {
                continue;
            };
            let Some(at) = keys
                .iter()
                .position(|key| key.0 == schema_name && key.1 == table)
            else {
                continue;
            };
            if let (Some(to_schema), Some(to_table)) = (text(to_schema), text(to_table))
                && !seen(&to_schema, &to_table)
            {
                continue;
            }
            tables[at].constraints.push(definition);
        }
        if exact_counts {
            let counted: Vec<usize> = (0..tables.len())
                .filter(|at| matches!(tables[*at].kind.as_str(), "table" | "partitioned table"))
                .collect();
            if counted.len() > MAX_EXACT_COUNTS {
                return Err(AppError::new(
                    ErrorCategory::Configuration,
                    format!(
                        "exact counts read every row of {} tables, {MAX_EXACT_COUNTS} at most: name a schema",
                        counted.len()
                    ),
                ));
            }
            for at in counted {
                let (schema_name, table) = &keys[at];
                let sql = format!(
                    "SELECT count(*) FROM {}.{}",
                    quote(connection.dialect, schema_name),
                    quote(connection.dialect, table)
                );
                let (rows, _) = self.catalog_rows(session, &sql).await?;
                tables[at].rows = rows
                    .first()
                    .and_then(|row| row.first())
                    .and_then(text)
                    .and_then(|count| count.parse().ok());
            }
        }
        Ok(Overview { tables, truncated })
    }

    /// Rows of one of Dexo's own catalog queries, and whether there were more.
    async fn catalog_rows(
        &self,
        session: &dyn Session,
        sql: &str,
    ) -> Result<(Vec<Vec<DbValue>>, bool), AppError> {
        let mut request = QueryRequest::read(sql, MAX_CATALOG_ROWS + 1);
        request.timeout = Duration::from_secs(self.profile.limits.timeout_secs);
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut rows = Vec::new();
        while let Some(event) = stream.next().await {
            if let QueryEvent::Rows(batch) = event.map_err(map_driver_error)? {
                rows.extend(batch.rows);
            }
        }
        let truncated = rows.len() as u64 > MAX_CATALOG_ROWS;
        rows.truncate(MAX_CATALOG_ROWS as usize);
        Ok((rows, truncated))
    }
}

fn text(value: &DbValue) -> Option<String> {
    match value {
        DbValue::Null => None,
        other => Some(display_value(other)),
    }
}

fn is_true(value: &DbValue) -> bool {
    match value {
        DbValue::Bool(value) => *value,
        other => text(other).is_some_and(|text| {
            matches!(
                text.to_ascii_lowercase().as_str(),
                "yes" | "true" | "t" | "1"
            )
        }),
    }
}

fn kind_words(kind: &str) -> &'static str {
    match kind {
        "r" | "BASE TABLE" => "table",
        "p" => "partitioned table",
        "v" | "VIEW" | "SYSTEM VIEW" => "view",
        "m" => "materialized view",
        "f" => "foreign table",
        _ => "relation",
    }
}

/// `name` as one identifier of `dialect`.
fn quote(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::Mysql => format!("`{}`", name.replace('`', "``")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// schema, table, kind, estimated rows, column, type, nullable, default -- partitions
/// are left to their parent.
const POSTGRES_COLUMNS: &str = "SELECT n.nspname, c.relname, c.relkind::text, c.reltuples::bigint,
       a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull,
       pg_get_expr(d.adbin, d.adrelid)
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND NOT c.relispartition
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%'
ORDER BY n.nspname, c.relname, a.attnum";

/// schema, table, `name definition`, and the schema and table a foreign key references.
const POSTGRES_CONSTRAINTS: &str = "SELECT n.nspname, c.relname,
       con.conname || ' ' || pg_get_constraintdef(con.oid), fn.nspname, fc.relname
FROM pg_constraint con
JOIN pg_class c ON c.oid = con.conrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
LEFT JOIN pg_class fc ON fc.oid = con.confrelid
LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace
WHERE con.contype IN ('p', 'u', 'f', 'c', 'x')
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
ORDER BY n.nspname, c.relname, con.contype = 'p' DESC, con.conname";

/// The MySQL columns of the same shape: the database stands for the schema.
const MYSQL_COLUMNS: &str = "SELECT c.TABLE_SCHEMA, c.TABLE_NAME, t.TABLE_TYPE, t.TABLE_ROWS,
       c.COLUMN_NAME, c.COLUMN_TYPE, c.IS_NULLABLE, c.COLUMN_DEFAULT
FROM information_schema.COLUMNS c
JOIN information_schema.TABLES t
  ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
WHERE c.TABLE_SCHEMA NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME, c.ORDINAL_POSITION";

/// MySQL's keys, one definition each: its columns, and what a foreign key references.
const MYSQL_CONSTRAINTS: &str = "SELECT k.TABLE_SCHEMA, k.TABLE_NAME,
       CONCAT(k.CONSTRAINT_NAME, ' ', tc.CONSTRAINT_TYPE, ' (',
              GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', '), ')',
              IF(MAX(k.REFERENCED_TABLE_NAME) IS NULL, '',
                 CONCAT(' REFERENCES ', MAX(k.REFERENCED_TABLE_NAME), ' (',
                        GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ', '),
                        ')'))),
       MAX(k.REFERENCED_TABLE_SCHEMA), MAX(k.REFERENCED_TABLE_NAME)
FROM information_schema.KEY_COLUMN_USAGE k
JOIN information_schema.TABLE_CONSTRAINTS tc
  ON tc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND tc.TABLE_NAME = k.TABLE_NAME
 AND tc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
WHERE k.TABLE_SCHEMA NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
GROUP BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE
ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, tc.CONSTRAINT_TYPE = 'PRIMARY KEY' DESC, k.CONSTRAINT_NAME";

#[cfg(test)]
mod tests {
    use dexo_driver_api::DbValue;
    use dexo_test_support::FakeSession;

    use crate::mcp::connection::McpConnection;
    use crate::mcp::profile::McpProfile;
    use crate::mcp::selector::{Effect, SelectorRule};
    use crate::mcp::service::McpService;

    fn service() -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        McpService::new(profile)
    }

    fn connection() -> McpConnection {
        McpConnection {
            name: "local".into(),
            driver: "postgres".into(),
            dialect: dexo_sql::Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: crate::Environment::Local,
            read_only: false,
        }
    }

    fn column(table: &str, name: &str, kind: &str, default: Option<&str>) -> Vec<DbValue> {
        vec![
            DbValue::Text("public".into()),
            DbValue::Text(table.into()),
            DbValue::Text("r".into()),
            DbValue::I64(12),
            DbValue::Text(name.into()),
            DbValue::Text(kind.into()),
            DbValue::Bool(default.is_none()),
            default.map_or(DbValue::Null, |text| DbValue::Text(text.into())),
        ]
    }

    fn constraint(table: &str, definition: &str, references: Option<&str>) -> Vec<DbValue> {
        vec![
            DbValue::Text("public".into()),
            DbValue::Text(table.into()),
            DbValue::Text(definition.into()),
            references.map_or(DbValue::Null, |_| DbValue::Text("public".into())),
            references.map_or(DbValue::Null, |table| DbValue::Text(table.into())),
        ]
    }

    /// One read shows every table the profile sees with its columns, defaults, keys and
    /// counts -- and nothing of a hidden one, not even a foreign key's name for it.
    #[tokio::test]
    async fn the_overview_shows_what_the_profile_sees_and_names_nothing_hidden() {
        let session = FakeSession::default()
            .answering(
                "FROM pg_class c",
                vec![
                    column("orders", "id", "integer", Some("nextval('orders_id_seq')")),
                    column("orders", "note", "text", None),
                    column("secrets", "token", "text", None),
                ],
            )
            .answering(
                "FROM pg_constraint",
                vec![
                    constraint("orders", "orders_pkey PRIMARY KEY (id)", None),
                    constraint(
                        "orders",
                        "orders_secret_fkey FOREIGN KEY (secret) REFERENCES secrets(id)",
                        Some("secrets"),
                    ),
                    constraint("secrets", "secrets_pkey PRIMARY KEY (token)", None),
                ],
            )
            .answering("count(*)", vec![vec![DbValue::I64(40)]]);
        let overview = service()
            .overview(&session, &connection(), Some("public"), true)
            .await
            .unwrap();
        assert_eq!(overview.tables.len(), 1);
        let orders = &overview.tables[0];
        assert_eq!(orders.name, "db.public.orders");
        assert_eq!((orders.estimated_rows, orders.rows), (Some(12), Some(40)));
        assert_eq!(orders.columns.len(), 2);
        assert_eq!(
            orders.columns[0].default.as_deref(),
            Some("nextval('orders_id_seq')")
        );
        assert!(!orders.columns[0].nullable && orders.columns[1].nullable);
        assert_eq!(orders.constraints, ["orders_pkey PRIMARY KEY (id)"]);
        let log = session.log();
        assert_eq!(log.first().map(String::as_str), Some("begin ReadOnly"));
        assert_eq!(log.last().map(String::as_str), Some("rollback"));
        assert!(
            log.iter()
                .any(|entry| entry == "execute SELECT count(*) FROM \"public\".\"orders\"")
        );
        assert!(
            !log.iter().any(|entry| entry.contains("secrets")),
            "{log:?}"
        );
        // Another schema shows nothing of this one.
        let elsewhere = service()
            .overview(&session, &connection(), Some("billing"), false)
            .await
            .unwrap();
        assert!(elsewhere.tables.is_empty());
    }
}
