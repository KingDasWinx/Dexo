//! The view follows the cursor only at the pane's edges. It assumed a pane 12 rows by
//! 80 columns whatever its size, so a taller one began scrolling halfway down.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use dexo_tui::action::Action;
use dexo_tui::{Focus, Model, update};

fn editor_with(text: &str, width: u16, height: u16) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.apply_size(width, height);
    model.active_document_mut().sql.insert(0, text).unwrap();
    model.active_document_mut().sql.set_cursor(0).unwrap();
    model
}

fn press(model: &mut Model, code: KeyCode) {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// Line numbers the frame shows, in order.
fn visible_lines(model: &Model) -> Vec<usize> {
    let frame = dexo_tui::render::render_to_string(model, model.width, model.height);
    (1..=500)
        .filter(|n| frame.contains(&format!("│{n:>4}")))
        .collect()
}

fn lines(count: usize) -> String {
    (1..=count)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn down_reaches_the_bottom_row_before_the_view_moves() {
    for (width, height) in [(160, 50), (100, 30), (70, 20)] {
        let mut model = editor_with(&lines(200), width, height);
        let rows = visible_lines(&model).len();
        assert!(rows > 12, "{width}x{height}: {rows} rows");

        for _ in 1..rows {
            press(&mut model, KeyCode::Down);
        }
        assert_eq!(
            model.active_document().viewport_line,
            0,
            "{width}x{height}: moved before the cursor reached row {rows}"
        );
        assert_eq!(visible_lines(&model).last(), Some(&rows));

        press(&mut model, KeyCode::Down);
        assert_eq!(model.active_document().viewport_line, 1, "{width}x{height}");
        assert_eq!(visible_lines(&model).last(), Some(&(rows + 1)));

        // Back up: nothing moves until the cursor reaches the top row.
        for _ in 0..rows - 1 {
            press(&mut model, KeyCode::Up);
        }
        assert_eq!(model.active_document().viewport_line, 1, "{width}x{height}");
        press(&mut model, KeyCode::Up);
        assert_eq!(model.active_document().viewport_line, 0, "{width}x{height}");
    }
}

#[test]
fn right_reaches_the_last_column_before_the_view_moves() {
    let long = "x".repeat(400);
    let mut model = editor_with(&long, 100, 30);
    press(&mut model, KeyCode::End);
    let at_end = model.active_document().viewport_column;
    assert!(at_end > 0);
    press(&mut model, KeyCode::Home);
    assert_eq!(model.active_document().viewport_column, 0);

    // The text area is the pane less its borders and gutter; walk to its last column.
    let mut steps = 0;
    while model.active_document().viewport_column == 0 {
        press(&mut model, KeyCode::Right);
        steps += 1;
        assert!(steps < 400);
    }
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    let row = frame.lines().find(|row| row.contains("   1")).unwrap();
    let shown = row.matches('x').count();
    assert_eq!(
        steps, shown,
        "scrolled after {steps} of {shown} columns:\n{frame}"
    );
    assert_eq!(model.active_document().viewport_column, 1);
    assert!(at_end >= 400 - shown);
}

/// The wheel looks elsewhere without the cursor; the next frame must not snap back.
#[test]
fn the_wheel_moves_the_view_and_it_stays_put() {
    let mut model = editor_with(&lines(200), 100, 30);
    let (column, row) = (60, 10);
    for _ in 0..5 {
        update(
            &mut model,
            Action::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }),
        );
    }
    let scrolled = model.active_document().viewport_line;
    assert!(scrolled > 0);
    update(&mut model, Action::RefreshSqlIntelligence);
    assert_eq!(model.active_document().viewport_line, scrolled);
}

/// PageUp and PageDown did nothing outside the completion popup.
#[test]
fn page_down_and_up_move_a_screenful() {
    let mut model = editor_with(&lines(200), 100, 30);
    let rows = visible_lines(&model).len();
    press(&mut model, KeyCode::PageDown);
    let doc = model.active_document();
    assert_eq!(doc.viewport_line, rows);
    assert_eq!(doc.cursor(), lines(rows).chars().count() + 1);
    press(&mut model, KeyCode::PageUp);
    let doc = model.active_document();
    assert_eq!(doc.viewport_line, 0);
    assert_eq!(doc.cursor(), 0);

    // Near the end the last page fills the screen rather than scrolling past it.
    for _ in 0..20 {
        press(&mut model, KeyCode::PageDown);
    }
    assert_eq!(visible_lines(&model).last(), Some(&200));
    assert_eq!(visible_lines(&model).len(), rows);
}
