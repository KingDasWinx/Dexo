//! Compare keeps its sources in sight after comparing, swaps them, and lists what differs
//! by the kind of object, with the counts as filters.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::screens::schema_diff::{DiffEntry, DiffOption, DiffOptionKind, SchemaDiffScreen};
use dexo_tui::{Action, Effect, Model, update};

fn entry(kind: &'static str, object: &str) -> DiffEntry {
    DiffEntry {
        kind,
        object: object.into(),
        risk: String::new(),
    }
}

fn compared() -> Model {
    let live = |name: &str, id: u128| DiffOption {
        label: format!("{name}  (connected)"),
        name: name.into(),
        kind: DiffOptionKind::Live(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(id))),
        driver: "postgres".into(),
    };
    let mut model = Model::default();
    model.screen = dexo_tui::model::Screen::Compare;
    model.schema_diff = SchemaDiffScreen {
        from_label: "pg-dev".into(),
        to_label: "pg-prod".into(),
        entries: vec![
            entry("added", "table db.public.invoices"),
            entry("added", "index db.public.orders_note_idx"),
            entry("changed", "table db.public.orders"),
            entry("removed", "table db.public.legacy"),
        ],
        script: "CREATE TABLE public.invoices ();\n".into(),
        options: vec![live("pg-dev", 1), live("pg-prod", 2)],
        pick: [0, 1],
        compared: true,
        ..SchemaDiffScreen::default()
    };
    paint(&mut model);
    model
}

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, 140, 40)
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model);
    effects
}

#[test]
fn the_sources_stay_in_sight_and_s_swaps_them() {
    let mut model = compared();
    let frame = paint(&mut model);
    assert!(frame.contains("From ‹ pg-dev ›"), "{frame}");
    assert!(frame.contains("To ‹ pg-prod ›"), "{frame}");
    let effects = press(&mut model, KeyCode::Char('s'));
    let frame = paint(&mut model);
    assert!(frame.contains("From ‹ pg-prod ›"), "{frame}");
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadSchemaDiff { .. })),
        "compared again: {effects:?}"
    );
}

#[test]
fn the_counts_are_filters_by_key_and_by_click() {
    let mut model = compared();
    let frame = paint(&mut model);
    assert!(frame.contains("[a +2 added]"), "{frame}");
    assert!(frame.contains("[r −1 removed]"), "{frame}");
    assert!(frame.contains("[c ~1 changed]"), "{frame}");
    press(&mut model, KeyCode::Char('a'));
    assert!(!model.schema_diff.show_added);
    assert!(paint(&mut model).contains("+2 added (hidden)"));
    let (column, row) = model
        .hits
        .center(HitTarget::Press(KeyCode::Char('a'), false));
    update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(model.schema_diff.show_added);
}

#[test]
fn the_differences_are_listed_by_kind() {
    let mut model = compared();
    let frame = paint(&mut model);
    let tables = frame.find("Tables").expect("a Tables heading");
    let indexes = frame.find("Indexes").expect("an Indexes heading");
    assert!(tables < indexes, "{frame}");
    // The tables' three come together, before the index.
    let shown: Vec<&str> = model
        .schema_diff
        .filtered()
        .iter()
        .map(|entry| entry.object.as_str())
        .collect();
    assert_eq!(
        shown,
        [
            "table db.public.invoices",
            "table db.public.orders",
            "table db.public.legacy",
            "index db.public.orders_note_idx"
        ]
    );
    assert!(frame.contains("+ db.public.invoices"), "{frame}");
    assert!(frame.contains("− db.public.legacy"), "{frame}");
}

#[test]
fn schemas_that_match_say_so() {
    let mut model = compared();
    model.schema_diff.entries.clear();
    model.schema_diff.script.clear();
    let frame = paint(&mut model);
    assert!(frame.contains("✓ The schemas match."), "{frame}");
}
