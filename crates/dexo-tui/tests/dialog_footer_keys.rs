//! Every dialog with Submit and Cancel answers the same keys: Esc cancels, the arrows
//! walk between the input and the two buttons, Left and Right step between the buttons,
//! and Enter on Cancel cancels. Each dialog used to spell this out for itself, with Tab
//! and BackTab only, so the arrows did nothing on six of these eight.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::update::update;
use dexo_tui::widgets::form::FooterFocus;

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, M::NONE))
}

struct Dialog {
    name: &'static str,
    open: fn(&mut Model),
    is_open: fn(&Model) -> bool,
    footer: fn(&Model) -> FooterFocus,
}

fn workbench() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 40);
    model
}

fn open_parameters(model: &mut Model) {
    model.active_session = Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1)));
    model.session_generation = 1;
    model.active_document_mut().sql = dexo_sql::SqlDocument::new("select :N");
    model.editor.parameters = vec![dexo_tui::screens::editor::ParameterValue {
        name: "N".into(),
        value: dexo_driver_api::DbValue::Null,
        sensitive: false,
    }];
    model.editor.parameter_prompt = true;
}

fn dialogs() -> Vec<Dialog> {
    use dexo_tui::screens::projects::ProjectsMode;
    vec![
        Dialog {
            name: "transaction prompt",
            open: |m| m.transaction_prompt.open = true,
            is_open: |m| m.transaction_prompt.open,
            footer: |m| m.transaction_prompt.footer,
        },
        Dialog {
            name: "document name prompt",
            open: |m| m.document_name_prompt.open = true,
            is_open: |m| m.document_name_prompt.open,
            footer: |m| m.document_name_prompt.footer,
        },
        Dialog {
            name: "data query prompt",
            open: |m| m.data.query_prompt.open = true,
            is_open: |m| m.data.query_prompt.open,
            footer: |m| m.data.query_prompt.footer,
        },
        Dialog {
            name: "project name",
            open: |m| {
                m.projects.open = true;
                m.projects.mode = ProjectsMode::Create;
            },
            is_open: |m| m.projects.open && m.projects.mode == ProjectsMode::Create,
            footer: |m| m.projects.footer,
        },
        Dialog {
            name: "transfer",
            open: |m| m.transfer.open = true,
            is_open: |m| m.transfer.open,
            footer: |m| m.transfer.footer,
        },
        Dialog {
            name: "parameters",
            open: open_parameters,
            is_open: |m| m.editor.parameter_prompt,
            footer: |m| m.editor.parameter_footer,
        },
        Dialog {
            name: "connection form",
            open: |m| {
                update(m, Action::OpenConnectionForm);
            },
            is_open: |m| m.connection_form.open,
            footer: |m| m.connection_form.footer_focus(),
        },
        Dialog {
            name: "file picker",
            open: |m| m.file_picker.open = true,
            is_open: |m| m.file_picker.open,
            footer: |m| m.file_picker.footer_focus(),
        },
    ]
}

/// Walks Down until the footer lands on `target`, or gives up.
fn walk_to(model: &mut Model, dialog: &Dialog, target: FooterFocus) -> bool {
    for _ in 0..40 {
        if (dialog.footer)(model) == target {
            return true;
        }
        update(model, key(KeyCode::Down));
        if !(dialog.is_open)(model) {
            return false;
        }
    }
    false
}

#[test]
fn esc_cancels_every_dialog() {
    for dialog in dialogs() {
        let mut model = workbench();
        (dialog.open)(&mut model);
        update(&mut model, key(KeyCode::Esc));
        assert!(
            !(dialog.is_open)(&model),
            "Esc left the {} open",
            dialog.name
        );
    }
}

#[test]
fn the_arrows_reach_cancel_and_enter_cancels() {
    for dialog in dialogs() {
        let mut model = workbench();
        (dialog.open)(&mut model);
        assert!(
            walk_to(&mut model, &dialog, FooterFocus::Cancel),
            "the arrows never reached Cancel in the {}",
            dialog.name
        );
        update(&mut model, key(KeyCode::Enter));
        assert!(
            !(dialog.is_open)(&model),
            "Enter on Cancel left the {} open",
            dialog.name
        );
    }
}

#[test]
fn left_and_right_step_between_the_buttons() {
    for dialog in dialogs() {
        let mut model = workbench();
        (dialog.open)(&mut model);
        assert!(
            walk_to(&mut model, &dialog, FooterFocus::Submit),
            "the arrows never reached Submit in the {}",
            dialog.name
        );
        update(&mut model, key(KeyCode::Right));
        assert_eq!(
            (dialog.footer)(&model),
            FooterFocus::Cancel,
            "Right from Submit in the {}",
            dialog.name
        );
        update(&mut model, key(KeyCode::Left));
        assert_eq!(
            (dialog.footer)(&model),
            FooterFocus::Submit,
            "Left from Cancel in the {}",
            dialog.name
        );
    }
}

/// The reported bug, and the worse half of it. Esc closed the prompt the same way a
/// submit does, the caller could not tell them apart, and ran the statement -- which
/// found the parameter still null and opened the prompt again.
#[test]
fn cancelling_the_parameters_does_not_run_the_statement() {
    let mut model = workbench();
    open_parameters(&mut model);
    let effects = update(&mut model, key(KeyCode::Esc));
    assert!(
        !model.editor.parameter_prompt,
        "Esc did not close the prompt"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "Esc ran the statement: {effects:?}"
    );

    // And Enter on the Cancel button is the same cancel.
    let mut model = workbench();
    open_parameters(&mut model);
    update(&mut model, key(KeyCode::Down));
    update(&mut model, key(KeyCode::Down));
    assert_eq!(model.editor.parameter_footer, FooterFocus::Cancel);
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(!model.editor.parameter_prompt);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartScript(_))),
        "Enter on Cancel ran the statement: {effects:?}"
    );
}

/// Left and Right on the input belong to the text, not to the buttons.
#[test]
fn left_and_right_on_the_input_still_edit_the_text() {
    let mut model = workbench();
    model.document_name_prompt.open = true;
    for ch in "abc".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    update(&mut model, key(KeyCode::Left));
    assert_eq!(
        model.document_name_prompt.footer,
        FooterFocus::Input,
        "Left on the input moved focus to a button"
    );
    update(&mut model, key(KeyCode::Char('X')));
    assert_eq!(model.document_name_prompt.name.as_str(), "abXc");
}
