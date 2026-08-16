use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::screens::connections::ConnectionIntent;
use dexo_tui::screens::projects::{ProjectIntent, ProjectsMode};
use dexo_tui::{Action, Effect, Focus, Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn choose_effects(model: &mut Model, query: &str) -> Vec<Effect> {
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(press(model, KeyCode::Char(ch)));
    }
    effects.extend(press(model, KeyCode::Enter));
    effects
}

fn choose(model: &mut Model, query: &str) {
    let _ = choose_effects(model, query);
}

#[test]
fn escape_restores_palette_origin_focus() {
    let mut model = Model {
        focus: Focus::Results,
        ..Model::default()
    };
    update(&mut model, Action::OpenPalette);
    assert_eq!(model.focus, Focus::Palette);
    press(&mut model, KeyCode::Esc);
    assert_eq!(model.focus, Focus::Results);
}

#[test]
fn project_create_opens_the_existing_name_form() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    assert!(!model.palette.open);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert!(model.projects.name_input.is_empty());
}

#[test]
fn palette_renders_registered_shortcut() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("Ctrl+P"));
}

#[test]
fn project_create_preserves_invalid_input() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    press(&mut model, KeyCode::Enter);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert_eq!(
        model.projects.error.as_deref(),
        Some("project name is required")
    );
}

#[test]
fn project_rename_loads_a_visible_chooser_before_input() {
    let mut model = Model::default();
    let effects = choose_effects(&mut model, "project.rename");
    assert!(model.projects.open);
    assert_eq!(model.projects.intent, Some(ProjectIntent::Rename));
    assert!(matches!(effects.as_slice(), [Effect::ListProjects]));
}

#[test]
fn connection_delete_opens_browser_and_never_hides_confirmation() {
    let mut model = Model::default();
    choose(&mut model, "connection.delete");
    assert!(model.connections.open);
    assert_eq!(model.connections.intent, Some(ConnectionIntent::Delete));
    assert!(model.connections.delete_target.is_none());
}
