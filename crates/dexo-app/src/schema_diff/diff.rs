use dexo_driver_api::{CatalogObject, ObjectKind};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct RenameMapping {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum SchemaDifference {
    Added(CatalogObject),
    Removed(CatalogObject),
    Changed {
        before: CatalogObject,
        after: CatalogObject,
    },
}

pub fn diff(
    from: &[CatalogObject],
    to: &[CatalogObject],
    renames: &[RenameMapping],
    kind: Option<ObjectKind>,
) -> Vec<SchemaDifference> {
    let from_map = index(from, kind.as_ref());
    let to_map = index(to, kind.as_ref());
    let mut out = Vec::new();
    for (key, before) in &from_map {
        let mapped = renames
            .iter()
            .find(|rename| rename.from == *key)
            .map(|rename| rename.to.as_str())
            .unwrap_or(key.as_str());
        match to_map.get(mapped) {
            None => out.push(SchemaDifference::Removed((*before).clone())),
            Some(after) if *after != *before => out.push(SchemaDifference::Changed {
                before: (*before).clone(),
                after: (*after).clone(),
            }),
            Some(_) => {}
        }
    }
    for (key, after) in &to_map {
        let was_rename = renames.iter().any(|rename| rename.to == *key);
        if was_rename {
            continue;
        }
        if !from_map.contains_key(key) {
            out.push(SchemaDifference::Added((*after).clone()));
        }
    }
    out.sort_by_key(identity);
    out
}

fn identity(item: &SchemaDifference) -> String {
    match item {
        SchemaDifference::Added(object) | SchemaDifference::Removed(object) => object_key(object),
        SchemaDifference::Changed { after, .. } => object_key(after),
    }
}

fn object_key(object: &CatalogObject) -> String {
    format!(
        "{}:{}",
        object.kind.as_str(),
        object.qualified_name.display_unquoted()
    )
}

fn index<'a>(
    objects: &'a [CatalogObject],
    kind: Option<&ObjectKind>,
) -> std::collections::BTreeMap<String, &'a CatalogObject> {
    objects
        .iter()
        .filter(|object| kind.is_none_or(|want| object.kind == *want))
        .map(|object| (object_key(object), object))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{RenameMapping, SchemaDifference, diff};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn table(name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn add_remove_alter_and_ambiguous_rename_stays_remove_plus_add() {
        let from = vec![table("orders"), table("users")];
        let to = vec![
            table("orders").with_attribute("comment", serde_json::json!("x")),
            table("orders_new"),
        ];
        let changes = diff(&from, &to, &[], None);
        assert!(
            changes
                .iter()
                .any(|item| matches!(item, SchemaDifference::Removed(object) if object.qualified_name.object() == "users"))
        );
        assert!(
            changes
                .iter()
                .any(|item| matches!(item, SchemaDifference::Added(object) if object.qualified_name.object() == "orders_new"))
        );
        assert!(
            changes
                .iter()
                .any(|item| matches!(item, SchemaDifference::Changed { .. }))
        );
        let mapped = diff(
            &from,
            &[table("orders_new")],
            &[RenameMapping {
                from: "table:db.public.users".into(),
                to: "table:db.public.orders_new".into(),
            }],
            Some(ObjectKind::Table),
        );
        assert!(
            mapped
                .iter()
                .any(|item| matches!(item, SchemaDifference::Changed { .. }))
        );
        assert!(
            !mapped
                .iter()
                .any(|item| matches!(item, SchemaDifference::Added(_)))
        );
    }

    #[test]
    fn swapping_inputs_reverses_added_and_removed() {
        let from = vec![table("a")];
        let to = vec![table("b")];
        let forward = diff(&from, &to, &[], None);
        let reverse = diff(&to, &from, &[], None);
        assert!(
            matches!(forward[0], SchemaDifference::Removed(_))
                || matches!(forward[1], SchemaDifference::Removed(_))
        );
        let added: Vec<_> = forward
            .iter()
            .filter_map(|item| match item {
                SchemaDifference::Added(object) => Some(object.qualified_name.object().to_string()),
                _ => None,
            })
            .collect();
        let reversed_removed: Vec<_> = reverse
            .iter()
            .filter_map(|item| match item {
                SchemaDifference::Removed(object) => {
                    Some(object.qualified_name.object().to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(added, reversed_removed);
    }
}
