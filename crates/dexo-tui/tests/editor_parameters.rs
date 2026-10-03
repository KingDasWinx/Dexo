//! The parameter prompt: each field starts on its own value, never on what the last one
//! was answered with, and a run that reuses values says so.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::palette::{filter_entries, palette_entries};
use dexo_tui::{Action, Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch));
    }
}

fn edit_parameters(model: &mut Model) {
    update(model, Action::OpenPalette);
    update(model, Action::PaletteQuery("Edit Parameters".into()));
    let entries = palette_entries(model);
    let visible = filter_entries(&entries, model.palette.query.as_str());
    model.palette.selected = visible
        .iter()
        .position(|entry| entry.id == "editor.parameters")
        .expect("Edit Parameters is in the palette");
    press(model, KeyCode::Enter);
}

fn parameter_model() -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    model.set_sql("select * from customers where id = :id and region = :region");
    dexo_tui::screens::editor::refresh_intelligence(&mut model, false);
    model
}

#[test]
fn each_prompt_starts_empty_and_a_second_pass_starts_on_its_own_value() {
    let mut model = parameter_model();
    edit_parameters(&mut model);
    assert!(model.editor.parameter_prompt);
    let screen = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(screen.contains("Parameters (1 of 2)"), "{screen}");
    assert!(screen.contains("id = "), "{screen}");

    type_text(&mut model, "5");
    press(&mut model, KeyCode::Enter);
    // The second prompt is empty: it was `region = 5`, and typing made `5north`.
    assert_eq!(model.editor.parameter_draft.as_str(), "");
    type_text(&mut model, "north");
    press(&mut model, KeyCode::Enter);
    assert!(!model.editor.parameter_prompt);
    assert_eq!(model.editor.parameter_draft.as_str(), "");

    // Editing again walks all of them, each on its own value, selected.
    edit_parameters(&mut model);
    assert_eq!(model.editor.parameter_draft.as_str(), "5");
    assert!(model.editor.parameter_draft.is_selected());
    type_text(&mut model, "7");
    press(&mut model, KeyCode::Enter);
    assert_eq!(model.editor.parameter_draft.as_str(), "north");
    press(&mut model, KeyCode::Enter);
    assert!(!model.editor.parameter_prompt);
    let values: Vec<String> = model
        .editor
        .parameters
        .iter()
        .map(|parameter| format!("{:?}", parameter.value))
        .collect();
    assert_eq!(values, ["Text(\"7\")", "Text(\"north\")"]);
}

#[test]
fn a_run_with_values_from_before_says_which_and_how_to_change_them() {
    let mut model = parameter_model();
    edit_parameters(&mut model);
    type_text(&mut model, "5");
    press(&mut model, KeyCode::Enter);
    type_text(&mut model, "north");
    press(&mut model, KeyCode::Enter);
    // The run the prompt started is over before the next one: a second run while one is
    // going is refused.
    model.active_task = None;
    model.active_query = None;
    model.active_operation = None;
    update(&mut model, Action::ExecuteStatement);
    let shown = model
        .messages
        .toast
        .as_ref()
        .map(|toast| toast.message.clone())
        .unwrap_or_default();
    assert!(
        shown.contains("id = 5, region = north") && shown.contains("Edit Parameters"),
        "{shown}"
    );
}

/// `:id` is Dexo's parameter, not the server's: an explain says so before asking it.
#[test]
fn an_explain_of_a_named_parameter_says_what_to_do_without_asking_the_server() {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.active_session = Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1)));
    model.active_document_mut().sql =
        dexo_sql::SqlDocument::new("select * from orders where id = :id");
    let effects = update(&mut model, Action::OpenExplain);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::RunExplain { .. })),
        "{effects:?}"
    );
    let said = model.messages.toast.expect("a message").message;
    assert!(said.contains(":id") && said.contains("values"), "{said}");
}
