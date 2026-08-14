use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dexo_tui::model::{GridModel, Model};
use dexo_tui::render::render_to_string;

fn main() {
    let model = Model {
        results: GridModel::fixture_rows(1_000),
        ..Model::default()
    };
    let mut samples = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let _ = render_to_string(&model, 120, 40);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let payload =
        format!("{{\"metric\":\"grid_viewport\",\"p95_ms\":{p95},\"visible_only\":true}}");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/grid-viewport.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
