use dexo_driver_api::{ColumnId, DbValue, DriverError, Mutation, QualifiedName};

use super::change_set::{ChangeSet, PendingChange, RowIdentity};
use super::copy::{SqlDialect, sql_literal};

pub fn mutations_for(
    table: QualifiedName,
    changes: &ChangeSet,
) -> Result<Vec<Mutation>, DriverError> {
    changes
        .pending()
        .iter()
        .map(|change| match change {
            PendingChange::Insert { values } => Ok(Mutation::Insert {
                table: table.clone(),
                columns: values
                    .iter()
                    .map(|(name, _)| ColumnId(name.clone()))
                    .collect(),
                values: values.iter().map(|(_, value)| value.clone()).collect(),
            }),
            PendingChange::Update {
                identity,
                original,
                values,
            } => Ok(Mutation::Update {
                table: table.clone(),
                identity: zip_identity(identity),
                original: original
                    .iter()
                    .map(|(name, value)| (ColumnId(name.clone()), value.clone()))
                    .collect(),
                changes: values
                    .iter()
                    .map(|(name, value)| (ColumnId(name.clone()), value.clone()))
                    .collect(),
            }),
            PendingChange::Delete { identity, original } => Ok(Mutation::Delete {
                table: table.clone(),
                identity: zip_identity(identity),
                original: original
                    .iter()
                    .map(|(name, value)| (ColumnId(name.clone()), value.clone()))
                    .collect(),
            }),
        })
        .collect()
}

fn zip_identity(
    identity: &super::change_set::RowIdentity,
) -> Vec<(ColumnId, dexo_driver_api::DbValue)> {
    identity
        .columns
        .iter()
        .zip(identity.values.iter())
        .map(|(name, value)| (ColumnId(name.clone()), value.clone()))
        .collect()
}

/// The statements `changes` stand for, one per change, with the values they carry: what
/// the review shows. It is for reading only -- what runs is `mutations_for`'s, with every
/// value bound, never this text -- so a value is written as the literal that reads right.
pub fn preview_sql(table: &QualifiedName, changes: &ChangeSet, dialect: SqlDialect) -> String {
    let target = [table.schema().or(table.catalog()), Some(table.object())]
        .into_iter()
        .flatten()
        .map(|part| name(part, dialect))
        .collect::<Vec<_>>()
        .join(".");
    let condition = |identity: &RowIdentity| {
        identity
            .columns
            .iter()
            .zip(&identity.values)
            .map(|(column, value)| match value {
                DbValue::Null => format!("{} IS NULL", name(column, dialect)),
                value => format!(
                    "{} = {}",
                    name(column, dialect),
                    sql_literal(value, dialect)
                ),
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    changes
        .pending()
        .iter()
        .map(|change| match change {
            PendingChange::Insert { values } => {
                let columns = values
                    .iter()
                    .map(|(column, _)| name(column, dialect))
                    .collect::<Vec<_>>()
                    .join(", ");
                let literals = values
                    .iter()
                    .map(|(_, value)| sql_literal(value, dialect))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("INSERT INTO {target} ({columns}) VALUES ({literals});")
            }
            PendingChange::Update {
                identity, values, ..
            } => {
                let set = values
                    .iter()
                    .map(|(column, value)| {
                        format!(
                            "{} = {}",
                            name(column, dialect),
                            sql_literal(value, dialect)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("UPDATE {target} SET {set} WHERE {};", condition(identity))
            }
            PendingChange::Delete { identity, .. } => {
                format!("DELETE FROM {target} WHERE {};", condition(identity))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `name` as an identifier: bare when it reads as one, quoted the way `dialect` quotes
/// otherwise.
fn name(name: &str, dialect: SqlDialect) -> String {
    let bare = name
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
    match (bare, dialect) {
        (true, _) => name.to_string(),
        (false, SqlDialect::Mysql) => format!("`{}`", name.replace('`', "``")),
        (false, _) => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::{mutations_for, preview_sql};
    use crate::data::{ChangeSet, ColumnDef, RowIdentity, SqlDialect, TableMeta};
    use dexo_driver_api::{DbValue, QualifiedName};

    fn table(columns: &[(&str, bool)]) -> TableMeta {
        TableMeta {
            columns: columns
                .iter()
                .map(|(name, key)| ColumnDef {
                    name: (*name).into(),
                    primary_key: *key,
                    unique: *key,
                    nullable: !*key,
                })
                .collect(),
        }
    }

    /// The review reads as the statements, values and all, with a quote in a value
    /// doubled; what runs is still the bound mutations.
    #[test]
    fn preview_shows_the_values_and_the_rows_it_changes() {
        let meta = table(&[("order_id", true), ("product_id", true), ("Note", false)]);
        let mut changes = ChangeSet::for_table(&meta);
        let identity = RowIdentity {
            columns: vec!["order_id".into(), "product_id".into()],
            values: vec![DbValue::I64(597), DbValue::I64(3)],
        };
        changes.delete(identity.clone(), Vec::new());
        changes.update(
            identity,
            Vec::new(),
            vec![("Note".into(), DbValue::Text("it's".into()))],
        );
        changes.insert(vec![("order_id".into(), DbValue::Text("'; drop".into()))]);
        let target = QualifiedName::new(Some("db"), Some("public"), "items");
        let sql = preview_sql(&target, &changes, SqlDialect::Postgres);
        assert_eq!(
            sql,
            "DELETE FROM public.items WHERE order_id = 597 AND product_id = 3;\n\
             UPDATE public.items SET \"Note\" = 'it''s' WHERE order_id = 597 AND product_id = 3;\n\
             INSERT INTO public.items (order_id) VALUES ('''; drop');"
        );
        assert!(mutations_for(target, &changes).is_ok());
    }
}
