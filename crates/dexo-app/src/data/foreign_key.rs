use dexo_driver_api::{ColumnId, DbValue, Filter, QualifiedName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignKey {
    pub local: Vec<String>,
    pub referenced_table: QualifiedName,
    pub referenced: Vec<String>,
}

pub fn related_filter(fk: &ForeignKey, row: &[(String, Option<DbValue>)]) -> Option<Filter> {
    if fk.local.len() != fk.referenced.len() || fk.local.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for (local, referenced) in fk.local.iter().zip(fk.referenced.iter()) {
        let value = row.iter().find(|(name, _)| name == local)?.1.as_ref()?;
        parts.push(Filter::Eq(ColumnId(referenced.clone()), value.clone()));
    }
    Some(if parts.len() == 1 {
        parts.pop().unwrap()
    } else {
        Filter::And(parts)
    })
}

pub fn from_attributes(
    attributes: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<ForeignKey> {
    let local = string_list(attributes.get("fk_local")?)?;
    let referenced = string_list(attributes.get("fk_referenced")?)?;
    let table = attributes.get("fk_table")?.as_str()?.to_string();
    if local.is_empty() || local.len() != referenced.len() || table.is_empty() {
        return None;
    }
    let catalog = attributes
        .get("fk_catalog")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let schema = attributes
        .get("fk_schema")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    Some(ForeignKey {
        local,
        referenced_table: QualifiedName::new(catalog, schema, table),
        referenced,
    })
}

fn string_list(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{ForeignKey, related_filter};
    use dexo_driver_api::{ColumnId, DbValue, Filter, QualifiedName};

    #[test]
    fn composite_fk_maps_local_columns_to_typed_filter() {
        let fk = ForeignKey {
            local: vec!["org_id".into(), "user_id".into()],
            referenced_table: QualifiedName::new(Some("db"), Some("public"), "users"),
            referenced: vec!["org".into(), "id".into()],
        };
        let filter = related_filter(
            &fk,
            &[
                ("org_id".into(), Some(DbValue::I64(7))),
                ("user_id".into(), Some(DbValue::I64(3))),
            ],
        )
        .unwrap();
        assert_eq!(
            filter,
            Filter::And(vec![
                Filter::Eq(ColumnId("org".into()), DbValue::I64(7)),
                Filter::Eq(ColumnId("id".into()), DbValue::I64(3)),
            ])
        );
        assert!(
            related_filter(
                &fk,
                &[
                    ("org_id".into(), None),
                    ("user_id".into(), Some(DbValue::I64(1)))
                ]
            )
            .is_none()
        );
    }
}
