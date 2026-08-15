use dexo_driver_api::{
    ColumnId, DataMutator, DataPage, DataRequest, DbValue, DriverError, DriverErrorCategory,
    Filter, Mutation, Page, QualifiedName, Sort,
};
use tokio_postgres::types::ToSql;

use crate::decode::{column_meta, decode_row};
use crate::error::map_error;
use crate::session::PostgresSession;

fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
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

fn to_box(value: &DbValue) -> Box<dyn ToSql + Sync + Send> {
    match value {
        DbValue::Null => Box::new(Option::<i32>::None),
        DbValue::Bool(value) => Box::new(*value),
        DbValue::I64(value) => Box::new(*value),
        DbValue::U64(value) => Box::new(i64::try_from(*value).unwrap_or(i64::MAX)),
        DbValue::Decimal(value) | DbValue::Text(value) | DbValue::Json(value) => {
            Box::new(value.clone())
        }
        DbValue::Bytes(value) => Box::new(value.clone()),
        DbValue::Native { text, .. } => Box::new(text.clone()),
    }
}

struct Binder {
    values: Vec<DbValue>,
}

impl Binder {
    fn new() -> Self {
        Self { values: Vec::new() }
    }

    fn push(&mut self, value: DbValue) -> String {
        // ponytail: explicit casts so tokio-postgres doesn't send unknown OIDs (int vs bigint). Ceiling: typed column metadata.
        let cast = match &value {
            DbValue::I64(_) | DbValue::U64(_) => "::bigint",
            DbValue::Bool(_) => "::bool",
            DbValue::Bytes(_) => "::bytea",
            DbValue::Json(_) => "::jsonb",
            _ => "",
        };
        self.values.push(value);
        format!("${}{cast}", self.values.len())
    }

    fn boxed(&self) -> Vec<Box<dyn ToSql + Sync + Send>> {
        self.values.iter().map(to_box).collect()
    }
}

fn render_filter(filter: &Filter, binder: &mut Binder) -> String {
    match filter {
        Filter::Eq(column, value) => {
            format!("{} = {}", quote(&column.0), binder.push(value.clone()))
        }
        Filter::Ne(column, value) => {
            format!("{} <> {}", quote(&column.0), binder.push(value.clone()))
        }
        Filter::Gt(column, value) => {
            format!("{} > {}", quote(&column.0), binder.push(value.clone()))
        }
        Filter::Gte(column, value) => {
            format!("{} >= {}", quote(&column.0), binder.push(value.clone()))
        }
        Filter::Lt(column, value) => {
            format!("{} < {}", quote(&column.0), binder.push(value.clone()))
        }
        Filter::Lte(column, value) => {
            format!("{} <= {}", quote(&column.0), binder.push(value.clone()))
        }
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

fn render_fetch(request: &DataRequest) -> Result<(String, Binder), DriverError> {
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
        binder.push(DbValue::I64(i64::from(request.page.limit) + 1)),
        binder.push(DbValue::I64(request.page.offset as i64))
    ));
    Ok((sql, binder))
}

fn render_mutation(mutation: &Mutation) -> Result<(String, Binder), DriverError> {
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
                .map(|value| binder.push(value.clone()))
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
                .map(|(column, value)| {
                    format!("{} = {}", quote(&column.0), binder.push(value.clone()))
                })
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
    Ok((sql, binder))
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
            _ => format!("{} = {}", quote(&column.0), binder.push(value.clone())),
        })
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn cap_row(row: Vec<DbValue>) -> Vec<DbValue> {
    row.into_iter().map(cap_value).collect()
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
            _ => format!("{} = {}", quote(&column.0), binder.push(value.clone())),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[async_trait::async_trait]
impl DataMutator for PostgresSession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        let _ = Page::new(request.page.offset, request.page.limit)?;
        request.validate()?;
        let (sql, binder) = render_fetch(&request)?;
        let boxed = binder.boxed();
        let refs: Vec<&(dyn ToSql + Sync)> =
            boxed.iter().map(|value| value.as_ref() as _).collect();
        let rows = self.client.query(&sql, &refs).await.map_err(map_error)?;
        let columns = rows
            .first()
            .map(|row| row.columns().iter().map(column_meta).collect())
            .unwrap_or_else(|| {
                request
                    .columns
                    .iter()
                    .map(|column| dexo_driver_api::ColumnMeta {
                        name: column.0.clone(),
                        type_name: "unknown".into(),
                        nullable: true,
                    })
                    .collect()
            });
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
        let start = binder.push(DbValue::I64(offset.saturating_add(1) as i64));
        let len = binder.push(DbValue::I64(i64::from(limit)));
        let pred = identity_predicate(&value.identity, &mut binder)?;
        let sql = format!(
            "SELECT substring({} from {start} for {len}) FROM {} WHERE {pred}",
            quote(&value.column.0),
            qualify(&value.object)
        );
        let boxed = binder.boxed();
        let refs: Vec<&(dyn ToSql + Sync)> =
            boxed.iter().map(|value| value.as_ref() as _).collect();
        let row = self
            .client
            .query_opt(&sql, &refs)
            .await
            .map_err(map_error)?;
        let Some(row) = row else {
            return Ok(Vec::new());
        };
        if let Ok(bytes) = row.try_get::<_, Vec<u8>>(0) {
            Ok(bytes)
        } else if let Ok(text) = row.try_get::<_, String>(0) {
            Ok(text.into_bytes())
        } else {
            Ok(Vec::new())
        }
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        self.client
            .batch_execute("BEGIN")
            .await
            .map_err(map_error)?;
        let result = apply_inner(self, mutations).await;
        match result {
            Ok(()) => self.client.batch_execute("COMMIT").await.map_err(map_error),
            Err(error) => {
                let _ = self.client.batch_execute("ROLLBACK").await;
                Err(error)
            }
        }
    }
}

#[async_trait::async_trait]
impl dexo_driver_api::BulkWriter for PostgresSession {
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

async fn apply_inner(session: &PostgresSession, mutations: &[Mutation]) -> Result<(), DriverError> {
    for mutation in mutations {
        let (sql, binder) = render_mutation(mutation)?;
        let boxed = binder.boxed();
        let refs: Vec<&(dyn ToSql + Sync)> =
            boxed.iter().map(|value| value.as_ref() as _).collect();
        let affected = session
            .client
            .execute(&sql, &refs)
            .await
            .map_err(map_error)?;
        if !matches!(mutation, Mutation::Insert { .. }) && affected != 1 {
            return Err(DriverError::new(
                DriverErrorCategory::Conflict,
                "mutation conflict",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn postgres_quote_wraps_and_escapes() {
        assert_eq!(quote("id"), "\"id\"");
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote("x;drop"), "\"x;drop\"");
    }
}
