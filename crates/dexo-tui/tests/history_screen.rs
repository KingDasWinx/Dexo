//! History is every connection's, filtered by connection, status and text, by day, with
//! the run in fields under its statement. It was the connection in use's alone, a
//! statement to a line, and kept no failure.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_storage::{HistoryOutcome, HistoryRow};
use dexo_tui::mouse::HitMap;
use dexo_tui::{Action, Effect, Model, update};

fn utc(ago: chrono::Duration) -> String {
    (chrono::Local::now() - ago)
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn row(connection: &str, sql: &str, outcome: HistoryOutcome, ago: chrono::Duration) -> HistoryRow {
    HistoryRow {
        id: format!("{connection}-{sql}"),
        sql: sql.into(),
        connection_id: Some(connection.into()),
        created_at: utc(ago),
        outcome,
        duration_ms: Some(11),
        rows: (outcome == HistoryOutcome::Ok).then_some(1),
        error: (outcome == HistoryOutcome::Failed)
            .then(|| "relation \"nope\" does not exist".into()),
        database: Some("qa0".into()),
    }
}

fn history() -> Model {
    let mut model = Model::default();
    let effects = update(&mut model, Action::SearchHistory);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadHistory {
                connection_id: None
            }
        )),
        "every connection's: {effects:?}"
    );
    let minute = chrono::Duration::minutes(1);
    update(
        &mut model,
        Action::HistoryLoaded(vec![
            row(
                "pg-dev",
                "select * from nope",
                HistoryOutcome::Failed,
                minute,
            ),
            row(
                "pg-dev",
                "select count(*) from orders",
                HistoryOutcome::Ok,
                minute * 2,
            ),
            row("my-dev", "select now()", HistoryOutcome::Ok, minute * 3),
            row(
                "pg-dev",
                "select count(*) from orders",
                HistoryOutcome::Ok,
                minute * 4,
            ),
            row(
                "shop",
                "select * from t",
                HistoryOutcome::Ok,
                chrono::Duration::hours(25),
            ),
        ]),
    );
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

fn shown(model: &Model) -> Vec<String> {
    model
        .editor
        .history_lines()
        .iter()
        .map(|line| line.row.sql.clone())
        .collect()
}

#[test]
fn every_connections_statements_show_with_none_connected_each_once() {
    let mut model = history();
    assert_eq!(
        shown(&model),
        [
            "select * from nope",
            "select count(*) from orders",
            "select now()",
            "select * from t"
        ]
    );
    let frame = paint(&mut model);
    assert!(frame.contains("✗"), "{frame}");
    assert!(frame.contains("my-dev"), "{frame}");
    assert!(frame.contains("11 ms"), "{frame}");
}

#[test]
fn c_narrows_to_a_connection_and_f_to_the_failures() {
    let mut model = history();
    let mut seen = Vec::new();
    for _ in 0..4 {
        press(&mut model, KeyCode::Char('c'));
        seen.push((model.editor.history_connection.clone(), shown(&model)));
    }
    assert!(seen.contains(&(Some("my-dev".into()), vec!["select now()".to_string()])));
    assert!(seen.contains(&(Some("shop".into()), vec!["select * from t".to_string()])));
    assert_eq!(seen.last().unwrap().0, None, "and back to all");
    let frame = paint(&mut model);
    assert!(frame.contains("Connection: all"), "{frame}");
    press(&mut model, KeyCode::Char('f'));
    press(&mut model, KeyCode::Char('f'));
    assert_eq!(shown(&model), ["select * from nope"]);
    let frame = paint(&mut model);
    assert!(frame.contains("Status: failed"), "{frame}");
}

#[test]
fn the_search_is_typed_into_after_slash_and_its_letters_are_text() {
    let mut model = history();
    press(&mut model, KeyCode::Char('/'));
    for ch in "xcount".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert!(shown(&model).is_empty());
    press(&mut model, KeyCode::Home);
    press(&mut model, KeyCode::Delete);
    assert_eq!(shown(&model), ["select count(*) from orders"]);
    press(&mut model, KeyCode::Enter);
    assert!(!model.editor.history_search.typing);
    // Esc clears the search, then leaves.
    press(&mut model, KeyCode::Esc);
    assert_eq!(shown(&model).len(), 4);
    press(&mut model, KeyCode::Esc);
    assert_ne!(model.screen, dexo_tui::model::Screen::History);
}

#[test]
fn the_list_is_by_day_and_the_detail_is_the_run_in_fields() {
    let mut model = history();
    let frame = paint(&mut model);
    assert!(frame.contains("Today"), "{frame}");
    assert!(frame.contains("Yesterday"), "{frame}");
    for label in ["Connection", "When", "Took", "Result", "Runs"] {
        assert!(frame.contains(&format!(" {label} ")), "{label}: {frame}");
    }
    assert!(
        frame.contains("relation \"nope\" does not exist"),
        "{frame}"
    );
    press(&mut model, KeyCode::Down);
    let frame = paint(&mut model);
    assert!(frame.contains("pg-dev (qa0)"), "{frame}");
    assert!(frame.contains(" Rows "), "{frame}");
    let runs = frame
        .lines()
        .find(|line| line.contains(" Runs "))
        .unwrap_or_default();
    assert!(runs.contains('2'), "{runs}");
}

#[test]
fn nothing_matching_offers_to_clear_the_filters() {
    let mut model = history();
    model.editor.history_search.input.set_text("zzz");
    let frame = paint(&mut model);
    assert!(frame.contains("Nothing matches the filters."), "{frame}");
    assert!(frame.contains("[Esc Clear filters]"), "{frame}");
}

/// The history above, with pg-dev saved and the project open.
fn history_with_connections() -> Model {
    let mut model = history();
    model.project_id = "project-1".into();
    model
        .connections
        .load_profiles(vec![dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::from_u128(7)),
            None,
            "pg-dev",
            "postgres",
            "development",
            serde_json::json!({"host":"h","port":5432,"username":"u","database":"qa0"}),
            dexo_app::SecretRef::new("r".into()),
        )]);
    paint(&mut model);
    model
}

#[test]
fn the_runs_buttons_are_over_its_detail() {
    let mut model = history_with_connections();
    let frame = paint(&mut model);
    for button in [
        "[⏎ Open]",
        "[r Run again]",
        "[y Copy]",
        "[s Save…]",
        "[x Delete]",
        "[C Clear…]",
    ] {
        assert!(frame.contains(button), "{button}: {frame}");
    }
}

#[test]
fn open_and_run_again_put_the_statement_on_its_connection() {
    let mut model = history_with_connections();
    press(&mut model, KeyCode::Enter);
    assert_eq!(model.screen, dexo_tui::model::Screen::Workbench);
    assert_eq!(model.active_document().text(), "select * from nope");
    let pg_dev = uuid::Uuid::from_u128(7).to_string();
    assert_eq!(
        model.active_document().connection_id.as_deref(),
        Some(pg_dev.as_str())
    );

    let mut model = history_with_connections();
    let effects = press(&mut model, KeyCode::Char('r'));
    assert_eq!(model.active_document().text(), "select * from nope");
    // pg-dev is not connected: it is dialled, once, and the run waits for it.
    let dials = effects
        .iter()
        .filter(|effect| matches!(effect, Effect::ConnectProfile { .. }))
        .count();
    assert_eq!(dials, 1, "{effects:?}");
    assert!(model.pending_execute.is_some());
}

#[test]
fn copy_save_and_delete_act_on_the_pick() {
    let mut model = history_with_connections();
    let effects = press(&mut model, KeyCode::Char('y'));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::CopyToClipboard { text } if text == "select * from nope"
    )));
    press(&mut model, KeyCode::Char('s'));
    let prompt = model.save_query_prompt.as_ref().expect("asks for a name");
    assert_eq!(prompt.sql, "select * from nope");
    assert_eq!(prompt.connection_id, uuid::Uuid::from_u128(7).to_string());
    model.save_query_prompt = None;

    // count(*) ran twice on pg-dev: both runs go.
    press(&mut model, KeyCode::Down);
    let effects = press(&mut model, KeyCode::Char('x'));
    let ids = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::DeleteHistory { ids } => Some(ids.clone()),
            _ => None,
        })
        .expect("deletes");
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(
        !model
            .editor
            .history_lines()
            .iter()
            .any(|line| line.row.sql == "select count(*) from orders")
    );
}

#[test]
fn a_statement_of_a_connection_no_longer_saved_cannot_be_saved_and_says_why() {
    let mut model = history_with_connections();
    // my-dev's statement: no such saved connection.
    press(&mut model, KeyCode::Down);
    press(&mut model, KeyCode::Down);
    press(&mut model, KeyCode::Char('s'));
    assert!(model.save_query_prompt.is_none());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.message.contains("saved connection"))
    );
}

#[test]
fn clear_asks_naming_how_many_go_and_clears_what_is_shown() {
    let mut model = history_with_connections();
    press(&mut model, KeyCode::Char('c'));
    assert_eq!(model.editor.history_connection.as_deref(), Some("pg-dev"));
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT)),
    );
    let frame = paint(&mut model);
    assert!(frame.contains("Clear the 2 statements shown?"), "{frame}");
    // Cancel has the focus.
    let kept = press(&mut model, KeyCode::Enter);
    assert!(
        !kept
            .iter()
            .any(|effect| matches!(effect, Effect::DeleteHistory { .. }))
    );
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT)),
    );
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    let ids = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::DeleteHistory { ids } => Some(ids.clone()),
            _ => None,
        })
        .expect("clears");
    assert_eq!(ids.len(), 3, "pg-dev's three runs: {ids:?}");
    assert_eq!(model.editor.history.len(), 2, "the others stay");
}

#[test]
fn saved_queries_search_after_slash_filter_by_connection_and_take_letters() {
    let mut model = history_with_connections();
    press(&mut model, KeyCode::Tab);
    let pg_dev = uuid::Uuid::from_u128(7).to_string();
    let saved = |id: &str, name: &str, connection: &str| dexo_storage::SavedQuery {
        id: id.into(),
        connection_id: connection.into(),
        name: name.into(),
        sql: format!("select '{name}'"),
    };
    update(
        &mut model,
        Action::SavedQueriesLoaded(Ok(vec![
            saved("q1", "Late orders", &pg_dev),
            saved("q2", "Elsewhere", "another"),
        ])),
    );
    let frame = paint(&mut model);
    assert!(frame.contains("/ search"), "{frame}");
    assert!(frame.contains("[c Connection: all]"), "{frame}");
    for button in [
        "[⏎ Open]",
        "[r Run]",
        "[y Copy]",
        "[F2 Rename]",
        "[x Delete]",
    ] {
        assert!(frame.contains(button), "{button}: {frame}");
    }
    press(&mut model, KeyCode::Char('c'));
    assert_eq!(model.saved_queries.filtered().len(), 1);
    press(&mut model, KeyCode::Char('c'));
    press(&mut model, KeyCode::Char('c'));
    assert_eq!(model.saved_queries.filtered().len(), 2);
    press(&mut model, KeyCode::Char('/'));
    for ch in "late".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    press(&mut model, KeyCode::Enter);
    assert_eq!(model.saved_queries.filtered().len(), 1);
    let effects = press(&mut model, KeyCode::Char('y'));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::CopyToClipboard { text } if text == "select 'Late orders'"
    )));
    press(&mut model, KeyCode::Char('x'));
    assert!(model.saved_queries.deleting.is_some());
}

/// A statement whose connection is gone -- renamed, deleted, a temporary one -- is not run:
/// it would have run on the connection in use instead.
#[test]
fn run_again_refuses_a_statement_whose_connection_is_gone() {
    let mut model = history_with_connections();
    // my-dev's statement: no connection goes by that name.
    press(&mut model, KeyCode::Down);
    press(&mut model, KeyCode::Down);
    let effects = press(&mut model, KeyCode::Char('r'));
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::StartScript(_) | Effect::ConnectProfile { .. }
        )),
        "{effects:?}"
    );
    assert!(model.pending_execute.is_none());
    assert_eq!(model.screen, dexo_tui::model::Screen::History);
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.message.contains("not a connection any more"))
    );
}

/// A paste into History's search goes into the search: it went into the hidden editor
/// document, which kept the keys' focus behind the screen.
#[test]
fn a_paste_goes_into_the_search_not_the_hidden_document() {
    let mut model = history();
    let before = model.active_document().text();
    press(&mut model, KeyCode::Char('/'));
    update(&mut model, Action::Paste("count".into()));
    assert_eq!(model.editor.history_search.input.as_str(), "count");
    assert_eq!(model.active_document().text(), before);
    // With no field typed in, a paste does nothing at all: its letters are not keys.
    press(&mut model, KeyCode::Enter);
    let effects = update(&mut model, Action::Paste("x".into()));
    assert!(effects.is_empty());
    assert_eq!(model.editor.history.len(), 5, "x deleted a statement");
    assert_eq!(model.active_document().text(), before);
}
