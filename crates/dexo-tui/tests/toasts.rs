use dexo_tui::{Action, Model, update};

/// A long refusal ran off the right edge mid-word, with no sign that more was said.
#[test]
fn a_long_toast_wraps_inside_the_screen_and_ends_in_an_ellipsis_when_it_must() {
    let mut model = Model::default();
    model.messages.error(
        "Not run: pg-readonly is read-only, and EXPLAIN ANALYZE would run a statement that is \
         not a read: DELETE FROM order_items WHERE order_id IN (SELECT id FROM orders WHERE \
         status = 'cancelled' AND created_at < now() - interval '30 days') and then some more \
         words that no toast has room for, so the end of this sentence is cut."
            .into(),
    );

    let frame = dexo_tui::render::render_to_string(&model, 80, 24);

    let toast_rows: Vec<&str> = frame
        .lines()
        .filter(|line| line.contains('│') && line.trim_end().ends_with('│'))
        .collect();
    assert!(!toast_rows.is_empty(), "{frame}");
    assert!(frame.contains("Not run: pg-readonly"), "{frame}");
    assert!(frame.contains('…'), "no ellipsis:\n{frame}");
    for line in frame.lines() {
        assert!(line.chars().count() <= 80, "{line}");
    }
}

#[test]
fn a_short_toast_is_one_line_and_has_no_ellipsis() {
    let mut model = Model::default();
    model.messages.info("Connected to pg-dev".into());

    let frame = dexo_tui::render::render_to_string(&model, 80, 24);

    assert!(frame.contains("Connected to pg-dev"), "{frame}");
    assert!(!frame.contains('…'), "{frame}");
}

/// The red toast stayed over the editor until Esc, minutes after the statement failed.
#[test]
fn an_error_toast_goes_away_by_itself() {
    let mut model = Model::default();
    model
        .messages
        .error("syntax error at or near \"selec\"".into());

    for _ in 0..20 {
        update(&mut model, Action::ToastTick);
    }

    assert!(model.messages.toast.is_none());
    assert_eq!(model.messages.len(), 1, "the log keeps the error");
}

#[test]
fn a_later_message_replaces_an_error_toast() {
    let mut model = Model::default();
    model
        .messages
        .error("current transaction is aborted".into());
    model.messages.info("Rolled back to savepoint sp1".into());

    let frame = dexo_tui::render::render_to_string(&model, 100, 24);

    assert!(frame.contains("Rolled back"), "{frame}");
    assert!(!frame.contains("aborted"), "{frame}");
}
