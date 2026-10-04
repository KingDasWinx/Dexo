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

#[test]
fn enter_on_advanced_options_opens_them_and_goes_in() {
    let mut model = filled_form();
    while !model.connection_form.on_advanced() {
        model.connection_form.focus_next();
    }

    enter(&mut model);

    assert!(model.connection_form.advanced);
    let lines = model.connection_form.lines().join("\n");
    assert!(lines.contains("> environment:"), "{lines}");

    // Back on the row, Enter folds them.
    model.connection_form.focus_prev();
    enter(&mut model);
    assert!(!model.connection_form.advanced);
}

/// A tunnel's port, user and key show once its host is typed; a proxy's kind once its
/// host is; TLS files once a mode is picked.
#[test]
fn what_depends_on_another_field_shows_once_that_is_set() {
    let mut model = filled_form();
    model.connection_form.set_advanced(true);
    let lines = |model: &Model| model.connection_form.lines().join("\n");
    assert!(lines(&model).contains("SSH host:"));
    assert!(!lines(&model).contains("SSH user:"), "{}", lines(&model));
    assert!(!lines(&model).contains("proxy kind:"));
    assert!(!lines(&model).contains("CA file:"));
    assert!(lines(&model).contains("── SSH tunnel"));

    model.connection_form.set_value("ssh_host", "bastion");
    model.connection_form.set_value("proxy_host", "proxy.local");
    model.connection_form.set_value("tls_mode", "required");

    for shown in [
        "SSH user:",
        "SSH key:",
        "proxy kind:",
        "proxy port:",
        "CA file:",
    ] {
        assert!(lines(&model).contains(shown), "{shown}: {}", lines(&model));
    }
}
