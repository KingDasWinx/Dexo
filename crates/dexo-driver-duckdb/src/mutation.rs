use dexo_driver_api::{
    BulkWriter, ColumnId, ColumnKeyInfo, ColumnMeta, DataMutator, DataPage, DataRequest, DbValue,
    DriverError, DriverErrorCategory, Filter, Mutation, Page, QualifiedName, RemoteValueRef, Sort,
};
use duckdb::types::Value;
use duckdb::{Connection, OptionalExt, params};

use crate::catalog::{place_of, rows};
use crate::decode::{column_meta, qualify, quote, to_sql};
use crate::error::{map_error, writes_refused};
use crate::session::{DuckdbSession, begin_own, decode_rows};

#[derive(Default)]
struct Binder {
    values: Vec<Value>,
}

impl Binder {
    fn push(&mut self, value: &DbValue) -> &'static str {
        self.values.push(to_sql(value));
        "?"
    }
}

fn render_filter(filter: &Filter, binder: &mut Binder) -> String {
    match filter {
        Filter::Eq(column, value) => format!("{} = {}", quote(&column.0), binder.push(value)),
        Filter::Ne(column, value) => format!("{} <> {}", quote(&column.0), binder.push(value)),
        Filter::Gt(column, value) => format!("{} > {}", quote(&column.0), binder.push(value)),
        Filter::Gte(column, value) => format!("{} >= {}", quote(&column.0), binder.push(value)),
        Filter::Lt(column, value) => format!("{} < {}", quote(&column.0), binder.push(value)),
        Filter::Lte(column, value) => format!("{} <= {}", quote(&column.0), binder.push(value)),
        Filter::IsNull(column) => format!("{} IS NULL", quote(&column.0)),
        Filter::IsNotNull(column) => format!("{} IS NOT NULL", quote(&column.0)),
        Filter::And(parts) => wrap(parts, " AND ", binder),
        Filter::Or(parts) => wrap(parts, " OR ", binder),
        Filter::Not(inner) => format!("NOT ({})", render_filter(inner, binder)),
    }
}

fn wrap(parts: &[Filter], sep: &str, binder: &mut Binder) -> String {
    format!(
        "({})",
        parts
            .iter()
            .map(|part| render_filter(part, binder))
            .collect::<Vec<_>>()
            .join(sep)
    )
}

fn render_fetch(request: &DataRequest, rowid: bool) -> (String, Binder) {
    let mut binder = Binder::default();
    let cols = if request.columns.is_empty() {
        if rowid {
            "rowid AS \"rowid\", *".to_string()
        } else {
            "*".to_string()
        }
    } else {
        request
            .columns
            .iter()
            .map(|column| quote(&column.0))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut sql = format!("SELECT {cols} FROM {}", qualify(&request.object));
    let typed = request
        .filter
        .as_ref()
        .map(|filter| render_filter(filter, &mut binder));
    if let Some(condition) = request.clauses.condition(typed) {
        sql.push_str(" WHERE ");
        sql.push_str(&condition);
    }
    if let Some(order) = request.clauses.order() {
        sql.push_str(" ORDER BY ");
        sql.push_str(order);
    } else if !request.sort.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(
            &request
                .sort
                .iter()
                .map(|Sort { column, descending }| {
                    format!(
                        "{} {}",
                        quote(&column.0),
                        if *descending { "DESC" } else { "ASC" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    // On a line of its own: a comment ending the ORDER BY text would take it otherwise.
    sql.push_str(&format!(
        "\nLIMIT {} OFFSET {}",
        request.page.limit.saturating_add(1),
        request.page.offset
    ));
    (sql, binder)
}

fn render_mutation(mutation: &Mutation) -> (String, Binder) {
    let mut binder = Binder::default();
    let sql = match mutation {
        Mutation::Insert {
            table,
            columns,
            values,
        } => {
            let cols = columns
                .iter()
                .map(|column| quote(&column.0))
                .collect::<Vec<_>>()
                .join(", ");
            let slots = values
                .iter()
                .map(|value| binder.push(value))
                .collect::<Vec<_>>()
                .join(", ");
            format!("INSERT INTO {} ({cols}) VALUES ({slots})", qualify(table))
        }
        Mutation::Update {
            table,
            identity,
            original,
            changes,
        } => {
            let set = changes
                .iter()
                .map(|(column, value)| format!("{} = {}", quote(&column.0), binder.push(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "UPDATE {} SET {set} WHERE {}",
                qualify(table),
                predicate(identity.iter().chain(original), &mut binder)
            )
        }
        Mutation::Delete {
            table,
            identity,
            original,
        } => format!(
            "DELETE FROM {} WHERE {}",
            qualify(table),
            predicate(identity.iter().chain(original), &mut binder)
        ),
    };
    (sql, binder)
}

fn predicate<'a>(
    columns: impl Iterator<Item = &'a (ColumnId, DbValue)>,
    binder: &mut Binder,
) -> String {
    columns
        .map(|(column, value)| match value {
            DbValue::Null => format!("{} IS NULL", quote(&column.0)),
            _ => format!("{} = {}", quote(&column.0), binder.push(value)),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn cap_value(value: DbValue) -> DbValue {
    // ponytail: cap after fetch; ceiling: large cells are still materialized once. Upgrade: substring() in SELECT by column type.
    const CAP: usize = 64 * 1024;
    match value {
        DbValue::Bytes(bytes) if bytes.len() > CAP => DbValue::Native {
            type_name: "truncated".into(),
            text: bytes.len().to_string(),
            bytes: bytes[..CAP].to_vec(),
        },
        DbValue::Text(text) if text.len() > CAP => DbValue::Native {
            type_name: "truncated-text".into(),
            text: text.len().to_string(),
            bytes: text.as_bytes()[..CAP].to_vec(),
        },
        other => other,
    }
}

/// A table with no primary key is still keyed: by its rowid, which the page then
/// carries as its first column. A view has none, and a column the user called `rowid`
/// hides DuckDB's, which leaves the table no key to edit by.
fn keyed_by_rowid(conn: &Connection, name: &QualifiedName) -> Result<bool, DriverError> {
    let (database, schema) = place_of(conn, name)?;
    conn.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM duckdb_constraints() k
                 WHERE k.database_name = t.database_name AND k.schema_name = t.schema_name
                   AND k.table_name = t.table_name AND k.constraint_type = 'PRIMARY KEY')
            AND NOT EXISTS (SELECT 1 FROM duckdb_columns() c
                 WHERE c.database_name = t.database_name AND c.schema_name = t.schema_name
                   AND c.table_name = t.table_name AND lower(c.column_name) = 'rowid')
         FROM duckdb_tables() t
         WHERE lower(t.database_name) = lower($1) AND lower(t.schema_name) = lower($2)
           AND lower(t.table_name) = lower($3)",
        params![database, schema, name.object()],
        |row| row.get::<_, bool>(0),
    )
    .optional()
    .map(|keyless| keyless.unwrap_or(false))
    .map_err(map_error)
}

fn table_keys(conn: &Connection, name: &QualifiedName) -> Result<Vec<ColumnKeyInfo>, DriverError> {
    let (database, schema) = place_of(conn, name)?;
    let place = |sql: &str| {
        sql.replace(
            "$WHERE",
            "lower(database_name) = lower($1) AND lower(schema_name) = lower($2) AND lower(table_name) = lower($3)",
        )
    };
    let columns = rows(
        conn,
        &place("SELECT column_name FROM duckdb_columns() WHERE $WHERE ORDER BY column_index"),
        params![database, schema, name.object()],
        |row| row.get::<_, String>(0),
    )?;
    let keys = rows(
        conn,
        &place(
            "SELECT constraint_type, to_json(constraint_column_names)::VARCHAR FROM duckdb_constraints()
             WHERE $WHERE AND constraint_type IN ('PRIMARY KEY', 'UNIQUE')",
        ),
        params![database, schema, name.object()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let keys: Vec<(bool, Vec<String>)> = keys
        .into_iter()
        .map(|(kind, columns)| {
            (
                kind == "PRIMARY KEY",
                serde_json::from_str(&columns).unwrap_or_default(),
            )
        })
        .collect();
    let mut infos: Vec<ColumnKeyInfo> = columns
        .into_iter()
        .map(|column| ColumnKeyInfo {
            primary_key: keys
                .iter()
                .any(|(primary, columns)| *primary && columns.contains(&column)),
            unique: keys
                .iter()
                .any(|(_, columns)| columns == std::slice::from_ref(&column)),
            name: column,
        })
        .collect();
    if keyed_by_rowid(conn, name)? {
        infos.insert(
            0,
            ColumnKeyInfo {
                name: "rowid".into(),
                primary_key: true,
                unique: true,
            },
        );
    }
    Ok(infos)
}

/// Runs a SELECT Dexo wrote around the user's clauses: in a read-only transaction rolled
/// back after it, unless the user has one open.
fn read_rows(
    conn: &Connection,
    sql: &str,
    values: Vec<Value>,
) -> Result<(Vec<ColumnMeta>, Vec<Vec<DbValue>>), DriverError> {
    let fenced = begin_own(conn, "BEGIN TRANSACTION READ ONLY")?;
    let read = (|| {
        let mut statement = conn.prepare(sql).map_err(map_error)?;
        let _ = statement
            .stream_arrow(duckdb::params_from_iter(values))
            .map_err(map_error)?;
        let schema = statement.schema();
        let columns: Vec<ColumnMeta> = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(index, field)| column_meta(field.name(), &statement.column_logical_type(index)))
            .collect();
        let type_names: Vec<String> = columns
            .iter()
            .map(|column| column.type_name.clone())
            .collect();
        let mut rows = Vec::new();
        while let Some(chunk) = statement.step().map_err(map_error)? {
            rows.extend(decode_rows(chunk.columns(), &type_names)?);
        }
        Ok((columns, rows))
    })();
    if fenced {
        conn.execute_batch("ROLLBACK").map_err(map_error)?;
    }
    read
}

/// The mutations together or not at all: in a transaction of their own, committed after
/// the last. Inside the user's, DuckDB has no savepoint to fence them with, so they join
/// it, and a failure says how to undo the ones before it.
fn apply_all(conn: &Connection, mutations: &[Mutation]) -> Result<(), DriverError> {
    let own = begin_own(conn, "BEGIN TRANSACTION")?;
    let applied = mutations.iter().try_for_each(|mutation| {
        let (sql, binder) = render_mutation(mutation);
        let affected = conn
            .execute(&sql, duckdb::params_from_iter(binder.values))
            .map_err(map_error)?;
        if !matches!(mutation, Mutation::Insert { .. }) && affected != 1 {
            return Err(DriverError::new(
                DriverErrorCategory::Conflict,
                format!("mutation conflict: expected to affect exactly 1 row, affected {affected}"),
            ));
        }
        Ok(())
    });
    match (own, applied) {
        (true, Ok(())) => conn.execute_batch("COMMIT").map_err(map_error),
        (true, Err(error)) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
        (false, Err(error)) => Err(error.with_hint(
            "DuckDB has no savepoints: roll back the open transaction to undo the edits applied before this one",
        )),
        (false, Ok(())) => Ok(()),
    }
}

#[async_trait::async_trait]
impl DataMutator for DuckdbSession {
    /// DuckDB's own count of a table's rows, kept as it writes them.
    async fn estimate_rows(&self, target: &QualifiedName) -> Result<Option<u64>, DriverError> {
        let target = target.clone();
        self.with_conn(move |conn| {
            let (database, schema) = place_of(conn, &target)?;
            conn.query_row(
                "SELECT estimated_size FROM duckdb_tables()
                 WHERE lower(database_name) = lower($1) AND lower(schema_name) = lower($2)
                   AND lower(table_name) = lower($3)",
                params![database, schema, target.object()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(|size| size.flatten().and_then(|size| u64::try_from(size).ok()))
            .map_err(map_error)
        })
        .await
    }

    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        Page::new(request.page.offset, request.page.limit)?;
        request.validate()?;
        self.with_conn(move |conn| {
            let rowid = request.columns.is_empty() && keyed_by_rowid(conn, &request.object)?;
            let (sql, binder) = render_fetch(&request, rowid);
            let (columns, rows) = read_rows(conn, &sql, binder.values)?;
            let rows = rows
                .into_iter()
                .map(|row| row.into_iter().map(cap_value).collect())
                .collect();
            Ok(DataPage::from_fetched(
                columns,
                rows,
                request.page.offset,
                request.page.limit,
            ))
        })
        .await
    }

    /// The bytes of one cell from `offset`, text as its UTF-8.
    async fn fetch_value(
        &self,
        value: &RemoteValueRef,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, DriverError> {
        if value.identity.is_empty() {
            return Err(DriverError::unsupported(
                "remote value requires a stable row identity",
            ));
        }
        let mut binder = Binder::default();
        let pred = predicate(value.identity.iter(), &mut binder);
        // ponytail: the whole cell is read for each page of it; ceiling: a cell of many
        // MB read page by page. Upgrade: substring() by the column's type.
        let sql = format!(
            "SELECT {} FROM {} WHERE {pred}",
            quote(&value.column.0),
            qualify(&value.object)
        );
        self.with_conn(move |conn| {
            let (_, rows) = read_rows(conn, &sql, binder.values)?;
            let bytes = match rows
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
            {
                Some(DbValue::Bytes(bytes)) => bytes,
                Some(DbValue::Text(text) | DbValue::Json(text) | DbValue::Decimal(text)) => {
                    text.into_bytes()
                }
                Some(DbValue::Native { text, .. }) => text.into_bytes(),
                _ => Vec::new(),
            };
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            let end = start.saturating_add(limit as usize).min(bytes.len());
            Ok(bytes[start..end].to_vec())
        })
        .await
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        if self.read_only() {
            return Err(writes_refused());
        }
        let mutations = mutations.to_vec();
        self.with_conn(move |conn| apply_all(conn, &mutations))
            .await
    }

    async fn table_columns(
        &self,
        target: &QualifiedName,
    ) -> Result<Vec<ColumnKeyInfo>, DriverError> {
        let target = target.clone();
        self.with_conn(move |conn| table_keys(conn, &target)).await
    }
}

#[async_trait::async_trait]
impl BulkWriter for DuckdbSession {
    async fn insert_batch(
        &self,
        table: &QualifiedName,
        columns: &[String],
        rows: &[Vec<DbValue>],
    ) -> Result<u64, DriverError> {
        let mutations: Vec<Mutation> = rows
            .iter()
            .map(|values| Mutation::Insert {
                table: table.clone(),
                columns: columns.iter().cloned().map(ColumnId).collect(),
                values: values.clone(),
            })
            .collect();
        self.apply(&mutations).await?;
        Ok(rows.len() as u64)
    }
}
