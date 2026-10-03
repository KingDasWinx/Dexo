//! New document picks its connection: the one suggested, or any other saved one with
//! Left and Right on its row. It could only be the connection in view.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Effect, Focus, Model, update};

fn profile(name: &str, n: u128) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(n)),
        None,
        name,
        "postgres",
        "local",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new(format!("r{n}")),
    )
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn new_document(names: &[&str]) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.connections.load_profiles(
        names
            .iter()
            .enumerate()
            .map(|(n, name)| profile(name, n as u128 + 1))
            .collect(),
    );
    model.connection.name = names[0].into();
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
    );
    assert!(model.document_name_prompt.open);
    model
}

fn first_line(model: &Model) -> String {
    model.document_name_prompt.lines()[0].clone()
}

#[test]
fn up_then_right_picks_another_connection_for_the_document() {
    let mut model = new_document(&["alpha", "beta"]);
    assert!(
        first_line(&model).contains("< alpha >"),
        "{}",
        first_line(&model)
    );

    press(&mut model, KeyCode::Up);
    press(&mut model, KeyCode::Right);
    assert!(
        first_line(&model).contains("< beta >"),
        "{}",
        first_line(&model)
    );
    press(&mut model, KeyCode::Enter);
    assert!(
        model.document_name_prompt.on_name(),
        "Enter went on to the name"
    );
    let effects = press(&mut model, KeyCode::Enter);

    assert!(!model.document_name_prompt.open);
    assert_eq!(
        model.active_document().connection_id.as_deref(),
        Some(uuid::Uuid::from_u128(2).to_string().as_str())
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ConnectProfile { profile, .. } if profile.name == "beta"
        )),
        "{effects:?}"
    );
}

#[test]
fn left_goes_round_and_typing_on_the_row_names_nothing() {
    let mut model = new_document(&["alpha", "beta", "gamma"]);
    press(&mut model, KeyCode::Up);

    press(&mut model, KeyCode::Left);
    assert!(first_line(&model).contains("< gamma >"));
    press(&mut model, KeyCode::Char('x'));

    assert_eq!(model.document_name_prompt.name.as_str(), "query-1.sql");
}

#[test]
fn a_single_connection_is_said_not_picked() {
    let mut model = new_document(&["alpha"]);
    assert_eq!(first_line(&model), "connection: alpha");

    press(&mut model, KeyCode::Up);

    assert!(!model.document_name_prompt.on_connection);
}

#[test]
fn a_click_on_an_arrow_picks() {
    let mut model = new_document(&["alpha", "beta"]);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let (column, row) = model
        .hits
        .center(HitTarget::FormChoice { index: 1, step: 1 });
    assert_ne!((column, row), (0, 0), "no arrow to click");

    update(
        &mut model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );

    assert!(
        first_line(&model).contains("< beta >"),
        "{}",
        first_line(&model)
    );
}

/// With no connection in view the document is for none, and the saved ones can be
/// picked: the dialog said to pick one in the explorer.
#[test]
fn with_none_in_view_a_saved_one_can_be_picked() {
    let mut model = new_document(&["alpha"]);
    model.connection.name.clear();
    update(&mut model, Action::NewDocument);
    assert!(
        first_line(&model).contains("< none >"),
        "{}",
        first_line(&model)
    );

    press(&mut model, KeyCode::Up);
    press(&mut model, KeyCode::Right);

    assert_eq!(first_line(&model), "connection: alpha");
    assert!(!model.document_name_prompt.on_connection);
}
