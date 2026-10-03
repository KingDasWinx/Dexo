//! The record modal's copy actions each put the record on the clipboard.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::GridModel;
use dexo_tui::{Effect, Model, update};

fn copied_from_menu(id: &str) -> Option<String> {
    let index = dexo_tui::palette::results_menu_items(false)
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
    assert_eq!(copied_from_menu("data.copy.cell").as_deref(), Some("2"));
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

fn three_by_three() -> Model {
    use dexo_driver_api::{ColumnMeta, DbValue};
    let mut model = Model::default();
    model.results.set_columns(
        ["a", "b", "c"]
            .map(|name| ColumnMeta {
                name: name.into(),
                type_name: "int8".into(),
                nullable: false,
            })
            .to_vec(),
    );
    model.results.append_rows(
        (0..3)
            .map(|row| (0..3).map(|col| DbValue::I64(row * 3 + col + 1)).collect())
            .collect(),
    );
    model.results.select_cell(1, 0);
    model
}

fn copied(model: &mut Model, action: Action) -> Option<String> {
    update(model, action)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::CopyToClipboard { text } => Some(text),
            _ => None,
        })
}

/// The same words copy the same rows from the palette and from the Enter menu: the
/// cursor's row, not the one cell under it.
#[test]
fn copy_as_from_the_palette_takes_the_row_like_the_menu() {
    use dexo_app::data::CopyFormat;
    let mut model = three_by_three();
    let json = copied(&mut model, Action::CopyGrid(CopyFormat::Json)).unwrap();
    assert!(
        json.contains("\"a\": 4") && json.contains("\"c\": 6"),
        "{json}"
    );
    let mut model = three_by_three();
    assert_eq!(
        copied(&mut model, Action::CopyGrid(CopyFormat::Csv)).as_deref(),
        Some("a,b,c\n4,5,6\n")
    );
    let mut model = three_by_three();
    assert_eq!(
        copied(&mut model, Action::CopyGrid(CopyFormat::Value)).as_deref(),
        Some("4")
    );
}

/// The toast says what went to the clipboard.
#[test]
fn the_copy_toast_says_what_was_copied() {
    use dexo_app::data::CopyFormat;
    let toast = |format: CopyFormat| {
        let mut model = three_by_three();
        let text = copied(&mut model, Action::CopyGrid(format)).unwrap();
        update(&mut model, Action::ClipboardWritten { text });
        model.messages.last().unwrap().message.clone()
    };
    assert_eq!(toast(CopyFormat::Value), "copied the cell");
    assert_eq!(toast(CopyFormat::Csv), "copied 1 row, 3 columns as CSV");
}

/// Ctrl+C in the grid copies the cell, as it copies the selection in the editor.
#[test]
fn ctrl_c_in_the_grid_copies_the_cell() {
    let mut model = three_by_three();
    model.focus = dexo_tui::model::Focus::Results;
    let effects = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CopyToClipboard { text } if text == "4"))
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

/// On a query's result there is no cell to edit: the row's menu does not offer it, and F2
/// says why. Both renamed the document.
#[test]
fn a_query_result_offers_no_cell_edit_and_f2_renames_nothing() {
    assert!(
        !dexo_tui::palette::results_menu_items(false)
            .iter()
            .any(|(id, _)| *id == "data.edit_cell")
    );
    assert!(
        dexo_tui::palette::results_menu_items(true)
            .iter()
            .any(|(id, _)| *id == "data.edit_cell")
    );
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    *model.results = GridModel::sample_rows(3);
    model.results.select_cell(1, 0);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
    );
    assert!(!model.document_name_prompt.open);
    assert!(
        model
            .messages
            .last()
            .is_some_and(|message| message.message.contains("query's result"))
    );
}
