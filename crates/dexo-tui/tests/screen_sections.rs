//! Alt+1 and Alt+2 go to a screen's list and its detail, as they go to the workbench's
//! panes. On the detail the arrows read it; on the list they pick. They only said the
//! keys worked on the workbench.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::model::Screen;
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::screen::Section;
use dexo_tui::{Action, Model, update};

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) {
    update(model, Action::Key(KeyEvent::new(code, modifiers)));
    paint(model);
}

fn alt(model: &mut Model, digit: char) {
    press(model, KeyCode::Char(digit), KeyModifiers::ALT);
}

fn paint(model: &mut Model) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn row(sql: &str) -> dexo_storage::HistoryRow {
    dexo_storage::HistoryRow {
        sql: sql.into(),
        connection_id: Some("pg-dev".into()),
        created_at: "2026-10-03 12:00:00".into(),
    }
}

/// History with a statement longer than its pane, and a short one.
fn history() -> Model {
    let mut model = Model::default();
    model.connection.name = "pg-dev".into();
    update(&mut model, Action::SearchHistory);
    let long: Vec<String> = (0..60).map(|line| format!("select {line}")).collect();
    update(
        &mut model,
        Action::HistoryLoaded(vec![row(&long.join("\n")), row("select 1")]),
    );
    assert_eq!(model.screen, Screen::History);
    paint(&mut model);
    model
}

fn section(model: &Model) -> Section {
    dexo_tui::screen::section(model)
}

fn said(model: &Model) -> Vec<String> {
    model
        .messages
        .iter()
        .map(|message| message.message.clone())
        .collect()
}

#[test]
fn alt_2_reads_the_detail_and_alt_1_picks_again() {
    let mut model = history();
    assert_eq!(section(&model), Section::List);

    alt(&mut model, '2');
    assert_eq!(section(&model), Section::Detail);
    press(&mut model, KeyCode::Down, KeyModifiers::NONE);
    press(&mut model, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(model.detail_scroll, 2);
    assert_eq!(model.editor.history_selected, 0, "the pick stayed");
    let screen = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(screen.contains("Up/Down read"), "{screen}");
    assert!(screen.contains("select 2"), "{screen}");
    assert!(
        !screen.contains("select 1\n"),
        "the top was read past: {screen}"
    );

    alt(&mut model, '1');
    press(&mut model, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(section(&model), Section::List);
    assert_eq!(model.editor.history_selected, 1);
    assert_eq!(model.detail_scroll, 0, "another pick reads from the top");
}

#[test]
fn a_key_for_a_pane_the_screen_lacks_says_which_it_has() {
    let mut model = history();

    alt(&mut model, '3');

    assert!(
        said(&model)
            .iter()
            .any(|line| line.contains("History has two panes: Alt+1 and Alt+2")),
        "{:?}",
        said(&model)
    );
    assert_eq!(section(&model), Section::List);
}

#[test]
fn a_click_on_the_detail_gives_it_the_keys() {
    let mut model = history();
    let (column, row) = model.hits.center(HitTarget::ScreenDetail);

    update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );

    assert_eq!(section(&model), Section::Detail);
}

#[test]
fn the_connection_form_keeps_the_keys_until_it_closes() {
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::nil()),
            None,
            "prod",
            "postgres",
            "local",
            serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
            dexo_app::SecretRef::new("ref".into()),
        )]);
    update(&mut model, Action::OpenConnectionForm);
    paint(&mut model);
    assert_eq!(section(&model), Section::Detail);

    alt(&mut model, '1');

    assert!(model.connection_form.open);
    assert_eq!(section(&model), Section::Detail);
    assert!(
        said(&model)
            .iter()
            .any(|line| line.contains("The form has the keys")),
        "{:?}",
        said(&model)
    );

    press(&mut model, KeyCode::Esc, KeyModifiers::NONE);
    alt(&mut model, '1');

    assert!(!model.connection_form.open);
    assert_eq!(section(&model), Section::List);
}

#[test]
fn the_sections_are_kept_per_screen() {
    let mut model = history();
    alt(&mut model, '2');

    update(&mut model, Action::GoToScreen(Screen::Agents));
    paint(&mut model);
    assert_eq!(section(&model), Section::List);
    update(&mut model, Action::GoToScreen(Screen::History));
    paint(&mut model);

    assert_eq!(section(&model), Section::Detail);
}
