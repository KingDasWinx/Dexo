//! The record modal's copy actions each put the record on the clipboard.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::GridModel;
use dexo_tui::{Effect, Model, update};

fn copied_from_menu(id: &str) -> Option<String> {
    let index = dexo_tui::palette::results_menu_items()
        .iter()
        .position(|(item, _)| *item == id)
        .unwrap_or_else(|| panic!("no {id} in the menu"));
    let mut model = Model::default();
    *model.results = GridModel::sample_rows(6);
    model.results.select_cell(2, 0);
    model.results_menu.open = true;
    model.results_menu.selected = index;
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    )
    .into_iter()
    .find_map(|effect| match effect {
        Effect::CopyToClipboard { text } => Some(text),
        _ => None,
    })
}

/// "Copy cell" copied a one-column table: pasting `2` gave `n` and `2` on two lines.
#[test]
fn copy_cell_copies_the_value_alone() {
    assert_eq!(copied_from_menu("copy-cell").as_deref(), Some("2"));
}

#[test]
fn the_copy_as_actions_copy_the_record() {
    assert_eq!(
        copied_from_menu("data.copy.json").as_deref(),
        Some("[\n  {\n    \"n\": 2\n  }\n]")
    );
    assert_eq!(copied_from_menu("data.copy.csv").as_deref(), Some("n\n2\n"));
    assert_eq!(
        copied_from_menu("data.copy.markdown").as_deref(),
        Some("| n |\n| --- |\n| 2 |\n")
    );
    assert!(
        copied_from_menu("data.copy.sql")
            .is_some_and(|sql| sql.starts_with("INSERT INTO") && sql.contains("(2)"))
    );
}

/// A copy that reached nowhere looked exactly like one that worked.
#[test]
fn a_finished_copy_says_so() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::ClipboardWritten {
            text: "a\nb\nc".into(),
        },
    );
    let toast = format!("{:?}", model.messages);
    assert!(toast.contains("copied 3 lines to clipboard"), "{toast}");
}
