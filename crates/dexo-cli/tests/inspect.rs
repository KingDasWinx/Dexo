use clap::Parser;
use dexo_cli::args::Args;
use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

#[test]
fn inspect_json_args_parse() {
    let args = Args::parse_from([
        "dexo",
        "inspect",
        "--connection",
        "c",
        "--object",
        "db.public.users",
        "--format",
        "json",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Inspect {
            object: Some(ref name),
            ..
        }) if name == "db.public.users"
    ));
}

#[test]
fn inspect_offline_snapshot_latest_parses() {
    let args = Args::parse_from([
        "dexo",
        "inspect",
        "--connection",
        "c",
        "--object",
        "db.public.users",
        "--snapshot",
        "latest",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Inspect {
            snapshot: Some(ref snapshot),
            ..
        }) if snapshot == "latest"
    ));
}

#[test]
fn inspect_json_is_stdout_data_only() {
    let object = CatalogObject::new(
        ObjectId::new("t1"),
        ObjectKind::Table,
        QualifiedName::new(Some("db"), Some("public"), "users"),
        None,
    );
    let json = serde_json::to_string(&object).unwrap();
    assert!(json.contains("db"));
    assert!(json.contains("users"));
    assert!(!json.to_ascii_lowercase().contains("password"));
}

#[test]
fn inspect_grants_flag_parses() {
    let args = Args::parse_from([
        "dexo",
        "inspect",
        "--connection",
        "c",
        "--grants",
        "--object",
        "reporter",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Inspect { grants: true, .. })
    ));
}
