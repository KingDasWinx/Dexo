//! Inspect Value shows the value the way the database prints it, and all of it.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::{ColumnMeta, DbValue};
use dexo_tui::action::Action;
use dexo_tui::render::render_to_string;
use dexo_tui::{Model, update};

fn inspecting(value: DbValue) -> Model {
    let mut model = Model::default();
    model.results.set_columns(vec![ColumnMeta {
        name: "v".into(),
        type_name: "text".into(),
        nullable: true,
    }]);
    model.results.append_rows(vec![vec![value]]);
    model.results.select_cell(0, 0);
    update(&mut model, Action::InspectValue);
    model
}

fn press(model: &mut Model, code: KeyCode) {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// `I64(198)` and `Decimal("4477.50")` were Rust's own Debug output.
#[test]
fn numbers_and_decimals_show_their_digits() {
    for (value, text) in [
        (DbValue::I64(198), "198"),
        (DbValue::Decimal("4477.50".into()), "4477.50"),
        (DbValue::Bool(true), "true"),
    ] {
        let screen = render_to_string(&inspecting(value), 80, 24);
        assert!(screen.contains(text), "{screen}");
        assert!(
            !screen.contains("I64(") && !screen.contains("Decimal("),
            "{screen}"
        );
    }
}

/// A 400-character text was one line cut at the border; it wraps, and the keys scroll it.
#[test]
fn a_long_text_wraps_and_scrolls() {
    let text: String = (0..300).map(|n| format!("w{n:03} ")).collect();
    let mut model = inspecting(DbValue::Text(text.clone()));
    let screen = render_to_string(&model, 60, 14);
    assert!(
        screen.contains(&format!("text, {} characters", text.chars().count())),
        "{screen}"
    );
    assert!(screen.contains("w000"), "{screen}");
    assert!(
        !screen.contains("w299"),
        "the first screen holds the whole text:\n{screen}"
    );
    for _ in 0..40 {
        press(&mut model, KeyCode::Down);
    }
    let screen = render_to_string(&model, 60, 14);
    assert!(screen.contains("w299"), "{screen}");
    press(&mut model, KeyCode::Home);
    assert!(render_to_string(&model, 60, 14).contains("w000"));
    press(&mut model, KeyCode::Esc);
    assert!(model.data.viewer.is_none());
}

/// A tab in a value is drawn as spaces, not dropped.
#[test]
fn tabs_keep_their_width() {
    let screen = render_to_string(&inspecting(DbValue::Text("tab\there".into())), 80, 24);
    assert!(!screen.contains("tabhere"), "{screen}");
    assert!(screen.contains("tab    here"), "{screen}");
}
