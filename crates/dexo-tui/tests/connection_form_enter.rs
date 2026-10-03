//! Enter in the connection form goes to the next field; on a button it does what the
//! button says. It saved from any field, half filled in.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::update::update;

fn enter(model: &mut Model) -> Vec<Effect> {
    update(
        model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    )
}

fn filled_form() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 40);
    update(&mut model, Action::OpenConnectionForm);
    for (label, value) in [
        ("name", "local-pg"),
        ("driver", "postgres"),
        ("host", "127.0.0.1"),
        ("database", "dexo"),
        ("username", "dexo"),
        ("password", "pw"),
    ] {
        if let Some(field) = model
            .connection_form
            .fields
            .iter_mut()
            .find(|field| field.label == label)
        {
            field.value = value.into();
        }
    }
    model
}

fn saves(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::CreateConnection { .. }))
}

#[test]
fn enter_on_a_field_goes_to_the_next_one() {
    let mut model = filled_form();
    let first = model.connection_form.focus;

    let effects = enter(&mut model);

    assert!(!saves(&effects), "{effects:?}");
    assert!(model.connection_form.open);
    assert_ne!(model.connection_form.focus, first);
    assert!(!model.connection_form.on_submit());
}

#[test]
fn enter_on_a_choice_goes_on_too() {
    let mut model = filled_form();
    while !model.connection_form.on_choice() {
        model.connection_form.focus_next();
    }

    let effects = enter(&mut model);

    assert!(!saves(&effects), "{effects:?}");
    assert!(!model.connection_form.on_choice());
}

#[test]
fn enter_walks_the_fields_to_save_and_saves_there() {
    let mut model = filled_form();
    for _ in 0..40 {
        if model.connection_form.on_submit() {
            break;
        }
        assert!(!saves(&enter(&mut model)));
    }
    assert!(model.connection_form.on_submit());

    assert!(saves(&enter(&mut model)));
}

#[test]
fn enter_on_cancel_closes_without_saving() {
    let mut model = filled_form();
    while !model.connection_form.on_cancel() {
        model.connection_form.focus_next();
    }

    let effects = enter(&mut model);

    assert!(!saves(&effects));
    assert!(!model.connection_form.open);
}
