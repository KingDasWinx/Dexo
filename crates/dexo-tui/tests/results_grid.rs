//! The results grid as a user drives it: wide results, the record view, moving about,
//! what NULL and a line break look like, and what a failed filter leaves behind.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::{ColumnMeta, DbValue};
use dexo_tui::action::Action;
use dexo_tui::model::{Focus, Model};
use dexo_tui::render::render_to_string;
use dexo_tui::update;

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn column(name: &str) -> ColumnMeta {
    ColumnMeta {
        name: name.into(),
        type_name: "text".into(),
        nullable: true,
    }
}

/// `id` and `c01..c39`: forty columns, five rows.
fn wide(rows: usize) -> Model {
    let mut model = Model::default();
    model.apply_size(120, 36);
    model.focus = Focus::Results;
    let mut columns = vec![column("id")];
    columns.extend((1..40).map(|n| column(&format!("c{n:02}"))));
    model.results.set_columns(columns);
    model.results.append_rows(
        (0..rows)
            .map(|row| {
                let mut cells = vec![DbValue::I64(row as i64 + 1)];
                cells.extend((1..40).map(|n| DbValue::Text(format!("v{n:02}"))));
                cells
            })
            .collect(),
    );
    model.results.select_cell(0, 0);
    model
}

/// Forty columns were squeezed to a character or two, so no name could be read; they keep
/// the width their values need and the grid scrolls sideways.
#[test]
fn a_wide_result_keeps_readable_columns_and_scrolls_sideways() {
    let mut model = wide(5);
    let screen = render_to_string(&model, 120, 36);
    assert!(screen.contains("c01") && screen.contains("c02"), "{screen}");
    assert!(screen.contains("v01"), "{screen}");
    assert!(
        !screen.contains("c39"),
        "everything squeezed into the pane:\n{screen}"
    );

    // End goes to the last column and the view follows it; Home comes back.
    update(&mut model, key(KeyCode::End));
    let screen = render_to_string(&model, 120, 36);
    assert!(
        screen.contains("c39"),
        "the last column is out of reach:\n{screen}"
    );
    assert!(!screen.contains("c01"), "{screen}");
    update(&mut model, key(KeyCode::Home));
    assert!(render_to_string(&model, 120, 36).contains("c01"));
}

/// Home and End, Ctrl+Home and Ctrl+End: the corners of a result, without a thousand
/// PageDowns.
#[test]
fn home_end_and_ctrl_home_end_reach_the_corners() {
    let mut model = wide(300);
    update(&mut model, ctrl(KeyCode::End));
    assert_eq!(model.results.cursor_row(), Some(299));
    update(&mut model, key(KeyCode::End));
    assert_eq!(model.results.selection().map(|(_, col)| col), Some(39));
    update(&mut model, ctrl(KeyCode::Home));
    assert_eq!(model.results.cursor_row(), Some(0));
    update(&mut model, key(KeyCode::Home));
    assert_eq!(model.results.selection(), Some((0, 0)));
}

/// The record view shows every field: a cursor walks them, the view follows it, and
/// Left and Right turn to the next record.
#[test]
fn the_record_view_reaches_every_field() {
    let mut model = wide(3);
    update(&mut model, key(KeyCode::Char('x')));
    assert!(model.expanded_records);
    let screen = render_to_string(&model, 120, 36);
    assert!(
        screen.contains("RECORD 1") && screen.contains("c01"),
        "{screen}"
    );
    assert!(!screen.contains("c39"), "{screen}");

    for _ in 0..39 {
        update(&mut model, key(KeyCode::Down));
    }
    let screen = render_to_string(&model, 120, 36);
    assert!(
        screen.contains("c39"),
        "the last field is out of reach:\n{screen}"
    );
    assert_eq!(model.results.selection(), Some((0, 39)));

    update(&mut model, key(KeyCode::Right));
    assert_eq!(
        model.results.cursor_row(),
        Some(1),
        "Right did not turn the record"
    );
}

/// NULL, the text `NULL` and an empty string are three things; a line break does not
/// move the cells after it.
#[test]
fn null_the_empty_string_and_line_breaks_are_drawn_as_themselves() {
    let mut model = Model::default();
    model.apply_size(120, 36);
    model
        .results
        .set_columns(vec![column("n"), column("t"), column("e"), column("x")]);
    model.results.append_rows(vec![
        vec![
            DbValue::Null,
            DbValue::Text("NULL".into()),
            DbValue::Text(String::new()),
            DbValue::Text("a\nb".into()),
        ],
        vec![
            DbValue::Text("1".into()),
            DbValue::Text("2".into()),
            DbValue::Text("3".into()),
            DbValue::Text("4".into()),
        ],
    ]);
    let screen = render_to_string(&model, 120, 36);
    assert!(
        screen.contains("\"\""),
        "the empty string is blank:\n{screen}"
    );
    assert!(
        screen.contains("a↵b"),
        "a line break is not drawn:\n{screen}"
    );
    // The two rows put the same cells in the same columns.
    let at = |needle: &str| {
        screen
            .lines()
            .find(|line| line.contains(needle))
            .and_then(|line| line.find(needle).map(|at| line[..at].chars().count()))
    };
    assert_eq!(at("a↵b"), at("4"));
}

/// Esc shrinks a selection of rows back to the cursor.
#[test]
fn esc_clears_a_row_selection() {
    let mut model = wide(5);
    update(&mut model, Action::ResultsExtendDown);
    update(&mut model, Action::ResultsExtendDown);
    assert!(model.results.row_selected(1) && model.results.row_selected(2));
    update(&mut model, key(KeyCode::Esc));
    assert!(!model.results.row_selected(1), "Esc left the rows selected");
    assert_eq!(model.results.cursor_row(), Some(2));
}

/// A result with columns and no rows says so, instead of showing a header alone.
#[test]
fn an_empty_result_says_it_has_no_rows() {
    let mut model = Model::default();
    model.apply_size(120, 36);
    model.results.set_columns(vec![column("id")]);
    assert!(render_to_string(&model, 120, 36).contains("no rows"));
}

/// `n` and `p` say where they stand instead of doing nothing.
#[test]
fn paging_keys_say_so_when_there_is_no_other_page() {
    let mut model = wide(5);
    model.active_session = Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1)));
    let effects = update(&mut model, key(KeyCode::Char('n')));
    assert!(effects.is_empty());
    assert!(
        model
            .messages
            .last()
            .unwrap()
            .message
            .contains("not in pages"),
        "{:?}",
        model.messages.last()
    );
    // Neither does the status line offer what it cannot do.
    let screen = render_to_string(&model, 120, 36);
    assert!(!screen.contains("n/p page"), "{screen}");
}

/// A WHERE the server refuses leaves the bar showing it as refused, not as the filter on
/// the rows that are still there.
#[test]
fn a_refused_filter_is_marked_and_keeps_its_text() {
    let mut model = wide(5);
    model.data.bars.where_input.set_text("nonexistent = 1");
    assert!(
        !model
            .data
            .bars
            .refused(dexo_tui::screens::data::ClauseBar::Where)
    );
    model.data.bars.failed = true;
    assert!(
        model
            .data
            .bars
            .refused(dexo_tui::screens::data::ClauseBar::Where)
    );
    // What ran is the text, so it is not refused.
    model.data.bars.applied.where_sql = Some("nonexistent = 1".into());
    assert!(
        !model
            .data
            .bars
            .refused(dexo_tui::screens::data::ClauseBar::Where)
    );
}
