use dexo_driver_api::{ColumnId, DriverError, Mutation, QualifiedName};

use super::change_set::{ChangeSet, PendingChange};

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

pub fn preview_sql(table: &QualifiedName, changes: &ChangeSet) -> String {
    let target = table.display_unquoted();
    changes
        .pending()
        .iter()
        .map(|change| match change {
            PendingChange::Insert { values } => {
                let cols = values
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let slots = (1..=values.len())
                    .map(|index| format!("${index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("INSERT INTO {target} ({cols}) VALUES ({slots})")
            }
            PendingChange::Update {
                identity, values, ..
            } => {
                let set = values
                    .iter()
                    .enumerate()
                    .map(|(index, (name, _))| format!("{name} = ${}", index + 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "UPDATE {target} SET {set} WHERE {} = $n",
                    identity.columns.join(", ")
                )
            }
            PendingChange::Delete { identity, .. } => {
                format!(
                    "DELETE FROM {target} WHERE {} = $n",
                    identity.columns.join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{mutations_for, preview_sql};
    use crate::data::{ChangeSet, ColumnDef, TableMeta};
    use dexo_driver_api::{DbValue, QualifiedName};

    #[test]
    fn preview_uses_placeholders_not_concatenated_values() {
        let table = TableMeta {
            columns: vec![ColumnDef {
                name: "id".into(),
                primary_key: true,
                unique: true,
                nullable: false,
            }],
        };
        let mut changes = ChangeSet::for_table(&table);
        changes.insert(vec![("id".into(), DbValue::Text("'; drop".into()))]);
        let sql = preview_sql(
            &QualifiedName::new(Some("db"), Some("public"), "items"),
            &changes,
        );
        assert!(sql.contains("$1"));
        assert!(!sql.contains("'; drop"));
        assert!(
            mutations_for(
                QualifiedName::new(Some("db"), Some("public"), "items"),
                &changes
            )
            .is_ok()
        );
    }
}
