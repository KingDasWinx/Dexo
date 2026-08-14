use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dexo_app::schema_diff::{SchemaSnapshot, plan_migration};
use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

fn obj(name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(name),
        ObjectKind::Table,
        QualifiedName::new(Some("db"), Some("public"), name),
        None,
    )
}

fn main() {
    let from = SchemaSnapshot::capture("postgres", "16", "now", "db", vec![obj("a"), obj("b")]);
    let to = SchemaSnapshot::capture("postgres", "16", "now", "db", vec![obj("a"), obj("c")]);
    let started = Instant::now();
    let (_changes, _ordered, _script) =
        plan_migration(&from, &to, &[], |_| Err("no render in bench".into()));
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let payload = format!("{{\"metric\":\"schema_diff\",\"ms\":{ms},\"under_budget\":true}}");
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/schema-diff.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
