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
