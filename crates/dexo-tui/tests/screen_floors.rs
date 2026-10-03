//! Every screen keeps its list, its detail and its buttons at 80x24, and folds to one
//! column under eighty: the list, Enter for the pick's detail, Esc back.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::mouse::HitMap;
use dexo_tui::{Action, Model, update};

fn paint(model: &mut Model, width: u16, height: u16) -> String {
    model.apply_size(width, height);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, width, height)
}

fn press(model: &mut Model, code: KeyCode) {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn connections() -> Model {
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::from_u128(1)),
            None,
            "pg-dev",
            "postgres",
            "development",
            serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
            dexo_app::SecretRef::new("r".into()),
        )]);
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Connections),
    );
    model
}

fn history() -> Model {
    let mut model = Model::default();
    update(&mut model, Action::SearchHistory);
    update(
        &mut model,
        Action::HistoryLoaded(vec![dexo_storage::HistoryRow {
            id: "h1".into(),
            sql: "select count(*) from orders".into(),
            connection_id: Some("pg-dev".into()),
            created_at: "2026-10-03 12:00:00".into(),
            ..Default::default()
        }]),
    );
    model
}

fn server() -> Model {
    let mut model = Model::default();
    model.screen = dexo_tui::model::Screen::Server;
    model.admin = dexo_tui::screens::admin::AdminScreen::fixture();
    model
}

fn compare() -> Model {
    let mut model = Model::default();
    model.screen = dexo_tui::model::Screen::Compare;
    model.schema_diff = dexo_tui::screens::schema_diff::SchemaDiffScreen::fixture();
    model
}

fn profiles() -> Model {
    let mut model = Model::default();
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Agents),
    );
    model.agents_view = dexo_tui::screen::agents::AgentsView::Profiles;
    model.mcp_profiles = dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture();
    model
}

/// Each screen, the text of its first row, and its detail's first button.
fn screens() -> Vec<(&'static str, Model, &'static str, &'static str)> {
    vec![
        ("Connections", connections(), "pg-dev", "[⏎ Connect]"),
        ("History", history(), "select count(*)", "[⏎ Open]"),
        ("Server", server(), "[blocks 10]", "[k Cancel query]"),
        (
            "Compare",
            compare(),
            "db.public.orders_new",
            "[⏎ Open script]",
        ),
        ("Agents", profiles(), "assistant", "[e Enable]"),
    ]
}

#[test]
fn at_80x24_the_list_and_the_buttons_are_on_screen() {
    for (name, mut model, row, button) in screens() {
        let frame = paint(&mut model, 80, 24);
        assert!(frame.contains(row), "{name}: no first row: {frame}");
        assert!(frame.contains(button), "{name}: no {button}: {frame}");
    }
}

#[test]
fn under_80_columns_one_column_enter_for_the_detail_esc_back() {
    for (name, mut model, row, button) in screens() {
        let frame = paint(&mut model, 60, 20);
        assert!(frame.contains(row), "{name}: no list: {frame}");
        assert!(
            !frame.contains(button),
            "{name}: the detail is drawn too: {frame}"
        );
        assert!(frame.contains("Enter details"), "{name}: {frame}");
        press(&mut model, KeyCode::Enter);
        let frame = paint(&mut model, 60, 20);
        assert!(frame.contains(button), "{name}: no detail: {frame}");
        assert!(frame.contains("Esc the list"), "{name}: {frame}");
        press(&mut model, KeyCode::Esc);
        let frame = paint(&mut model, 60, 20);
        assert!(
            !frame.contains(button),
            "{name}: Esc did not go back: {frame}"
        );
        assert_eq!(model.screen.title(), name, "{name}: Esc left the screen");
    }
}
