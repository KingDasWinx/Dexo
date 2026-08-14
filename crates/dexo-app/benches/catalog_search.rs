use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dexo_app::search_service::{SearchService, generate_catalog};

fn main() {
    let mut objects = generate_catalog(100_000);
    objects.push(dexo_driver_api::CatalogObject::new(
        dexo_driver_api::ObjectId::new("needle"),
        dexo_driver_api::ObjectKind::Table,
        dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "needle"),
        None,
    ));
    let service = SearchService::from_objects(objects);
    let mut samples = Vec::with_capacity(50);
    for _ in 0..50 {
        let started = Instant::now();
        let hits = service.search("needle");
        assert!(!hits.is_empty());
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let payload = serde_json::json!({
        "metric": "catalog_search",
        "n": 100000,
        "p95_ms": p95,
        "budget_ms": 100,
        "under_budget": p95 <= 100.0
    });
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/catalog-search.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("benchmarks dir");
    }
    fs::write(&path, serde_json::to_string_pretty(&payload).unwrap()).expect("write baseline");
    println!("{}", payload);
}
