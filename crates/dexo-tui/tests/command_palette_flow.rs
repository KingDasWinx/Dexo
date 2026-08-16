use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::screens::projects::ProjectsMode;
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
