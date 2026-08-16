use dexo_driver_api::{
    AlterOp, ConstraintKind, ConstraintSpec, DdlPlan, DriverError, DriverErrorCategory, IndexDef,
    ObjectKind, PrivilegeDef, QualifiedName, RoutineDef, RoutineKind, SchemaChange, TableDef,
    TableShape, ViewDef,
};

pub struct MysqlDialect;

impl MysqlDialect {
    pub fn quote_ident(name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }

    pub fn quote_qualified(name: &QualifiedName) -> String {
        let mut parts = Vec::with_capacity(2);
        if let Some(schema) = name.schema() {
            parts.push(Self::quote_ident(schema));
        } else if let Some(catalog) = name.catalog() {
            parts.push(Self::quote_ident(catalog));
        }
        parts.push(Self::quote_ident(name.object()));
        parts.join(".")
    }

    pub fn quote_column(name: &QualifiedName) -> String {
        Self::quote_ident(name.object())
    }
}

pub fn render(change: &SchemaChange) -> Result<DdlPlan, DriverError> {
    change
        .validate()
        .map_err(|error| DriverError::new(DriverErrorCategory::Syntax, error.to_string()))?;
    let mut plan = DdlPlan {
        transactional: false,
        ..DdlPlan::default()
    };
    match change {
        SchemaChange::CreateTable { target, def } => render_create_table(&mut plan, target, def),
        SchemaChange::AlterTable { target, ops } => render_alter_table(&mut plan, target, ops),
        SchemaChange::CreateView { target, def } => render_create_view(&mut plan, target, def),
        SchemaChange::AlterRoutine { target, def } => render_routine(&mut plan, target, def),
        SchemaChange::CreateIndex { target, def } => render_create_index(&mut plan, target, def),
        SchemaChange::DropObject { target, kind } => render_drop(&mut plan, target, kind),
        SchemaChange::RenameObject { target, new_name } => {
            plan.push(
                format!(
                    "RENAME TABLE {} TO {}",
                    MysqlDialect::quote_qualified(target),
                    MysqlDialect::quote_qualified(new_name)
                ),
                true,
            );
            plan.warnings
                .push("RENAME TABLE causes an implicit commit".into());
        }
        SchemaChange::Grant { target, def } => render_grant(&mut plan, target, def, true),
        SchemaChange::Revoke { target, def } => render_grant(&mut plan, target, def, false),
    }
    plan.risk = change.risk();
    Ok(plan)
}

fn render_create_table(plan: &mut DdlPlan, target: &QualifiedName, def: &TableDef) {
    match &def.shape {
        TableShape::Enum { labels } => {
            plan.warnings
                .push("MySQL enums are column types, not CREATE TYPE".into());
            let values = labels
                .iter()
                .map(|label| format!("'{}'", label.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ");
            plan.push(
                format!(
                    "CREATE TABLE {} (\n  {} ENUM({values}) NOT NULL\n)",
                    MysqlDialect::quote_qualified(target),
                    MysqlDialect::quote_ident("value")
                ),
                true,
            );
        }
        TableShape::Domain { .. } => {
            plan.warnings
                .push("MySQL has no DOMAIN objects; skipped".into());
        }
        TableShape::Table => {
            let mut lines: Vec<String> = def.columns.iter().map(column_sql).collect();
            lines.extend(def.constraints.iter().map(constraint_sql));
            let mut sql = format!(
                "CREATE TABLE {} (\n  {}\n)",
                MysqlDialect::quote_qualified(target),
                lines.join(",\n  ")
            );
            if let Some(engine) = &def.engine {
                sql.push_str(" ENGINE=");
                sql.push_str(engine);
            }
            if let Some(charset) = &def.charset {
                sql.push_str(" DEFAULT CHARSET=");
                sql.push_str(charset);
            }
            if let Some(collation) = &def.collation {
                sql.push_str(" COLLATE=");
                sql.push_str(collation);
            }
            if let Some(partition) = &def.partition {
                let cols = partition
                    .columns
                    .iter()
                    .map(MysqlDialect::quote_column)
                    .collect::<Vec<_>>()
                    .join(", ");
                let method = partition.method.to_ascii_uppercase();
                if method == "HASH" || method == "KEY" {
                    sql.push_str(&format!(" PARTITION BY {method} ({cols}) PARTITIONS 2"));
                } else {
                    sql.push_str(&format!(
                        " PARTITION BY {method} ({cols}) (PARTITION {} VALUES LESS THAN MAXVALUE)",
                        MysqlDialect::quote_ident("p0")
                    ));
                }
            }
            plan.push(sql, true);
            plan.warnings
                .push("CREATE TABLE may rebuild/lock the table and implicitly commits".into());
            plan.rollback.push(format!(
                "DROP TABLE IF EXISTS {}",
                MysqlDialect::quote_qualified(target)
            ));
        }
    }
}

fn column_sql(column: &dexo_driver_api::ColumnSpec) -> String {
    let mut sql = format!(
        "{} {}",
        MysqlDialect::quote_column(&column.name),
        column.data_type
    );
    if column.auto_increment || column.identity.is_some() {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(generated) = &column.generated {
        sql.push_str(" GENERATED ALWAYS AS (");
        sql.push_str(&generated.expression);
        sql.push(')');
        sql.push_str(if generated.stored {
            " STORED"
        } else {
            " VIRTUAL"
        });
    }
    if let Some(default_sql) = &column.default_sql {
        sql.push_str(" DEFAULT ");
        sql.push_str(default_sql);
    }
    if !column.nullable {
        sql.push_str(" NOT NULL");
    }
    if column.primary_key {
        sql.push_str(" PRIMARY KEY");
    }
    sql
}

fn constraint_sql(constraint: &ConstraintSpec) -> String {
    let name = format!(
        "CONSTRAINT {}",
        MysqlDialect::quote_ident(constraint.name.object())
    );
    match &constraint.kind {
        ConstraintKind::PrimaryKey { columns } => {
            format!("{name} PRIMARY KEY ({})", join_cols(columns))
        }
        ConstraintKind::Unique { columns } => format!("{name} UNIQUE ({})", join_cols(columns)),
        ConstraintKind::Check { expression } => format!("{name} CHECK ({expression})"),
        ConstraintKind::ForeignKey(fk) => format!(
            "{name} FOREIGN KEY ({}) REFERENCES {} ({})",
            join_cols(&fk.columns),
            MysqlDialect::quote_qualified(&fk.referenced_table),
            join_cols(&fk.referenced_columns)
        ),
    }
}

fn join_cols(columns: &[QualifiedName]) -> String {
    columns
        .iter()
        .map(MysqlDialect::quote_column)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_alter_table(plan: &mut DdlPlan, target: &QualifiedName, ops: &[AlterOp]) {
    let table = MysqlDialect::quote_qualified(target);
    for op in ops {
        match op {
            AlterOp::AddColumn(column) => {
                plan.push(
                    format!("ALTER TABLE {table} ADD COLUMN {}", column_sql(column)),
                    true,
                );
                plan.warnings
                    .push("ALTER TABLE may rebuild/lock the table".into());
            }
            AlterOp::DropColumn { name } => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} DROP COLUMN {}",
                        MysqlDialect::quote_column(name)
                    ),
                    true,
                );
            }
            AlterOp::AddIndex(index) => {
                let idx_name = index
                    .columns
                    .first()
                    .map(|column| format!("{}_{}_idx", target.object(), column.object()))
                    .unwrap_or_else(|| format!("{}_idx", target.object()));
                render_create_index(
                    plan,
                    &QualifiedName::new(None::<String>, None::<String>, idx_name),
                    index,
                );
            }
            AlterOp::DropIndex { name } => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} DROP INDEX {}",
                        MysqlDialect::quote_ident(name.object())
                    ),
                    true,
                );
            }
            AlterOp::AddConstraint(constraint) => {
                plan.push(
                    format!("ALTER TABLE {table} ADD {}", constraint_sql(constraint)),
                    true,
                );
            }
            AlterOp::DropConstraint { name } => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} DROP CONSTRAINT {}",
                        MysqlDialect::quote_ident(name.object())
                    ),
                    true,
                );
            }
            AlterOp::AddForeignKey(fk) => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} ADD FOREIGN KEY ({}) REFERENCES {} ({})",
                        join_cols(&fk.columns),
                        MysqlDialect::quote_qualified(&fk.referenced_table),
                        join_cols(&fk.referenced_columns)
                    ),
                    true,
                );
            }
            AlterOp::AddPolicy(_) => {
                plan.warnings
                    .push("MySQL has no row-level POLICY objects; skipped".into());
            }
        }
    }
}

fn render_create_view(plan: &mut DdlPlan, target: &QualifiedName, def: &ViewDef) {
    if def.materialized {
        plan.warnings
            .push("MySQL has no materialized views; creating a regular VIEW".into());
    }
    let or_replace = if def.replace { "OR REPLACE " } else { "" };
    plan.push(
        format!(
            "CREATE {or_replace}VIEW {} AS {}",
            MysqlDialect::quote_qualified(target),
            def.sql
        ),
        true,
    );
    plan.rollback.push(format!(
        "DROP VIEW IF EXISTS {}",
        MysqlDialect::quote_qualified(target)
    ));
}

fn render_routine(plan: &mut DdlPlan, target: &QualifiedName, def: &RoutineDef) {
    let name = MysqlDialect::quote_qualified(target);
    match def.kind {
        RoutineKind::Function => {
            let returns = def.returns.as_deref().unwrap_or("INT");
            plan.push(
                format!(
                    "CREATE FUNCTION {name}({}) RETURNS {returns} DETERMINISTIC {}",
                    def.arguments, def.body
                ),
                true,
            );
            plan.rollback
                .push(format!("DROP FUNCTION IF EXISTS {name}"));
        }
        RoutineKind::Procedure => {
            plan.push(format!("CREATE PROCEDURE {name}() {}", def.body), true);
            plan.rollback
                .push(format!("DROP PROCEDURE IF EXISTS {name}"));
        }
        RoutineKind::Trigger => {
            let table = def
                .table
                .as_ref()
                .map(MysqlDialect::quote_qualified)
                .unwrap_or_else(|| MysqlDialect::quote_ident("unknown"));
            let timing = def.timing.as_deref().unwrap_or("BEFORE INSERT");
            plan.push(
                format!(
                    "CREATE TRIGGER {} {timing} ON {table} FOR EACH ROW {}",
                    MysqlDialect::quote_ident(target.object()),
                    def.body
                ),
                true,
            );
            plan.rollback.push(format!(
                "DROP TRIGGER IF EXISTS {}",
                MysqlDialect::quote_ident(target.object())
            ));
        }
        RoutineKind::Event => {
            let schedule = def.schedule.as_deref().unwrap_or("AT CURRENT_TIMESTAMP");
            plan.push(
                format!(
                    "CREATE EVENT {} ON SCHEDULE {schedule} DO {}",
                    MysqlDialect::quote_ident(target.object()),
                    def.body
                ),
                true,
            );
            plan.rollback.push(format!(
                "DROP EVENT IF EXISTS {}",
                MysqlDialect::quote_ident(target.object())
            ));
        }
    }
}

fn render_create_index(plan: &mut DdlPlan, target: &QualifiedName, def: &IndexDef) {
    let unique = if def.unique { " UNIQUE" } else { "" };
    let method = def
        .method
        .as_deref()
        .map(|value| format!(" USING {value}"))
        .unwrap_or_default();
    plan.push(
        format!(
            "CREATE{unique} INDEX {} ON {} ({}){method}",
            MysqlDialect::quote_ident(target.object()),
            MysqlDialect::quote_qualified(&def.table),
            join_cols(&def.columns)
        ),
        true,
    );
    plan.warnings
        .push("CREATE INDEX may lock the table and implicitly commits".into());
    plan.rollback.push(format!(
        "DROP INDEX {} ON {}",
        MysqlDialect::quote_ident(target.object()),
        MysqlDialect::quote_qualified(&def.table)
    ));
}

fn render_drop(plan: &mut DdlPlan, target: &QualifiedName, kind: &ObjectKind) {
    let name = MysqlDialect::quote_qualified(target);
    let sql = match kind {
        ObjectKind::Table => format!("DROP TABLE {name}"),
        ObjectKind::View => format!("DROP VIEW {name}"),
        ObjectKind::Index => format!("DROP INDEX {name}"),
        ObjectKind::Function => format!("DROP FUNCTION {name}"),
        ObjectKind::Procedure => format!("DROP PROCEDURE {name}"),
        ObjectKind::Trigger => format!(
            "DROP TRIGGER {}",
            MysqlDialect::quote_ident(target.object())
        ),
        ObjectKind::User | ObjectKind::Role => {
            format!("DROP ROLE {}", MysqlDialect::quote_ident(target.object()))
        }
        _ => format!("DROP TABLE {name}"),
    };
    plan.push(sql, true);
    plan.warnings
        .push("DROP implicitly commits and is irreversible without a backup".into());
}

fn render_grant(plan: &mut DdlPlan, target: &QualifiedName, def: &PrivilegeDef, grant: bool) {
    if def.create_principal {
        let kind = if def.login { "USER" } else { "ROLE" };
        plan.push(
            format!(
                "CREATE {kind} {}",
                MysqlDialect::quote_ident(def.principal.object())
            ),
            true,
        );
        plan.rollback.push(format!(
            "DROP {kind} IF EXISTS {}",
            MysqlDialect::quote_ident(def.principal.object())
        ));
        return;
    }
    let verb = if grant { "GRANT" } else { "REVOKE" };
    let arrow = if grant { "TO" } else { "FROM" };
    if def.role_membership {
        plan.push(
            format!(
                "{verb} {} {arrow} {}",
                MysqlDialect::quote_ident(target.object()),
                MysqlDialect::quote_ident(def.principal.object())
            ),
            true,
        );
        return;
    }
    let privs = if def.privileges.is_empty() {
        "ALL".to_string()
    } else {
        def.privileges.join(", ")
    };
    plan.push(
        format!(
            "{verb} {privs} ON {} {arrow} {}",
            MysqlDialect::quote_qualified(target),
            MysqlDialect::quote_ident(def.principal.object())
        ),
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::{MysqlDialect, render};
    use dexo_driver_api::{
        ColumnSpec, GeneratedSpec, IndexDef, PartitionSpec, PrivilegeDef, QualifiedName,
        RoutineDef, RoutineKind, SchemaChange, TableDef, TableShape, ViewDef,
    };

    fn ident(name: &str) -> QualifiedName {
        QualifiedName::new(None::<String>, None::<String>, name)
    }

    fn q(schema: &str, object: &str) -> QualifiedName {
        QualifiedName::new(Some(schema), None::<String>, object)
    }

    fn sqls(change: SchemaChange) -> Vec<String> {
        render(&change)
            .unwrap()
            .statements
            .into_iter()
            .map(|statement| statement.sql)
            .collect()
    }

    #[test]
    fn quotes_via_mysql_dialect_only() {
        assert_eq!(MysqlDialect::quote_ident("a`b"), "`a``b`");
        assert_eq!(
            MysqlDialect::quote_qualified(&q("dexo", "orders")),
            "`dexo`.`orders`"
        );
    }

    #[test]
    fn goldens_engine_charset_autoinc_generated_partition_event_routine_trigger_user_grant() {
        let table = sqls(SchemaChange::CreateTable {
            target: q("dexo", "orders"),
            def: TableDef {
                shape: TableShape::Table,
                columns: vec![
                    ColumnSpec {
                        name: ident("id"),
                        data_type: "bigint".into(),
                        nullable: false,
                        default_sql: None,
                        identity: None,
                        auto_increment: true,
                        generated: None,
                        primary_key: true,
                    },
                    ColumnSpec {
                        name: ident("label"),
                        data_type: "varchar(20)".into(),
                        nullable: true,
                        default_sql: None,
                        identity: None,
                        auto_increment: false,
                        generated: Some(GeneratedSpec {
                            expression: "concat(`id`,'x')".into(),
                            stored: true,
                        }),
                        primary_key: false,
                    },
                ],
                constraints: vec![],
                partition: Some(PartitionSpec {
                    method: "hash".into(),
                    columns: vec![ident("id")],
                }),
                engine: Some("InnoDB".into()),
                charset: Some("utf8mb4".into()),
                collation: Some("utf8mb4_0900_ai_ci".into()),
            },
        });
        assert_eq!(
            table,
            [
                "CREATE TABLE `dexo`.`orders` (\n  `id` bigint AUTO_INCREMENT NOT NULL PRIMARY KEY,\n  `label` varchar(20) GENERATED ALWAYS AS (concat(`id`,'x')) STORED\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci PARTITION BY HASH (`id`) PARTITIONS 2"
            ]
        );

        let index = sqls(SchemaChange::CreateIndex {
            target: ident("orders_id_idx"),
            def: IndexDef {
                table: q("dexo", "orders"),
                columns: vec![ident("id")],
                unique: true,
                concurrently: false,
                method: Some("BTREE".into()),
                include: vec![],
                predicate: None,
            },
        });
        assert_eq!(
            index,
            ["CREATE UNIQUE INDEX `orders_id_idx` ON `dexo`.`orders` (`id`) USING BTREE"]
        );

        let view = sqls(SchemaChange::CreateView {
            target: q("dexo", "orders_v"),
            def: ViewDef {
                sql: "SELECT id FROM orders".into(),
                materialized: false,
                replace: true,
            },
        });
        assert_eq!(
            view,
            ["CREATE OR REPLACE VIEW `dexo`.`orders_v` AS SELECT id FROM orders"]
        );

        let function = sqls(SchemaChange::AlterRoutine {
            target: ident("add1"),
            def: RoutineDef {
                kind: RoutineKind::Function,
                arguments: "n INT".into(),
                language: "sql".into(),
                body: "RETURN n + 1".into(),
                returns: Some("INT".into()),
                volatility: None,
                table: None,
                timing: None,
                schedule: None,
            },
        });
        assert_eq!(
            function,
            ["CREATE FUNCTION `add1`(n INT) RETURNS INT DETERMINISTIC RETURN n + 1"]
        );

        let procedure = sqls(SchemaChange::AlterRoutine {
            target: ident("noop"),
            def: RoutineDef {
                kind: RoutineKind::Procedure,
                arguments: String::new(),
                language: "sql".into(),
                body: "BEGIN SET @x = 1; END".into(),
                returns: None,
                volatility: None,
                table: None,
                timing: None,
                schedule: None,
            },
        });
        assert_eq!(
            procedure,
            ["CREATE PROCEDURE `noop`() BEGIN SET @x = 1; END"]
        );

        let trigger = sqls(SchemaChange::AlterRoutine {
            target: ident("orders_tg"),
            def: RoutineDef {
                kind: RoutineKind::Trigger,
                arguments: String::new(),
                language: "sql".into(),
                body: "SET NEW.id = NEW.id".into(),
                returns: None,
                volatility: None,
                table: Some(q("dexo", "orders")),
                timing: Some("BEFORE INSERT".into()),
                schedule: None,
            },
        });
        assert_eq!(
            trigger,
            [
                "CREATE TRIGGER `orders_tg` BEFORE INSERT ON `dexo`.`orders` FOR EACH ROW SET NEW.id = NEW.id"
            ]
        );

        let event = sqls(SchemaChange::AlterRoutine {
            target: ident("tick"),
            def: RoutineDef {
                kind: RoutineKind::Event,
                arguments: String::new(),
                language: "sql".into(),
                body: "SET @a = 1".into(),
                returns: None,
                volatility: None,
                table: None,
                timing: None,
                schedule: Some("AT CURRENT_TIMESTAMP".into()),
            },
        });
        assert_eq!(
            event,
            ["CREATE EVENT `tick` ON SCHEDULE AT CURRENT_TIMESTAMP DO SET @a = 1"]
        );

        let user = sqls(SchemaChange::Grant {
            target: ident("reporter"),
            def: PrivilegeDef {
                principal: ident("reporter"),
                privileges: vec![],
                with_grant_option: false,
                role_membership: false,
                create_principal: true,
                login: true,
            },
        });
        assert_eq!(user, ["CREATE USER `reporter`"]);

        let role = sqls(SchemaChange::Grant {
            target: ident("reader"),
            def: PrivilegeDef {
                principal: ident("reader"),
                privileges: vec![],
                with_grant_option: false,
                role_membership: false,
                create_principal: true,
                login: false,
            },
        });
        assert_eq!(role, ["CREATE ROLE `reader`"]);

        let grant = sqls(SchemaChange::Grant {
            target: q("dexo", "orders"),
            def: PrivilegeDef {
                principal: ident("reader"),
                privileges: vec!["SELECT".into()],
                with_grant_option: false,
                role_membership: false,
                create_principal: false,
                login: false,
            },
        });
        assert_eq!(grant, ["GRANT SELECT ON `dexo`.`orders` TO `reader`"]);
    }

    #[test]
    fn implicit_commit_and_lock_risk_are_marked() {
        let plan = render(&SchemaChange::CreateTable {
            target: q("dexo", "t"),
            def: TableDef {
                shape: TableShape::Table,
                columns: vec![ColumnSpec {
                    name: ident("id"),
                    data_type: "int".into(),
                    nullable: false,
                    default_sql: None,
                    identity: None,
                    auto_increment: true,
                    generated: None,
                    primary_key: true,
                }],
                constraints: vec![],
                partition: None,
                engine: Some("InnoDB".into()),
                charset: None,
                collation: None,
            },
        })
        .unwrap();
        assert!(!plan.transactional);
        assert!(plan.statements[0].implicit_commit);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("lock") || warning.contains("implicit"))
        );
    }
}
