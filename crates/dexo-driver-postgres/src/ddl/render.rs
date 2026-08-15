use dexo_driver_api::{
    AlterOp, ConstraintKind, ConstraintSpec, DdlPlan, DriverError, DriverErrorCategory, IndexDef,
    ObjectKind, PrivilegeDef, QualifiedName, RoutineDef, RoutineKind, SchemaChange, TableDef,
    TableShape, ViewDef,
};

pub struct PgDialect;

impl PgDialect {
    pub fn quote_ident(name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
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
        transactional: true,
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
                    "ALTER TABLE {} RENAME TO {}",
                    PgDialect::quote_qualified(target),
                    PgDialect::quote_ident(new_name.object())
                ),
                false,
            );
            plan.rollback.push(format!(
                "ALTER TABLE {} RENAME TO {}",
                PgDialect::quote_qualified(new_name),
                PgDialect::quote_ident(target.object())
            ));
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
            let values = labels
                .iter()
                .map(|label| format!("'{}'", label.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ");
            plan.push(
                format!(
                    "CREATE TYPE {} AS ENUM ({values})",
                    PgDialect::quote_qualified(target)
                ),
                false,
            );
            plan.rollback.push(format!(
                "DROP TYPE IF EXISTS {}",
                PgDialect::quote_qualified(target)
            ));
        }
        TableShape::Domain { base_type, check } => {
            let mut sql = format!(
                "CREATE DOMAIN {} AS {base_type}",
                PgDialect::quote_qualified(target)
            );
            if let Some(check) = check {
                sql.push_str(" CHECK (");
                sql.push_str(check);
                sql.push(')');
            }
            plan.push(sql, false);
            plan.rollback.push(format!(
                "DROP DOMAIN IF EXISTS {}",
                PgDialect::quote_qualified(target)
            ));
        }
        TableShape::Table => {
            let mut lines: Vec<String> = def.columns.iter().map(column_sql).collect();
            lines.extend(def.constraints.iter().map(constraint_sql));
            let mut sql = format!(
                "CREATE TABLE {} (\n  {}\n)",
                PgDialect::quote_qualified(target),
                lines.join(",\n  ")
            );
            if let Some(partition) = &def.partition {
                let cols = partition
                    .columns
                    .iter()
                    .map(PgDialect::quote_column)
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!(
                    " PARTITION BY {} ({cols})",
                    partition.method.to_ascii_uppercase()
                ));
            }
            plan.push(sql, false);
            plan.rollback.push(format!(
                "DROP TABLE IF EXISTS {}",
                PgDialect::quote_qualified(target)
            ));
        }
    }
}

fn column_sql(column: &dexo_driver_api::ColumnSpec) -> String {
    let mut sql = format!(
        "{} {}",
        PgDialect::quote_column(&column.name),
        column.data_type
    );
    if let Some(identity) = &column.identity {
        sql.push_str(if identity.always {
            " GENERATED ALWAYS AS IDENTITY"
        } else {
            " GENERATED BY DEFAULT AS IDENTITY"
        });
    }
    if let Some(generated) = &column.generated {
        sql.push_str(" GENERATED ALWAYS AS (");
        sql.push_str(&generated.expression);
        sql.push(')');
        sql.push_str(if generated.stored { " STORED" } else { "" });
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
        PgDialect::quote_ident(constraint.name.object())
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
            PgDialect::quote_qualified(&fk.referenced_table),
            join_cols(&fk.referenced_columns)
        ),
    }
}

fn join_cols(columns: &[QualifiedName]) -> String {
    columns
        .iter()
        .map(PgDialect::quote_column)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_alter_table(plan: &mut DdlPlan, target: &QualifiedName, ops: &[AlterOp]) {
    let table = PgDialect::quote_qualified(target);
    for op in ops {
        match op {
            AlterOp::AddColumn(column) => {
                plan.push(
                    format!("ALTER TABLE {table} ADD COLUMN {}", column_sql(column)),
                    false,
                );
                plan.rollback.push(format!(
                    "ALTER TABLE {table} DROP COLUMN {}",
                    PgDialect::quote_column(&column.name)
                ));
            }
            AlterOp::DropColumn { name } => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} DROP COLUMN {}",
                        PgDialect::quote_column(name)
                    ),
                    false,
                );
                plan.warnings
                    .push("DROP COLUMN is irreversible without a backup".into());
            }
            AlterOp::AddIndex(index) => {
                let idx_name = QualifiedName::new(
                    target.catalog(),
                    target.schema(),
                    index
                        .columns
                        .first()
                        .map(|column| format!("{}_{}_idx", target.object(), column.object()))
                        .unwrap_or_else(|| format!("{}_idx", target.object())),
                );
                render_create_index(plan, &idx_name, index);
            }
            AlterOp::DropIndex { name } => {
                plan.push(
                    format!("DROP INDEX {}", PgDialect::quote_qualified(name)),
                    false,
                );
            }
            AlterOp::AddConstraint(constraint) => {
                plan.push(
                    format!("ALTER TABLE {table} ADD {}", constraint_sql(constraint)),
                    false,
                );
            }
            AlterOp::DropConstraint { name } => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} DROP CONSTRAINT {}",
                        PgDialect::quote_ident(name.object())
                    ),
                    false,
                );
            }
            AlterOp::AddForeignKey(fk) => {
                plan.push(
                    format!(
                        "ALTER TABLE {table} ADD FOREIGN KEY ({}) REFERENCES {} ({})",
                        join_cols(&fk.columns),
                        PgDialect::quote_qualified(&fk.referenced_table),
                        join_cols(&fk.referenced_columns)
                    ),
                    false,
                );
            }
            AlterOp::AddPolicy(policy) => {
                plan.push(
                    format!(
                        "ALTER TABLE {} ENABLE ROW LEVEL SECURITY",
                        PgDialect::quote_qualified(&policy.table)
                    ),
                    false,
                );
                plan.push(
                    format!(
                        "CREATE POLICY {} ON {} FOR {} USING ({})",
                        PgDialect::quote_ident(target.object()),
                        PgDialect::quote_qualified(&policy.table),
                        policy.command,
                        policy.using_sql
                    ),
                    false,
                );
                plan.rollback.push(format!(
                    "DROP POLICY IF EXISTS {} ON {}",
                    PgDialect::quote_ident(target.object()),
                    PgDialect::quote_qualified(&policy.table)
                ));
            }
        }
    }
}

fn render_create_view(plan: &mut DdlPlan, target: &QualifiedName, def: &ViewDef) {
    let kind = if def.materialized {
        "MATERIALIZED VIEW"
    } else {
        "VIEW"
    };
    let or_replace = if def.replace && !def.materialized {
        "OR REPLACE "
    } else {
        ""
    };
    plan.push(
        format!(
            "CREATE {or_replace}{kind} {} AS {}",
            PgDialect::quote_qualified(target),
            def.sql
        ),
        false,
    );
    plan.rollback.push(format!(
        "DROP {kind} IF EXISTS {}",
        PgDialect::quote_qualified(target)
    ));
}

fn dollar_quote(body: &str) -> String {
    let mut n = 0_u32;
    loop {
        let tag = if n == 0 {
            "$dexo$".to_string()
        } else {
            format!("$dexo{n}$")
        };
        if !body.contains(&tag) {
            return format!("{tag}{body}{tag}");
        }
        n += 1;
    }
}

fn render_routine(plan: &mut DdlPlan, target: &QualifiedName, def: &RoutineDef) {
    let name = PgDialect::quote_qualified(target);
    match def.kind {
        RoutineKind::Function => {
            let returns = def.returns.as_deref().unwrap_or("void");
            let vol = def
                .volatility
                .as_deref()
                .map(|value| format!(" {value}"))
                .unwrap_or_default();
            plan.push(
                format!(
                    "CREATE FUNCTION {name}({}) RETURNS {returns} LANGUAGE {}{vol} AS {}",
                    def.arguments,
                    def.language,
                    dollar_quote(&def.body)
                ),
                false,
            );
            plan.rollback
                .push(format!("DROP FUNCTION IF EXISTS {name}"));
        }
        RoutineKind::Procedure => {
            plan.push(
                format!(
                    "CREATE PROCEDURE {name}({}) LANGUAGE {} AS {}",
                    def.arguments,
                    def.language,
                    dollar_quote(&def.body)
                ),
                false,
            );
            plan.rollback
                .push(format!("DROP PROCEDURE IF EXISTS {name}"));
        }
        RoutineKind::Trigger => {
            let table = def
                .table
                .as_ref()
                .map(PgDialect::quote_qualified)
                .unwrap_or_else(|| PgDialect::quote_ident("unknown"));
            let timing = def.timing.as_deref().unwrap_or("BEFORE INSERT");
            plan.push(
                format!(
                    "CREATE TRIGGER {} {timing} ON {table} FOR EACH ROW EXECUTE FUNCTION {}",
                    PgDialect::quote_ident(target.object()),
                    def.body
                ),
                false,
            );
            plan.rollback.push(format!(
                "DROP TRIGGER IF EXISTS {} ON {table}",
                PgDialect::quote_ident(target.object())
            ));
        }
        RoutineKind::Event => {
            plan.warnings
                .push("PostgreSQL has no EVENT objects; skipped".into());
        }
    }
}

fn render_create_index(plan: &mut DdlPlan, target: &QualifiedName, def: &IndexDef) {
    let concurrently = if def.concurrently {
        " CONCURRENTLY"
    } else {
        ""
    };
    if def.concurrently {
        plan.transactional = false;
        plan.warnings
            .push("CREATE INDEX CONCURRENTLY cannot run inside a transaction".into());
    }
    let unique = if def.unique { " UNIQUE" } else { "" };
    let method = def
        .method
        .as_deref()
        .map(|value| format!(" USING {value}"))
        .unwrap_or_default();
    let include = if def.include.is_empty() {
        String::new()
    } else {
        format!(" INCLUDE ({})", join_cols(&def.include))
    };
    let pred = def
        .predicate
        .as_deref()
        .map(|value| format!(" WHERE ({value})"))
        .unwrap_or_default();
    plan.push(
        format!(
            "CREATE{unique} INDEX{concurrently} {} ON {}{method} ({}){include}{pred}",
            PgDialect::quote_ident(target.object()),
            PgDialect::quote_qualified(&def.table),
            join_cols(&def.columns)
        ),
        false,
    );
    plan.rollback.push(format!(
        "DROP INDEX IF EXISTS {}",
        PgDialect::quote_ident(target.object())
    ));
}

fn render_drop(plan: &mut DdlPlan, target: &QualifiedName, kind: &ObjectKind) {
    let name = PgDialect::quote_qualified(target);
    let sql = match kind {
        ObjectKind::Table => format!("DROP TABLE {name}"),
        ObjectKind::View => format!("DROP VIEW {name}"),
        ObjectKind::MaterializedView => format!("DROP MATERIALIZED VIEW {name}"),
        ObjectKind::Index => format!("DROP INDEX {name}"),
        ObjectKind::Function => format!("DROP FUNCTION {name}"),
        ObjectKind::Procedure => format!("DROP PROCEDURE {name}"),
        ObjectKind::Trigger => format!("DROP TRIGGER {name}"),
        ObjectKind::User | ObjectKind::Role => {
            format!("DROP ROLE {}", PgDialect::quote_ident(target.object()))
        }
        ObjectKind::Sequence => format!("DROP SEQUENCE {name}"),
        ObjectKind::Schema => format!("DROP SCHEMA {name}"),
        _ => format!("DROP TABLE {name}"),
    };
    plan.push(sql, false);
    plan.warnings
        .push("DROP is irreversible without a backup".into());
}

fn render_grant(plan: &mut DdlPlan, target: &QualifiedName, def: &PrivilegeDef, grant: bool) {
    if def.create_principal {
        let login = if def.login { " LOGIN" } else { "" };
        plan.push(
            format!(
                "CREATE ROLE {}{login}",
                PgDialect::quote_ident(def.principal.object())
            ),
            false,
        );
        plan.rollback.push(format!(
            "DROP ROLE IF EXISTS {}",
            PgDialect::quote_ident(def.principal.object())
        ));
        return;
    }
    let verb = if grant { "GRANT" } else { "REVOKE" };
    let arrow = if grant { "TO" } else { "FROM" };
    let option = if grant && def.with_grant_option {
        " WITH GRANT OPTION"
    } else {
        ""
    };
    if def.role_membership {
        plan.push(
            format!(
                "{verb} {} {arrow} {}{option}",
                PgDialect::quote_ident(target.object()),
                PgDialect::quote_ident(def.principal.object())
            ),
            false,
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
            "{verb} {privs} ON TABLE {} {arrow} {}{option}",
            PgDialect::quote_qualified(target),
            PgDialect::quote_ident(def.principal.object())
        ),
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::{PgDialect, render};
    use dexo_driver_api::{
        AlterOp, ColumnSpec, IdentitySpec, IndexDef, PartitionSpec, PolicyDef, PrivilegeDef,
        QualifiedName, RoutineDef, RoutineKind, SchemaChange, TableDef, TableShape, ViewDef,
    };

    fn ident(name: &str) -> QualifiedName {
        QualifiedName::new(None::<String>, None::<String>, name)
    }

    fn q(schema: &str, object: &str) -> QualifiedName {
        QualifiedName::new(None::<String>, Some(schema), object)
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
    fn quotes_via_pg_dialect_only() {
        assert_eq!(PgDialect::quote_ident(r#"a"b"#), r#""a""b""#);
        assert_eq!(
            PgDialect::quote_qualified(&q("public", "orders")),
            r#""public"."orders""#
        );
    }

    #[test]
    fn goldens_identity_enum_domain_partition_matview_index_policy_routines_role_grant() {
        let identity = sqls(SchemaChange::CreateTable {
            target: q("public", "orders"),
            def: TableDef {
                shape: TableShape::Table,
                columns: vec![ColumnSpec {
                    name: ident("id"),
                    data_type: "bigint".into(),
                    nullable: false,
                    default_sql: None,
                    identity: Some(IdentitySpec { always: true }),
                    auto_increment: false,
                    generated: None,
                    primary_key: false,
                }],
                constraints: vec![],
                partition: None,
                engine: None,
                charset: None,
                collation: None,
            },
        });
        assert_eq!(
            identity,
            [
                "CREATE TABLE \"public\".\"orders\" (\n  \"id\" bigint GENERATED ALWAYS AS IDENTITY NOT NULL\n)"
            ]
        );

        let enum_sql = sqls(SchemaChange::CreateTable {
            target: q("public", "mood"),
            def: TableDef {
                shape: TableShape::Enum {
                    labels: vec!["sad".into(), "ok".into(), "happy".into()],
                },
                columns: vec![],
                constraints: vec![],
                partition: None,
                engine: None,
                charset: None,
                collation: None,
            },
        });
        assert_eq!(
            enum_sql,
            ["CREATE TYPE \"public\".\"mood\" AS ENUM ('sad', 'ok', 'happy')"]
        );

        let domain = sqls(SchemaChange::CreateTable {
            target: q("public", "posint"),
            def: TableDef {
                shape: TableShape::Domain {
                    base_type: "integer".into(),
                    check: Some("VALUE > 0".into()),
                },
                columns: vec![],
                constraints: vec![],
                partition: None,
                engine: None,
                charset: None,
                collation: None,
            },
        });
        assert_eq!(
            domain,
            ["CREATE DOMAIN \"public\".\"posint\" AS integer CHECK (VALUE > 0)"]
        );

        let partition = sqls(SchemaChange::CreateTable {
            target: q("public", "orders"),
            def: TableDef {
                shape: TableShape::Table,
                columns: vec![ColumnSpec {
                    name: ident("id"),
                    data_type: "integer".into(),
                    nullable: false,
                    default_sql: None,
                    identity: None,
                    auto_increment: false,
                    generated: None,
                    primary_key: false,
                }],
                constraints: vec![],
                partition: Some(PartitionSpec {
                    method: "range".into(),
                    columns: vec![ident("id")],
                }),
                engine: None,
                charset: None,
                collation: None,
            },
        });
        assert_eq!(
            partition,
            [
                "CREATE TABLE \"public\".\"orders\" (\n  \"id\" integer NOT NULL\n) PARTITION BY RANGE (\"id\")"
            ]
        );

        let matview = sqls(SchemaChange::CreateView {
            target: q("public", "orders_mv"),
            def: ViewDef {
                sql: "SELECT id FROM orders".into(),
                materialized: true,
                replace: false,
            },
        });
        assert_eq!(
            matview,
            ["CREATE MATERIALIZED VIEW \"public\".\"orders_mv\" AS SELECT id FROM orders"]
        );

        let index = sqls(SchemaChange::CreateIndex {
            target: ident("orders_id_idx"),
            def: IndexDef {
                table: q("public", "orders"),
                columns: vec![ident("id")],
                unique: true,
                concurrently: true,
                method: Some("btree".into()),
                include: vec![ident("qty")],
                predicate: Some("id > 0".into()),
            },
        });
        assert_eq!(
            index,
            [
                "CREATE UNIQUE INDEX CONCURRENTLY \"orders_id_idx\" ON \"public\".\"orders\" USING btree (\"id\") INCLUDE (\"qty\") WHERE (id > 0)"
            ]
        );

        let policy = sqls(SchemaChange::AlterTable {
            target: ident("orders_all"),
            ops: vec![AlterOp::AddPolicy(PolicyDef {
                table: q("public", "orders"),
                command: "ALL".into(),
                using_sql: "true".into(),
                check_sql: None,
            })],
        });
        assert_eq!(
            policy,
            [
                "ALTER TABLE \"public\".\"orders\" ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY \"orders_all\" ON \"public\".\"orders\" FOR ALL USING (true)"
            ]
        );

        let function = sqls(SchemaChange::AlterRoutine {
            target: q("public", "add1"),
            def: RoutineDef {
                kind: RoutineKind::Function,
                arguments: "n integer".into(),
                language: "sql".into(),
                body: "SELECT n + 1".into(),
                returns: Some("integer".into()),
                volatility: Some("IMMUTABLE".into()),
                table: None,
                timing: None,
                schedule: None,
            },
        });
        assert_eq!(
            function,
            [
                "CREATE FUNCTION \"public\".\"add1\"(n integer) RETURNS integer LANGUAGE sql IMMUTABLE AS $dexo$SELECT n + 1$dexo$"
            ]
        );

        let procedure = sqls(SchemaChange::AlterRoutine {
            target: q("public", "noop"),
            def: RoutineDef {
                kind: RoutineKind::Procedure,
                arguments: String::new(),
                language: "plpgsql".into(),
                body: "BEGIN NULL; END;".into(),
                returns: None,
                volatility: None,
                table: None,
                timing: None,
                schedule: None,
            },
        });
        assert_eq!(
            procedure,
            [
                "CREATE PROCEDURE \"public\".\"noop\"() LANGUAGE plpgsql AS $dexo$BEGIN NULL; END;$dexo$"
            ]
        );

        let trigger = sqls(SchemaChange::AlterRoutine {
            target: ident("orders_tg"),
            def: RoutineDef {
                kind: RoutineKind::Trigger,
                arguments: String::new(),
                language: "plpgsql".into(),
                body: "public.tg_fn()".into(),
                returns: None,
                volatility: None,
                table: Some(q("public", "orders")),
                timing: Some("BEFORE INSERT".into()),
                schedule: None,
            },
        });
        assert_eq!(
            trigger,
            [
                "CREATE TRIGGER \"orders_tg\" BEFORE INSERT ON \"public\".\"orders\" FOR EACH ROW EXECUTE FUNCTION public.tg_fn()"
            ]
        );

        let role = sqls(SchemaChange::Grant {
            target: ident("reporter"),
            def: PrivilegeDef {
                principal: ident("reporter"),
                privileges: vec![],
                with_grant_option: false,
                role_membership: false,
                create_principal: true,
                login: false,
            },
        });
        assert_eq!(role, ["CREATE ROLE \"reporter\""]);

        let grant = sqls(SchemaChange::Grant {
            target: q("public", "orders"),
            def: PrivilegeDef {
                principal: ident("reporter"),
                privileges: vec!["SELECT".into()],
                with_grant_option: false,
                role_membership: false,
                create_principal: false,
                login: false,
            },
        });
        assert_eq!(
            grant,
            ["GRANT SELECT ON TABLE \"public\".\"orders\" TO \"reporter\""]
        );
    }

    #[test]
    fn never_accepts_raw_identifier_outside_qualified_name() {
        let plan = render(&SchemaChange::DropObject {
            target: q("public", r#"odd"name"#),
            kind: dexo_driver_api::ObjectKind::Table,
        })
        .unwrap();
        assert_eq!(
            plan.statements[0].sql,
            "DROP TABLE \"public\".\"odd\"\"name\""
        );
    }
}
