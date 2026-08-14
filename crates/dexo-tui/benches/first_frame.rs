use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use dexo_tui::model::Model;
use dexo_tui::render::render_to_string;

fn main() {
    let model = Model::default();
    let mut samples = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        let _ = render_to_string(&model, 160, 50);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let under = p95 <= 300.0;
    let payload = format!(
        "{{\"metric\":\"first_frame\",\"p95_ms\":{p95},\"budget_ms\":300,\"under_budget\":{under}}}"
    );
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/first-frame.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
