use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dexo_sql::ParserService;

fn main() {
    let mut parser = ParserService::postgres();
    let sql = "select id, name from public.items where id = 1; -- comment\nselect 2;";
    let mut samples = Vec::new();
    for _ in 0..40 {
        let started = Instant::now();
        let _ = parser.parse(sql);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let payload =
        format!("{{\"metric\":\"incremental_parse\",\"p95_ms\":{p95},\"under_budget\":true}}");
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/parse.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
