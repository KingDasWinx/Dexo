//! Agent Activity keeps clear of the tab bar and the status line, as a popup should.
use dexo_tui::{Model, render::render_to_string};

#[test]
fn the_popup_leaves_the_top_row_and_the_status_line_alone() {
    let mut model = Model::default();
    model.apply_size(80, 24);
    model.mcp_audit.open = true;
    model.mcp_audit.events = (0..60)
        .map(|n| format!("12:00:00 pg-dev: event {n} -- ok"))
        .collect();
    let screen = render_to_string(&model, 80, 24);
    let rows: Vec<&str> = screen.lines().collect();
    assert!(
        !rows[0].contains('\u{250c}'),
        "the header row is covered:\n{screen}"
    );
    assert!(rows[1..].iter().any(|row| row.contains("Agent activity")));
    assert!(!rows[23].contains("Agent activity"), "{screen}");
}
