//! The Add and Edit connection form, as a person meets it: the driver clicked, the
//! message where the eye is, a Test that answers in the form, a save that closes it.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::action::{Action, Effect};
use dexo_tui::mouse::HitMap;
use dexo_tui::{Model, update};

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 36)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, 120, 36)
}

fn click(model: &mut Model, column: u16, row: u16) {
    update(
        model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

/// Where `needle` is on the painted screen, as the 0-based cell a click is sent to.
fn find(screen: &str, needle: &str) -> (u16, u16) {
    for (row, line) in screen.lines().enumerate() {
        if let Some(at) = line.find(needle) {
            return (line[..at].chars().count() as u16, row as u16);
        }
    }
    panic!("{needle:?} is not on the screen:\n{screen}");
}

fn field(model: &Model, label: &str) -> String {
    model
        .connection_form
        .fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.as_str().to_string())
        .unwrap_or_default()
}

fn open_form() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 36);
    update(&mut model, Action::OpenConnectionForm);
    model
}

/// Every other field and both buttons answered a click; the driver row did nothing, so a
/// mouse-only user could not choose MySQL.
#[test]
fn clicking_the_arrows_of_the_driver_picks_it_and_the_port_follows() {
    let mut model = open_form();
    let screen = paint(&mut model);
    let (column, row) = find(&screen, "< PostgreSQL >");
    assert_eq!(field(&model, "port"), "5432");
    // The closing `>` is the next driver, the opening `<` the one before.
    click(
        &mut model,
        column + "< PostgreSQL ".chars().count() as u16,
        row,
    );
    assert_eq!(field(&model, "driver"), "mysql");
    assert_eq!(field(&model, "port"), "3306", "the port follows the driver");
    let screen = paint(&mut model);
    let (column, row) = find(&screen, "< MySQL >");
    click(&mut model, column, row);
    assert_eq!(field(&model, "driver"), "postgres");
    assert_eq!(field(&model, "port"), "5432");
}

#[test]
fn clicking_the_row_of_a_choice_focuses_it_without_changing_it() {
    let mut model = open_form();
    let screen = paint(&mut model);
    let (column, row) = find(&screen, "driver:");
    click(&mut model, column + 2, row);
    assert_eq!(
        model.connection_form.focus,
        model
            .connection_form
            .fields
            .iter()
            .position(|field| field.label == "driver")
            .unwrap()
    );
    assert_eq!(field(&model, "driver"), "postgres");
}

/// The message is in a row of its own above the buttons: an error on the Advanced part of
/// the form used to be drawn after the last field, off screen, and Enter looked dead.
#[test]
fn an_error_is_seen_while_the_advanced_fields_are_scrolled() {
    let mut model = open_form();
    model.connection_form.set_advanced(true);
    for _ in 0..30 {
        press(&mut model, KeyCode::Tab);
    }
    model
        .connection_form
        .set_error("port must be a number from 1 to 65535".into());
    let screen = paint(&mut model);
    assert!(screen.contains("port must be a number"), "{screen}");
    assert!(screen.contains("[Submit]"), "{screen}");
}

/// A test run from the form answers in the form, where the user is looking, not in a toast
/// behind it.
#[test]
fn a_test_from_the_form_answers_in_the_form() {
    let mut model = open_form();
    for (label, value) in [
        ("name", "x"),
        ("host", "db"),
        ("database", "d"),
        ("username", "u"),
        ("password", "p"),
    ] {
        model.connection_form.set_value(label, value);
    }
    let effects = update(&mut model, Action::TestConnection);
    assert!(
        matches!(effects.as_slice(), [Effect::TestConnection { .. }]),
        "{effects:?}"
    );
    update(
        &mut model,
        Action::ConnectionTested {
            name: "x".into(),
            ok: false,
            message: "could not connect to db:5432: connection refused".into(),
        },
    );
    let screen = paint(&mut model);
    assert!(
        screen.contains("could not connect to db:5432: connection refused"),
        "{screen}"
    );
    assert!(model.connection_form.open, "the form is still open");
}

/// A connection that was saved is saved, whatever the first connect says: the form
/// stayed open on a save that had worked, and the next Submit said "already exists".
#[test]
fn a_saved_connection_closes_the_form_even_when_it_cannot_connect() {
    let mut model = open_form();
    let saved = ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(3)),
        None,
        "x3",
        "postgres",
        "local",
        serde_json::json!({"host": "127.0.0.1", "port": 55601, "username": "u", "database": "d"}),
        SecretRef::new("ref-3".into()),
    );
    update(&mut model, Action::ProfileSaved(saved));
    assert!(!model.connection_form.open);
    // The connect that follows fails: that is a message, not a form to keep open.
    update(
        &mut model,
        Action::ConnectionFormError {
            message: "x3: could not connect to 127.0.0.1:55601: connection refused".into(),
        },
    );
    let screen = paint(&mut model);
    assert!(
        screen.contains("could not connect to 127.0.0.1:55601"),
        "{screen}"
    );
}

/// The password typed in the Edit form is saved with the connection: it said "saved" and
/// kept the old one.
#[test]
fn a_password_typed_in_edit_goes_with_the_save() {
    let mut model = Model::default();
    model.apply_size(120, 36);
    let profile = ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(4)),
        None,
        "pg",
        "postgres",
        "local",
        serde_json::json!({"host": "db", "port": 5432, "username": "u", "database": "d"}),
        SecretRef::new("ref-4".into()),
    );
    model.connections.load_profiles(vec![profile]);
    update(&mut model, Action::EditSelectedConnection);
    assert!(model.connection_form.open);
    model.connection_form.set_value("password", "new-secret");
    let effects = update(&mut model, Action::SaveConnection);
    match effects.as_slice() {
        [Effect::SaveProfile { password, .. }] => assert_eq!(password, "new-secret"),
        other => panic!("{other:?}"),
    }
    // Left empty, it asks for nothing: the saved one is kept.
    update(&mut model, Action::EditSelectedConnection);
    let effects = update(&mut model, Action::SaveConnection);
    match effects.as_slice() {
        [Effect::SaveProfile { password, .. }] => assert!(password.is_empty()),
        other => panic!("{other:?}"),
    }
}
