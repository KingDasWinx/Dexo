//! Deleting a connection asked with a line of text tacked under the list -- `k keep
//! secrets  d delete secrets  esc cancel` -- with no buttons, no arrow keys, and `d`
//! meaning "duplicate" one keypress earlier.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_tui::action::Action;
use dexo_tui::mouse::{HitButton, HitMap, HitTarget};
use dexo_tui::{Effect, Model, update};

fn profile(name: &str) -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::new_v4()),
        None,
        name,
        "postgres",
        "local",
        serde_json::json!({}),
        SecretRef::new(format!("ref-{name}")),
    )
}

fn with_connections(names: &[&str]) -> Model {
    let mut model = Model::default();
    model
        .connections
        .load_profiles(names.iter().map(|name| profile(name)).collect());
    model.screen = dexo_tui::model::Screen::Connections;
    model
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn paint(model: &mut Model, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, width, height)
}

fn click(model: &mut Model, target: HitTarget) -> Vec<Effect> {
    let (column, row) = model.hits.center(target);
    update(
        model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    )
}

fn deletes(effects: &[Effect]) -> Option<(String, bool)> {
    effects.iter().find_map(|effect| match effect {
        Effect::DeleteProfile {
            profile,
            delete_secrets,
        } => Some((profile.name.clone(), *delete_secrets)),
        _ => None,
    })
}

fn asking_to_delete_prod() -> Model {
    let mut model = with_connections(&["local", "prod"]);
    model.connections.selected_profile = 1;
    press(&mut model, KeyCode::Char('x'));
    model
}

#[test]
fn x_opens_a_dialog_of_its_own_focused_on_cancel() {
    let mut model = asking_to_delete_prod();
    let frame = paint(&mut model, 100, 30);
    assert!(frame.contains("Delete connection"), "{frame}");
    assert!(
        frame.contains("Delete \"prod\"? This cannot be undone."),
        "{frame}"
    );
    assert!(frame.contains(">[Cancel]"), "{frame}");
    assert!(!frame.contains("keep secrets"), "{frame}");
}

/// Enter out of habit deletes nothing; the arrows move to Delete.
#[test]
fn enter_cancels_until_the_arrows_move_to_delete() {
    let mut model = asking_to_delete_prod();
    let effects = press(&mut model, KeyCode::Enter);
    assert_eq!(deletes(&effects), None);
    assert!(model.connections.delete_target.is_none());

    let mut model = asking_to_delete_prod();
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    assert_eq!(deletes(&effects), Some(("prod".into(), true)));
}

#[test]
fn esc_cancels_and_letters_do_nothing() {
    let mut model = asking_to_delete_prod();
    for letter in ['d', 'k', 'x', 'y'] {
        let effects = press(&mut model, KeyCode::Char(letter));
        assert!(effects.is_empty(), "{letter}: {effects:?}");
        assert!(model.connections.delete_target.is_some(), "{letter}");
    }
    press(&mut model, KeyCode::Esc);
    assert!(model.connections.delete_target.is_none());
    assert_eq!(
        model.screen,
        dexo_tui::model::Screen::Connections,
        "Esc closed the list as well"
    );
}

#[test]
fn the_buttons_take_a_click() {
    let mut model = asking_to_delete_prod();
    paint(&mut model, 100, 30);
    let effects = click(&mut model, HitTarget::Button(HitButton::ConfirmDelete));
    assert_eq!(deletes(&effects), Some(("prod".into(), true)));

    let mut model = asking_to_delete_prod();
    paint(&mut model, 100, 30);
    let effects = click(&mut model, HitTarget::Button(HitButton::Cancel));
    assert_eq!(deletes(&effects), None);
    assert!(model.connections.delete_target.is_none());
}

/// The hint read `d dup` while the click target looked for `d duplicate`.
#[test]
fn every_hint_under_the_list_takes_a_click() {
    let mut model = with_connections(&["local"]);
    paint(&mut model, 100, 30);
    let effects = click(&mut model, HitTarget::Button(HitButton::Duplicate));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::DuplicateProfile { .. })),
        "{effects:?}"
    );
    click(&mut model, HitTarget::Button(HitButton::Delete));
    assert!(model.connections.delete_target.is_some());
}

/// Past the rows the screen has, the list scrolls to the selection and the actions
/// stay in sight.
#[test]
fn the_list_scrolls_to_the_selection() {
    let names: Vec<String> = (0..40).map(|index| format!("db{index:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut model = with_connections(&refs);
    model.connections.selected_profile = 37;
    let frame = paint(&mut model, 100, 30);
    assert!(frame.contains("> db37"), "{frame}");
    assert!(frame.contains("x delete"), "{frame}");
}
