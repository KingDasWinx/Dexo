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
        compared_pick: [0, 1],
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

/// The toolbar's Compare compares from the pickers too, where it did nothing.
#[test]
fn e_compares_while_the_sources_are_picked() {
    let mut model = compared();
    press(&mut model, KeyCode::Char('p'));
    assert!(model.schema_diff.source_prompt);
    let effects = press(&mut model, KeyCode::Char('e'));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadSchemaDiff { .. })),
        "{effects:?}"
    );
}

/// Swapped and being compared again, the result shown is still the old one: its script
/// cannot be opened as the new one's, and its labels are its own.
#[test]
fn a_swap_leaves_the_result_shown_its_own_until_the_new_one_comes() {
    let mut model = compared();
    model.schema_diff.from_connection = Some("pg-dev".into());
    press(&mut model, KeyCode::Char('s'));
    assert!(model.schema_diff.loading);
    assert_eq!(model.schema_diff.from_label, "pg-dev");
    assert_eq!(model.schema_diff.from_connection.as_deref(), Some("pg-dev"));
    let documents = model.documents.len();
    press(&mut model, KeyCode::Enter);
    assert_eq!(model.documents.len(), documents, "the old script opened");
    assert_eq!(model.screen, dexo_tui::model::Screen::Compare);
    // It failed: the toolbar is back on the sources the result came from.
    update(
        &mut model,
        Action::SchemaDiffFailed {
            message: "gone".into(),
        },
    );
    let frame = paint(&mut model);
    assert!(
        frame.contains("From ‹ pg-dev › ⇄  To ‹ pg-prod ›"),
        "{frame}"
    );
}

/// Picks changed and given up go back to the result's.
#[test]
fn picks_given_up_go_back_to_the_results() {
    let mut model = compared();
    press(&mut model, KeyCode::Char('p'));
    press(&mut model, KeyCode::Char('s'));
    press(&mut model, KeyCode::Esc);
    assert!(!model.schema_diff.source_prompt);
    let frame = paint(&mut model);
    assert!(
        frame.contains("From ‹ pg-dev › ⇄  To ‹ pg-prod ›"),
        "{frame}"
    );
}

/// A click on [Cancel] does what Esc does: back to the result, not off the screen.
#[test]
fn a_click_on_cancel_goes_back_to_the_result() {
    let mut model = compared();
    press(&mut model, KeyCode::Char('p'));
    paint(&mut model);
    let (column, row) = model.hits.center(HitTarget::FooterCancel);
    update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(model.screen, dexo_tui::model::Screen::Compare);
    assert!(!model.schema_diff.source_prompt);
}

/// `/` searches the differences, as it searches every screen's list.
#[test]
fn slash_searches_the_differences() {
    let mut model = compared();
    let frame = paint(&mut model);
    assert!(frame.contains("/ search"), "{frame}");
    press(&mut model, KeyCode::Char('/'));
    for ch in "legacy".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let shown: Vec<String> = model
        .schema_diff
        .filtered()
        .iter()
        .map(|entry| entry.object.clone())
        .collect();
    assert_eq!(shown, ["table db.public.legacy"]);
    // The letters were the search's: `a` hid no kind.
    assert!(model.schema_diff.show_added);
    press(&mut model, KeyCode::Enter);
    press(&mut model, KeyCode::Esc);
    assert_eq!(model.schema_diff.filtered().len(), 4);
    assert_eq!(model.screen, dexo_tui::model::Screen::Compare);
}

/// The header lists Compare only while it is on screen; the palette and `Ctrl+G d` open it.
#[test]
fn the_header_lists_compare_only_while_on_it() {
    let mut model = Model::default();
    let frame = paint(&mut model);
    let header = frame.lines().next().unwrap_or_default();
    assert!(header.contains("History"), "{header}");
    assert!(!header.contains("Compare"), "{header}");
    let mut model = compared();
    let frame = paint(&mut model);
    let header = frame.lines().next().unwrap_or_default();
    assert!(header.contains("History [Compare]"), "{header}");
}
