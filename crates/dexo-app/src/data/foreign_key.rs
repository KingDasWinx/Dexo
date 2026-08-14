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
