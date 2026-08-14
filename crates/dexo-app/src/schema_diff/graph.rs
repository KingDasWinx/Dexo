use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::schema_diff::diff::SchemaDifference;

#[derive(Clone, Debug, PartialEq)]
pub struct OrderedChange {
    pub difference: SchemaDifference,
    pub manual: bool,
}

pub fn infer_edges(changes: &[SchemaDifference]) -> Vec<(String, String)> {
    use dexo_driver_api::ObjectKind;
    let mut edges = Vec::new();
    let added_tables: Vec<_> = changes
        .iter()
        .filter_map(|change| match change {
            SchemaDifference::Added(object) if object.kind == ObjectKind::Table => {
                Some(change_id(change))
            }
            _ => None,
        })
        .collect();
    let added_deps: Vec<_> = changes
        .iter()
        .filter_map(|change| match change {
            SchemaDifference::Added(object)
                if matches!(object.kind, ObjectKind::Constraint | ObjectKind::Index) =>
            {
                Some(change_id(change))
            }
            _ => None,
        })
        .collect();
    for table in &added_tables {
        for dep in &added_deps {
            edges.push((table.clone(), dep.clone()));
        }
    }
    let removed_deps: Vec<_> = changes
        .iter()
        .filter_map(|change| match change {
            SchemaDifference::Removed(object)
                if matches!(object.kind, ObjectKind::Constraint | ObjectKind::Index) =>
            {
                Some(change_id(change))
            }
            _ => None,
        })
        .collect();
    let removed_tables: Vec<_> = changes
        .iter()
        .filter_map(|change| match change {
            SchemaDifference::Removed(object) if object.kind == ObjectKind::Table => {
                Some(change_id(change))
            }
            _ => None,
        })
        .collect();
    for dep in &removed_deps {
        for table in &removed_tables {
            edges.push((dep.clone(), table.clone()));
        }
    }
    edges
}

pub fn order_changes(
    changes: Vec<SchemaDifference>,
    edges: &[(String, String)],
) -> Vec<OrderedChange> {
    let ids: Vec<String> = changes.iter().map(change_id).collect();
    let id_set: BTreeSet<_> = ids.iter().cloned().collect();
    let mut incoming: BTreeMap<String, usize> = ids.iter().cloned().map(|id| (id, 0)).collect();
    let mut outgoing: BTreeMap<String, Vec<String>> =
        ids.iter().cloned().map(|id| (id, Vec::new())).collect();
    for (from, to) in edges {
        if id_set.contains(from) && id_set.contains(to) {
            outgoing.entry(from.clone()).or_default().push(to.clone());
            *incoming.entry(to.clone()).or_default() += 1;
        }
    }
    let cyclic = cyclic_nodes(&ids, edges);
    let mut ready: Vec<String> = incoming
        .iter()
        .filter(|(id, count)| **count == 0 && !cyclic.contains(*id))
        .map(|(id, _)| id.clone())
        .collect();
    ready.sort();
    let mut ordered = Vec::new();
    let mut remaining: BTreeSet<_> = ids.iter().cloned().collect();
    while let Some(id) = ready.pop() {
        if !remaining.remove(&id) {
            continue;
        }
        ordered.push(id.clone());
        if let Some(nexts) = outgoing.get(&id) {
            for next in nexts {
                if let Some(count) = incoming.get_mut(next) {
                    *count = count.saturating_sub(1);
                    if *count == 0 && remaining.contains(next) && !cyclic.contains(next) {
                        ready.push(next.clone());
                        ready.sort();
                    }
                }
            }
        }
    }
    let mut leftovers: Vec<_> = remaining.into_iter().collect();
    leftovers.sort();
    ordered.extend(leftovers);
    let lookup: HashMap<_, _> = changes
        .into_iter()
        .map(|change| (change_id(&change), change))
        .collect();
    ordered
        .into_iter()
        .filter_map(|id| {
            lookup.get(&id).cloned().map(|difference| OrderedChange {
                manual: cyclic.contains(&id),
                difference,
            })
        })
        .collect()
}

pub fn change_id(change: &SchemaDifference) -> String {
    match change {
        SchemaDifference::Added(object) | SchemaDifference::Removed(object) => {
            format!(
                "{}:{}",
                object.kind.as_str(),
                object.qualified_name.display_unquoted()
            )
        }
        SchemaDifference::Changed { after, .. } => format!(
            "{}:{}",
            after.kind.as_str(),
            after.qualified_name.display_unquoted()
        ),
    }
}

fn cyclic_nodes(ids: &[String], edges: &[(String, String)]) -> BTreeSet<String> {
    let mut adj: BTreeMap<String, Vec<String>> =
        ids.iter().cloned().map(|id| (id, Vec::new())).collect();
    let mut radj = adj.clone();
    for (from, to) in edges {
        if adj.contains_key(from) && adj.contains_key(to) {
            adj.get_mut(from).unwrap().push(to.clone());
            radj.get_mut(to).unwrap().push(from.clone());
        }
    }
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for id in ids {
        dfs(id, &adj, &mut seen, &mut order);
    }
    seen.clear();
    let mut cyclic = BTreeSet::new();
    for id in order.into_iter().rev() {
        if seen.contains(&id) {
            continue;
        }
        let mut component = Vec::new();
        dfs(&id, &radj, &mut seen, &mut component);
        if component.len() > 1 {
            cyclic.extend(component);
        }
    }
    cyclic
}

fn dfs(
    id: &str,
    adj: &BTreeMap<String, Vec<String>>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<String>,
) {
    if !seen.insert(id.to_string()) {
        return;
    }
    if let Some(nexts) = adj.get(id) {
        for next in nexts {
            dfs(next, adj, seen, out);
        }
    }
    out.push(id.to_string());
}

#[cfg(test)]
mod tests {
    use super::order_changes;
    use crate::schema_diff::diff::SchemaDifference;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn obj(kind: ObjectKind, name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            kind,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn table_before_fk_on_create_and_fk_before_table_on_drop() {
        let table = SchemaDifference::Added(obj(ObjectKind::Table, "orders"));
        let fk = SchemaDifference::Added(obj(ObjectKind::Constraint, "orders_fk"));
        let ordered = order_changes(
            vec![fk.clone(), table.clone()],
            &[(
                "table:db.public.orders".into(),
                "constraint:db.public.orders_fk".into(),
            )],
        );
        assert_eq!(
            super::change_id(&ordered[0].difference),
            "table:db.public.orders"
        );
        let drop_fk = SchemaDifference::Removed(obj(ObjectKind::Constraint, "orders_fk"));
        let drop_table = SchemaDifference::Removed(obj(ObjectKind::Table, "orders"));
        let dropped = order_changes(
            vec![drop_table, drop_fk],
            &[(
                "constraint:db.public.orders_fk".into(),
                "table:db.public.orders".into(),
            )],
        );
        assert_eq!(
            super::change_id(&dropped[0].difference),
            "constraint:db.public.orders_fk"
        );
    }

    #[test]
    fn two_view_cycle_is_manual() {
        let a = SchemaDifference::Added(obj(ObjectKind::View, "v1"));
        let b = SchemaDifference::Added(obj(ObjectKind::View, "v2"));
        let ordered = order_changes(
            vec![a, b],
            &[
                ("view:db.public.v1".into(), "view:db.public.v2".into()),
                ("view:db.public.v2".into(), "view:db.public.v1".into()),
            ],
        );
        assert!(ordered.iter().all(|item| item.manual));
    }

    #[test]
    fn non_cycle_edges_respect_output_order() {
        let a = SchemaDifference::Added(obj(ObjectKind::Table, "a"));
        let b = SchemaDifference::Added(obj(ObjectKind::Table, "b"));
        let c = SchemaDifference::Added(obj(ObjectKind::Table, "c"));
        let ordered = order_changes(
            vec![c, b, a],
            &[
                ("table:db.public.a".into(), "table:db.public.b".into()),
                ("table:db.public.b".into(), "table:db.public.c".into()),
            ],
        );
        let ids: Vec<_> = ordered
            .iter()
            .map(|item| super::change_id(&item.difference))
            .collect();
        let pos = |name: &str| ids.iter().position(|id| id == name).unwrap();
        assert!(pos("table:db.public.a") < pos("table:db.public.b"));
        assert!(pos("table:db.public.b") < pos("table:db.public.c"));
        assert!(ordered.iter().all(|item| !item.manual));
    }
}
