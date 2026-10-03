//! A screen's actions are buttons: a click and the key printed in it do the same, Left
//! and Right walk them from the detail and Enter presses one, and a dimmed one says why.
//! They were a run of text at the bottom of a pane.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Effect, Model, update};

fn paint(model: &mut Model) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn connections() -> Model {
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::nil()),
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
    paint(&mut model);
    model
}

fn click(model: &mut Model, target: HitTarget) -> Vec<Effect> {
    let (column, row) = model.hits.center(target);
    assert_ne!((column, row), (0, 0), "{target:?} is not on screen");
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

fn key(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, modifiers)));
    paint(model);
    effects
}

#[test]
fn a_click_on_edit_opens_the_form_as_e_does() {
    let mut model = connections();

    click(&mut model, HitTarget::Press(KeyCode::Char('e'), false));

    assert!(model.connection_form.open);
}

#[test]
fn a_click_on_connect_dials_it() {
    let mut model = connections();

    let effects = click(&mut model, HitTarget::Press(KeyCode::Enter, false));

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ConnectProfile { .. })),
        "{effects:?}"
    );
}

#[test]
fn left_right_and_enter_press_the_detail_buttons() {
    let mut model = connections();
    key(&mut model, KeyCode::Char('2'), KeyModifiers::ALT);
    let buttons = dexo_tui::screen::buttons(&model);
    let edit = buttons
        .iter()
        .position(|button| button.key == KeyCode::Char('e'))
        .unwrap();

    for _ in 0..edit {
        key(&mut model, KeyCode::Right, KeyModifiers::NONE);
    }
    assert_eq!(dexo_tui::screen::button_focus(&model), Some(edit));
    key(&mut model, KeyCode::Enter, KeyModifiers::NONE);

    assert!(model.connection_form.open);
}

#[test]
fn a_dimmed_button_says_why_by_click_and_by_key() {
    let mut model = connections();

    let effects = click(&mut model, HitTarget::Press(KeyCode::Char('c'), false));

    assert!(effects.is_empty(), "{effects:?}");
    let said = |model: &Model| {
        model
            .messages
            .iter()
            .any(|message| message.message.contains("pg-dev is not connected"))
    };
    assert!(said(&model));
    model.messages = Default::default();
    key(&mut model, KeyCode::Char('c'), KeyModifiers::NONE);
    assert!(said(&model));
}

#[test]
fn the_buttons_are_drawn_over_the_detail() {
    let model = connections();

    let frame = dexo_tui::render::render_to_string(&model, 140, 40);

    for button in [
        "[⏎ Connect]",
        "[e Edit]",
        "[d Duplicate]",
        "[t Test]",
        "[x Delete]",
    ] {
        assert!(frame.contains(button), "{button}: {frame}");
    }
    assert!(!frame.contains("Enter connect  n new"), "{frame}");
}
