use dexo_driver_api::{
    BulkWriter, ColumnId, ColumnKeyInfo, DataMutator, DataPage, DataRequest, DbValue, DriverError,
    DriverErrorCategory, Filter, Mutation, Page, QualifiedName, RemoteValueRef, Sort,
};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::decode::{column_meta, decode, quote, to_sql};
use crate::error::map_error;
use crate::session::SqliteSession;

/// The database a name lives in: its catalog or schema part, whichever the caller filled
/// in (the explorer names `main.orders` as a schema; a snapshot parsed from text may not),
/// and `main` when neither.
fn schema_of(name: &QualifiedName) -> &str {
    name.schema().or(name.catalog()).unwrap_or("main")
}

fn qualify(name: &QualifiedName) -> String {
    format!("{}.{}", quote(schema_of(name)), quote(name.object()))
}

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

fn render_fetch(request: &DataRequest, rowid: Option<&str>) -> (String, Binder) {
    let mut binder = Binder::default();
    let cols = if request.columns.is_empty() {
        match rowid {
            Some(alias) => format!("{alias}, *"),
            None => "*".to_string(),
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
    if let Some(filter) = &request.filter {
        sql.push_str(" WHERE ");
        sql.push_str(&render_filter(filter, &mut binder));
    }
    if !request.sort.is_empty() {
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
    sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
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
    // ponytail: cap after fetch; ceiling: large cells are still materialized once. Upgrade: substr() in SELECT by column type.
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
/// carries as its first column. A view and a WITHOUT ROWID table (which must have a
/// primary key) never need it. SQLite answers to `rowid`, `_rowid_` and `oid`, unless a
/// real column has the name; the first one free is the key, and with none free the
/// table has no key to edit by -- a column called `rowid` is the user's, not unique.
fn rowid_alias(
    conn: &Connection,
    name: &QualifiedName,
) -> Result<Option<&'static str>, DriverError> {
    let sql = format!(
        "SELECT type = 'table' AND NOT EXISTS (SELECT 1 FROM pragma_table_info(?1, ?2) WHERE pk > 0)
         FROM {}.sqlite_master WHERE name = ?1",
        quote(schema_of(name))
    );
    let keyless = conn
        .query_row(&sql, params![name.object(), schema_of(name)], |row| {
            row.get::<_, bool>(0)
        })
        .optional()
        .map_err(map_error)?
        .unwrap_or(false);
    if !keyless {
        return Ok(None);
    }
    let mut statement = conn
        .prepare("SELECT name FROM pragma_table_xinfo(?1, ?2)")
        .map_err(map_error)?;
    let taken = statement
        .query_map(params![name.object(), schema_of(name)], |row| {
            row.get::<_, String>(0)
        })
        .map_err(map_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(map_error)?;
    Ok(["rowid", "_rowid_", "oid"].into_iter().find(|alias| {
        !taken
            .iter()
            .any(|column| column.eq_ignore_ascii_case(alias))
    }))
}

fn table_keys(conn: &Connection, name: &QualifiedName) -> Result<Vec<ColumnKeyInfo>, DriverError> {
    let mut statement = conn
        .prepare(
            "SELECT c.name, c.pk > 0, EXISTS (
                 SELECT 1 FROM pragma_index_list(?1, ?2) l
                 JOIN pragma_index_info(l.name, ?2) i
                 WHERE l.\"unique\" AND i.name = c.name
             )
             FROM pragma_table_xinfo(?1, ?2) c WHERE c.hidden <> 1 ORDER BY c.cid",
        )
        .map_err(map_error)?;
    let mut keys = statement
        .query_map(params![name.object(), schema_of(name)], |row| {
            Ok(ColumnKeyInfo {
                name: row.get(0)?,
                primary_key: row.get(1)?,
                unique: row.get(2)?,
            })
        })
        .map_err(map_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(map_error)?;
    if let Some(alias) = rowid_alias(conn, name)? {
        keys.insert(
            0,
            ColumnKeyInfo {
                name: alias.into(),
                primary_key: true,
                unique: true,
            },
        );
    }
    Ok(keys)
}

/// The mutations in one savepoint, so they land together or not at all, inside a
/// transaction the user has open as well as outside one.
fn apply_all(conn: &mut Connection, mutations: &[Mutation]) -> Result<(), DriverError> {
    let savepoint = conn.savepoint_with_name("dexo_apply").map_err(map_error)?;
    for mutation in mutations {
        let (sql, binder) = render_mutation(mutation);
        let affected = savepoint
            .execute(&sql, params_from_iter(binder.values))
            .map_err(map_error)?;
        if !matches!(mutation, Mutation::Insert { .. }) && affected != 1 {
            return Err(DriverError::new(
                DriverErrorCategory::Conflict,
                format!("mutation conflict: expected to affect exactly 1 row, affected {affected}"),
            ));
        }
    }
    savepoint.commit().map_err(map_error)
}

#[async_trait::async_trait]
impl DataMutator for SqliteSession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        Page::new(request.page.offset, request.page.limit)?;
        request.validate()?;
        self.with_conn(move |conn| {
            let rowid = if request.columns.is_empty() {
                rowid_alias(conn, &request.object)?
            } else {
                None
            };
            let (sql, binder) = render_fetch(&request, rowid);
            let mut statement = conn.prepare(&sql).map_err(map_error)?;
            let columns: Vec<_> = statement.columns().iter().map(column_meta).collect();
            let width = columns.len();
            let rows = statement
                .query_map(params_from_iter(binder.values), |row| {
                    Ok((0..width)
                        .map(|i| cap_value(decode(row.get_ref_unwrap(i))))
                        .collect::<Vec<_>>())
                })
                .map_err(map_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(map_error)?;
            Ok(DataPage::from_fetched(
                columns,
                rows,
                request.page.offset,
                request.page.limit,
            ))
        })
        .await
    }

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
        let start = binder.push(&DbValue::I64(offset.saturating_add(1) as i64));
        let len = binder.push(&DbValue::I64(i64::from(limit)));
        let pred = predicate(value.identity.iter(), &mut binder);
        let sql = format!(
            "SELECT substr({}, {start}, {len}) FROM {} WHERE {pred}",
            quote(&value.column.0),
            qualify(&value.object)
        );
        self.with_conn(move |conn| {
            conn.query_row(&sql, params_from_iter(binder.values), |row| {
                // substr() answers text or a blob, or NULL past the end.
                Ok(match row.get_ref(0)? {
                    ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes.to_vec(),
                    _ => Vec::new(),
                })
            })
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(map_error)
        })
        .await
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
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
impl BulkWriter for SqliteSession {
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
