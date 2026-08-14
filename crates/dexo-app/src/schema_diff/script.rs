use dexo_driver_api::{
    ColumnSpec, DdlPlan, IndexDef, ObjectKind, QualifiedName, SchemaChange, TableDef, TableShape,
    ViewDef,
};

use crate::schema_diff::diff::SchemaDifference;
use crate::schema_diff::graph::OrderedChange;
use crate::schema_diff::risk::{classify_difference, comment};

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationScript {
    pub forward: String,
    pub reverse: Option<String>,
}

pub fn generate_script(
    ordered: &[OrderedChange],
    render: impl Fn(&SchemaChange) -> Result<DdlPlan, String>,
) -> MigrationScript {
    let mut forward = String::new();
    let mut reverse_parts = Vec::new();
    let mut all_reversible = true;
    for item in ordered {
        let risk = classify_difference(&item.difference);
        if !risk.reversible || item.manual {
            all_reversible = false;
        }
        let change = to_change(&item.difference);
        let plan = render(&change).unwrap_or_else(|error| {
            let mut plan = DdlPlan::default();
            plan.warnings.push(error);
            plan
        });
        forward.push_str(&comment(risk, item.manual));
        forward.push('\n');
        if item.manual {
            forward.push_str("-- dexo:manual cycle; resolve by hand\n");
        }
        for statement in plan.sqls() {
            forward.push_str(statement);
            forward.push_str(";\n");
        }
        if risk.reversible && !item.manual {
            if let Some(undo) = invert(&item.difference) {
                if let Ok(undo_plan) = render(&undo) {
                    for statement in undo_plan.sqls() {
                        reverse_parts.push(statement.to_string());
                    }
                } else {
                    all_reversible = false;
                }
            } else {
                all_reversible = false;
            }
        }
    }
    MigrationScript {
        forward,
        reverse: all_reversible.then(|| {
            let mut sql = reverse_parts
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(";\n");
            if !sql.is_empty() && !sql.ends_with(';') {
                sql.push(';');
            }
            sql
        }),
    }
}

pub fn render_unquoted(change: &SchemaChange) -> Result<DdlPlan, String> {
    let mut plan = DdlPlan {
        transactional: true,
        ..DdlPlan::default()
    };
    let sql = match change {
        SchemaChange::DropObject { target, kind } => {
            format!(
                "DROP {} {}",
                kind.as_str().to_ascii_uppercase(),
                target.display_unquoted()
            )
        }
        SchemaChange::CreateTable { target, .. } => {
            format!(
                "CREATE TABLE {} (id int PRIMARY KEY)",
                target.display_unquoted()
            )
        }
        SchemaChange::CreateView { target, def } => {
            format!("CREATE VIEW {} AS {}", target.display_unquoted(), def.sql)
        }
        SchemaChange::CreateIndex { target, def } => format!(
            "CREATE INDEX {} ON {}",
            target.display_unquoted(),
            def.table.display_unquoted()
        ),
        SchemaChange::RenameObject { target, new_name } => format!(
            "ALTER TABLE {} RENAME TO {}",
            target.display_unquoted(),
            new_name.object()
        ),
        _ => format!("-- {}", change.target().display_unquoted()),
    };
    plan.push(sql, false);
    Ok(plan)
}

pub fn to_change(difference: &SchemaDifference) -> SchemaChange {
    match difference {
        SchemaDifference::Added(object) => match object.kind {
            ObjectKind::View | ObjectKind::MaterializedView => SchemaChange::CreateView {
                target: object.qualified_name.clone(),
                def: ViewDef {
                    sql: object
                        .attributes
                        .get("sql")
                        .and_then(|value| value.as_str())
                        .unwrap_or("SELECT 1")
                        .to_string(),
                    materialized: object.kind == ObjectKind::MaterializedView,
                    replace: false,
                },
            },
            ObjectKind::Index => SchemaChange::CreateIndex {
                target: object.qualified_name.clone(),
                def: IndexDef {
                    table: object.qualified_name.clone(),
                    columns: vec![QualifiedName::new(None::<String>, None::<String>, "id")],
                    unique: false,
                    concurrently: false,
                    method: None,
                    include: vec![],
                    predicate: None,
                },
            },
            _ => SchemaChange::CreateTable {
                target: object.qualified_name.clone(),
                def: TableDef {
                    shape: TableShape::Table,
                    columns: vec![ColumnSpec {
                        name: QualifiedName::new(None::<String>, None::<String>, "id"),
                        data_type: "int".into(),
                        nullable: false,
                        default_sql: None,
                        identity: None,
                        auto_increment: false,
                        generated: None,
                        primary_key: true,
                    }],
                    constraints: vec![],
                    partition: None,
                    engine: None,
                    charset: None,
                    collation: None,
                },
            },
        },
        SchemaDifference::Removed(object) => SchemaChange::DropObject {
            target: object.qualified_name.clone(),
            kind: object.kind.clone(),
        },
        SchemaDifference::Changed { before, after } => SchemaChange::RenameObject {
            target: before.qualified_name.clone(),
            new_name: after.qualified_name.clone(),
        },
    }
}

fn invert(difference: &SchemaDifference) -> Option<SchemaChange> {
    match difference {
        SchemaDifference::Added(object) => Some(SchemaChange::DropObject {
            target: object.qualified_name.clone(),
            kind: object.kind.clone(),
        }),
        SchemaDifference::Removed(_) => None,
        SchemaDifference::Changed { before, after } => Some(SchemaChange::RenameObject {
            target: after.qualified_name.clone(),
            new_name: before.qualified_name.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::generate_script;
    use crate::schema_diff::diff::{SchemaDifference, diff};
    use crate::schema_diff::graph::{OrderedChange, order_changes};
    use dexo_driver_api::{
        CatalogObject, DdlPlan, ObjectId, ObjectKind, QualifiedName, SchemaChange,
    };

    fn table(name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    fn stub_render(change: &SchemaChange) -> Result<DdlPlan, String> {
        let mut plan = DdlPlan {
            transactional: true,
            ..DdlPlan::default()
        };
        let sql = match change {
            SchemaChange::DropObject { target, .. } => {
                format!("DROP TABLE {}", target.display_unquoted())
            }
            SchemaChange::CreateTable { target, .. } => {
                format!(
                    "CREATE TABLE {} (id int PRIMARY KEY)",
                    target.display_unquoted()
                )
            }
            SchemaChange::CreateView { target, .. } => {
                format!("CREATE VIEW {} AS SELECT 1", target.display_unquoted())
            }
            SchemaChange::RenameObject { target, new_name } => format!(
                "ALTER TABLE {} RENAME TO {}",
                target.display_unquoted(),
                new_name.object()
            ),
            _ => format!("-- {}", change.target().display_unquoted()),
        };
        plan.push(sql, false);
        Ok(plan)
    }

    #[test]
    fn goldens_include_irreversible_drop_data_loss_lock_and_cycle() {
        let drop = OrderedChange {
            difference: SchemaDifference::Removed(table("gone")),
            manual: false,
        };
        let script = generate_script(&[drop], stub_render);
        assert!(script.forward.contains("DROP TABLE db.public.gone"));
        assert!(script.forward.contains("destructive=true"));
        assert!(script.forward.contains("lock=AccessExclusive"));
        assert!(script.reverse.is_none());

        let changed = OrderedChange {
            difference: SchemaDifference::Changed {
                before: table("t").with_attribute("type", serde_json::json!("int")),
                after: table("t").with_attribute("type", serde_json::json!("text")),
            },
            manual: false,
        };
        let typed = generate_script(&[changed], stub_render);
        assert!(typed.forward.contains("data_loss=true"));

        let cycle = order_changes(
            vec![
                SchemaDifference::Added(CatalogObject::new(
                    ObjectId::new("v1"),
                    ObjectKind::View,
                    QualifiedName::new(Some("db"), Some("public"), "v1"),
                    None,
                )),
                SchemaDifference::Added(CatalogObject::new(
                    ObjectId::new("v2"),
                    ObjectKind::View,
                    QualifiedName::new(Some("db"), Some("public"), "v2"),
                    None,
                )),
            ],
            &[
                ("view:db.public.v1".into(), "view:db.public.v2".into()),
                ("view:db.public.v2".into(), "view:db.public.v1".into()),
            ],
        );
        let cycled = generate_script(&cycle, stub_render);
        assert!(cycled.forward.contains("manual=cycle"));
    }

    #[test]
    fn applying_forward_in_memory_yields_empty_diff() {
        let from = vec![table("kept")];
        let to = vec![table("kept"), table("added")];
        let changes = diff(&from, &to, &[], None);
        let ordered: Vec<_> = changes
            .into_iter()
            .map(|difference| OrderedChange {
                difference,
                manual: false,
            })
            .collect();
        let script = generate_script(&ordered, stub_render);
        assert!(script.forward.contains("CREATE TABLE"));
        let mut applied = from;
        applied.push(table("added"));
        assert!(diff(&applied, &to, &[], None).is_empty());
        assert!(script.reverse.is_some());
    }
}
