use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::{Focus, Model};
use dexo_tui::update;

/// An arrow key in a long script: one update and one frame, the cost a held key pays
/// on every repeat. Before the windowed highlighter this was 52 ms at 3 300 lines --
/// over the key-repeat interval, so the cursor kept moving after the key came up.
fn main() {
    let one = include_str!("../../dexo-sql/tests/fixtures_schema_vendas.sql");
    let script = std::iter::repeat_n(one, 100).collect::<Vec<_>>().join("\n");

    let mut model = Model::default();
    model.apply_size(160, 45);
    model.focus = Focus::Editor;
    model.active_document_mut().sql = dexo_sql::SqlDocument::new(&script);
    dexo_tui::screens::editor::refresh_intelligence(&mut model, false);
    let _ = model
        .active_document_mut()
        .sql
        .set_cursor(script.chars().count() / 2);

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 45)).unwrap();
    let mut samples = Vec::new();
    for i in 0..80 {
        let code = if i % 2 == 0 {
            KeyCode::Down
        } else {
            KeyCode::Right
        };
        let started = Instant::now();
        update(
            &mut model,
            Action::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        );
        let mut hits = dexo_tui::mouse::HitMap::default();
        terminal
            .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
            .unwrap();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    let under = p95 <= 16.0;
    let lines = script.lines().count();
    let payload = format!(
        "{{\"metric\":\"editor_navigation\",\"lines\":{lines},\"p95_ms\":{p95},\"budget_ms\":16,\"under_budget\":{under}}}"
    );
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/editor-navigation.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
