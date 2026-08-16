use dexo_driver_api::{
    ColumnId, DataMutator, DataPage, DataRequest, DbValue, DriverError, DriverErrorCategory,
    Filter, Mutation, Page, QualifiedName, Sort,
};
use mysql_async::prelude::Queryable;
use mysql_async::{Params, Value};

use crate::decode::{column_meta, decode_row};
use crate::error::map_error;
use crate::session::MysqlSession;

fn quote(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

fn qualify(name: &QualifiedName) -> String {
    let mut parts = Vec::new();
    if let Some(catalog) = name.catalog() {
        parts.push(quote(catalog));
    }
    if let Some(schema) = name.schema() {
        parts.push(quote(schema));
    }
    parts.push(quote(name.object()));
    parts.join(".")
}

fn to_value(value: &DbValue) -> Value {
    match value {
        DbValue::Null => Value::NULL,
        DbValue::Bool(value) => Value::Int(i64::from(*value)),
        DbValue::I64(value) => Value::Int(*value),
        DbValue::U64(value) => Value::UInt(*value),
        DbValue::Decimal(value) | DbValue::Text(value) | DbValue::Json(value) => {
            Value::Bytes(value.as_bytes().to_vec())
        }
        DbValue::Bytes(value) => Value::Bytes(value.clone()),
        DbValue::Native { text, .. } => Value::Bytes(text.as_bytes().to_vec()),
    }
}

struct Binder {
    values: Vec<Value>,
}

impl Binder {
    fn new() -> Self {
        Self { values: Vec::new() }
    }

    fn push(&mut self, value: &DbValue) -> &'static str {
        self.values.push(to_value(value));
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

fn render_fetch(request: &DataRequest) -> (String, Binder) {
    let mut binder = Binder::new();
    let cols = if request.columns.is_empty() {
        "*".into()
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
    let mut binder = Binder::new();
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
                predicate(identity, original, &mut binder)
            )
        }
        Mutation::Delete {
            table,
            identity,
            original,
        } => format!(
            "DELETE FROM {} WHERE {}",
            qualify(table),
            predicate(identity, original, &mut binder)
        ),
    };
    (sql, binder)
}

fn predicate(
    identity: &[(ColumnId, DbValue)],
    original: &[(ColumnId, DbValue)],
    binder: &mut Binder,
) -> String {
    identity
        .iter()
        .chain(original.iter())
        .map(|(column, value)| match value {
            DbValue::Null => format!("{} IS NULL", quote(&column.0)),
            _ => format!("{} = {}", quote(&column.0), binder.push(value)),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn identity_predicate(
    identity: &[(ColumnId, DbValue)],
    binder: &mut Binder,
) -> Result<String, DriverError> {
    if identity.is_empty() {
        return Err(DriverError::unsupported(
            "remote value requires a stable row identity",
        ));
    }
    Ok(identity
        .iter()
        .map(|(column, value)| match value {
            DbValue::Null => format!("{} IS NULL", quote(&column.0)),
            _ => format!("{} = {}", quote(&column.0), binder.push(value)),
        })
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn cap_row(row: Vec<DbValue>) -> Vec<DbValue> {
    row.into_iter().map(cap_value).collect()
}

fn cap_value(value: DbValue) -> DbValue {
    // ponytail: cap after fetch; ceiling: large cells are still materialized once. Upgrade: SUBSTRING() in SELECT by column type.
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

#[async_trait::async_trait]
impl DataMutator for MysqlSession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        let _ = Page::new(request.page.offset, request.page.limit)?;
        request.validate()?;
        let (sql, binder) = render_fetch(&request);
        let mut conn = self.conn.lock().await;
        let rows: Vec<mysql_async::Row> = conn
            .exec(sql, Params::Positional(binder.values))
            .await
            .map_err(map_error)?;
        let columns = rows
            .first()
            .map(|row| row.columns_ref().iter().map(column_meta).collect())
            .unwrap_or_default();
        Ok(DataPage::from_fetched(
            columns,
            rows.iter().map(|row| cap_row(decode_row(row))).collect(),
            request.page.offset,
            request.page.limit,
        ))
    }

    async fn fetch_value(
        &self,
        value: &dexo_driver_api::RemoteValueRef,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, DriverError> {
        if value.identity.is_empty() {
            return Err(DriverError::unsupported(
                "remote value requires a stable row identity",
            ));
        }
        let mut binder = Binder::new();
        let start = binder.push(&DbValue::I64(offset.saturating_add(1) as i64));
        let len = binder.push(&DbValue::I64(i64::from(limit)));
        let pred = identity_predicate(&value.identity, &mut binder)?;
        let sql = format!(
            "SELECT SUBSTRING({} FROM {start} FOR {len}) FROM {} WHERE {pred}",
            quote(&value.column.0),
            qualify(&value.object)
        );
        let mut conn = self.conn.lock().await;
        let row: Option<mysql_async::Row> = conn
            .exec_first(sql, Params::Positional(binder.values))
            .await
            .map_err(map_error)?;
        let Some(row) = row else {
            return Ok(Vec::new());
        };
        Ok(match row.get::<Vec<u8>, _>(0) {
            Some(bytes) => bytes,
            None => row
                .get::<String, _>(0)
                .map(String::into_bytes)
                .unwrap_or_default(),
        })
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        let mut conn = self.conn.lock().await;
        conn.query_drop("BEGIN").await.map_err(map_error)?;
        for mutation in mutations {
            let (sql, binder) = render_mutation(mutation);
            let result = conn.exec_iter(sql, Params::Positional(binder.values)).await;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    let _ = conn.query_drop("ROLLBACK").await;
                    return Err(map_error(error));
                }
            };
            let affected = result.affected_rows();
            drop(result);
            if !matches!(mutation, Mutation::Insert { .. }) && affected != 1 {
                let _ = conn.query_drop("ROLLBACK").await;
                return Err(DriverError::new(
                    DriverErrorCategory::Conflict,
                    "mutation conflict",
                ));
            }
        }
        conn.query_drop("COMMIT").await.map_err(map_error)
    }
}

#[async_trait::async_trait]
impl dexo_driver_api::BulkWriter for MysqlSession {
    async fn insert_batch(
        &self,
        table: &dexo_driver_api::QualifiedName,
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

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn mysql_quote_wraps_and_escapes() {
        assert_eq!(quote("id"), "`id`");
        assert_eq!(quote("a`b"), "`a``b`");
        assert_eq!(quote("x;drop"), "`x;drop`");
    }
}
