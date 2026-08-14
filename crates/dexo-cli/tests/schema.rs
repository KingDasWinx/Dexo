use clap::Parser;
use dexo_app::schema_diff::{SchemaSnapshot, plan_migration, render_unquoted};
use dexo_cli::args::{Args, SchemaCommand, SchemaDiffFormat};
use dexo_cli::run::schema_apply_guard;
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
fn schema_snapshot_and_diff_args_parse() {
    let snapshot = Args::parse_from([
        "dexo",
        "schema",
        "snapshot",
        "--connection",
        "c",
        "--name",
        "prod",
    ]);
    assert!(matches!(
        snapshot.command,
        Some(dexo_cli::args::Command::Schema {
            command: SchemaCommand::Snapshot { ref name, .. }
        }) if name == "prod"
    ));
    let diff = Args::parse_from([
        "dexo",
        "schema",
        "diff",
        "--from",
        "a",
        "--to",
        "b",
        "--format",
        "sql",
        "--rename",
        "table:db.public.users=table:db.public.accounts",
    ]);
    assert!(matches!(
        diff.command,
        Some(dexo_cli::args::Command::Schema {
            command: SchemaCommand::Diff {
                format: SchemaDiffFormat::Sql,
                apply: false,
                ..
            }
        })
    ));
}

#[test]
fn schema_apply_requires_confirm_target() {
    assert!(schema_apply_guard(true, None).is_err());
    assert!(schema_apply_guard(true, Some("prod.public")).is_ok());
    assert!(schema_apply_guard(false, None).is_ok());
}

#[test]
fn schema_diff_json_and_sql_goldens() {
    let from = SchemaSnapshot::capture(
        "postgres",
        "16",
        "0",
        "db",
        vec![table("kept"), table("gone")],
    );
    let to = SchemaSnapshot::capture(
        "postgres",
        "16",
        "1",
        "db",
        vec![table("kept"), table("added")],
    );
    let (changes, _, script) = plan_migration(&from, &to, &[], render_unquoted);
    let json = serde_json::to_string(&changes).unwrap();
    assert!(json.contains("Added") || json.contains("added") || json.contains("gone"));
    assert!(script.forward.contains("CREATE TABLE"));
    assert!(
        script.forward.contains("DROP TABLE")
            || script.forward.contains("DROP table")
            || script.forward.contains("DROP")
    );
    assert!(script.forward.contains("destructive=true"));
    assert!(script.reverse.is_none());
}
