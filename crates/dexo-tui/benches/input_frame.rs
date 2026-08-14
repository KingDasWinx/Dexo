use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::render::render_to_string;
use dexo_tui::update;

fn main() {
    let mut model = Model::default();
    let mut samples = Vec::new();
    for _ in 0..40 {
        let started = Instant::now();
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        );
        let _ = render_to_string(&model, 100, 30);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        update(&mut model, Action::ClosePalette);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let under = p95 <= 50.0;
    let payload = format!(
        "{{\"metric\":\"input_frame\",\"p95_ms\":{p95},\"budget_ms\":50,\"under_budget\":{under}}}"
    );
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/input-frame.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
