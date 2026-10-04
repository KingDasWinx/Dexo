//! Search History and Clear History: the list is every connection's, can be searched, and
//! picking from it opens a document instead of replacing the one being written.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::{Action, Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch));
    }
}

fn row(sql: &str) -> dexo_storage::HistoryRow {
    dexo_storage::HistoryRow {
        sql: sql.into(),
        connection_id: Some("pg-dev".into()),
        created_at: "2026-10-03 12:00:00".into(),
        ..Default::default()
    }
}

fn with_history() -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.connection.name = "pg-dev".into();
    model.set_sql("select 42 as important_unsaved_work");
    let effects = update(&mut model, Action::SearchHistory);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadHistory {
                connection_id: None
            }
        )),
        "the history is every connection's: {effects:?}"
    );
    update(
        &mut model,
        Action::HistoryLoaded(vec![
            row("select count(*) from customers"),
            row("update orders set note = 'x'"),
            row("select count(*) from customers"),
            row("select 1"),
        ]),
    );
    model
}

#[test]
fn the_history_lists_each_statement_once_and_searches_after_slash() {
    let mut model = with_history();
    assert_eq!(model.editor.history_lines().len(), 3);
    let screen = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(screen.contains("[1 History]"), "{screen}");
    assert!(screen.contains("/ search"), "{screen}");
    assert!(screen.contains("Enter open"), "{screen}");

    press(&mut model, KeyCode::Char('/'));
    type_text(&mut model, "count");
    let found = model.editor.history_lines();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].row.sql.as_str(), "select count(*) from customers");
    type_text(&mut model, "zzz");
    assert!(model.editor.history_lines().is_empty());
    let screen = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(screen.contains("Nothing matches the filters"), "{screen}");
}

/// Enter opened the statement in the active document, unsaved work included, and ran
/// it. It opens in a document of its own and runs nothing.
#[test]
fn picking_opens_a_new_document_and_runs_nothing() {
    let mut model = with_history();
    let before = model.documents.len();
    let active_title = model.active_document().title.clone();
    press(&mut model, KeyCode::Char('/'));
    type_text(&mut model, "update");
    press(&mut model, KeyCode::Enter);
    let effects = press(&mut model, KeyCode::Enter);
    assert_eq!(model.screen, dexo_tui::model::Screen::Workbench);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "{effects:?}"
    );
    assert_eq!(model.documents.len(), before + 1);
    assert_eq!(
        model.active_document().text(),
        "update orders set note = 'x'"
    );
    assert_eq!(
        model.documents[before - 1].text(),
        "select 42 as important_unsaved_work",
        "the document being written was kept"
    );
    assert_eq!(model.documents[before - 1].title, active_title);
}

#[test]
fn clear_history_asks_first_and_only_its_buttons_answer() {
    use dexo_tui::palette::{filter_entries, palette_entries};
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.connection.name = "pg-dev".into();
    update(&mut model, Action::OpenPalette);
    update(&mut model, Action::PaletteQuery("Clear History".into()));
    let entries = palette_entries(&model);
    let visible = filter_entries(&entries, model.palette.query.as_str());
    let entry = visible
        .iter()
        .position(|entry| entry.id == "editor.history.clear")
        .expect("Clear History is listed");
    assert_eq!(
        visible[entry].disabled_reason, None,
        "it refused with 'history is empty' before the list was read"
    );
    model.palette.selected = entry;
    press(&mut model, KeyCode::Enter);
    assert!(model.editor.history_confirm_clear);
    let screen = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(screen.contains("Clear the history of pg-dev?"), "{screen}");
    assert!(screen.contains("[Clear]"), "{screen}");
    assert!(screen.contains(">[Cancel]"), "{screen}");

    // Enter out of habit keeps it.
    let kept = press(&mut model, KeyCode::Enter);
    assert!(kept.is_empty() && !model.editor.history_confirm_clear);

    update(&mut model, Action::OpenPalette);
    update(&mut model, Action::PaletteQuery("Clear History".into()));
    press(&mut model, KeyCode::Enter);
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ClearHistory { connection_id } if connection_id == "pg-dev"
        )),
        "{effects:?}"
    );
    assert!(!model.editor.history_confirm_clear);
}
