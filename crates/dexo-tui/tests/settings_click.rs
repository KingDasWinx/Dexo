use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Model, update};

fn settings_model() -> Model {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    model.settings.open = true;
    model
}

fn draw(model: &mut Model) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
        model.width,
        model.height,
    ))
    .unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
}

fn click(model: &mut Model, target: HitTarget) {
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
    );
}

/// A click anywhere on the Accent row stepped to the next accent, so Rose could not be
/// picked, and going back took five more clicks.
#[test]
fn clicking_an_option_chooses_that_option() {
    let mut model = settings_model();
    draw(&mut model);
    let rose = dexo_tui::theme::ACCENTS.len() - 1;
    assert_eq!(dexo_tui::theme::ACCENTS[rose].2, "Rose");

    click(
        &mut model,
        HitTarget::SettingsChoice {
            row: 2,
            index: rose,
        },
    );

    assert_eq!(model.settings.accent, dexo_tui::theme::ACCENTS[rose].0);
    draw(&mut model);
    // And back to the first one, in one click.
    click(&mut model, HitTarget::SettingsChoice { row: 2, index: 0 });
    assert_eq!(model.settings.accent, dexo_tui::theme::ACCENTS[0].0);
}

#[test]
fn clicking_the_off_of_a_toggle_turns_it_off_and_the_on_turns_it_on() {
    let mut model = settings_model();
    draw(&mut model);
    assert!(model.animation);

    click(&mut model, HitTarget::SettingsChoice { row: 5, index: 1 });
    assert!(!model.animation);
    // Off again is no change, not a toggle back.
    draw(&mut model);
    click(&mut model, HitTarget::SettingsChoice { row: 5, index: 1 });
    assert!(!model.animation);
    click(&mut model, HitTarget::SettingsChoice { row: 5, index: 0 });
    assert!(model.animation);
}

#[test]
fn the_footer_names_the_theme_key() {
    let mut model = settings_model();
    draw(&mut model);
    let frame = dexo_tui::render::render_to_string(&model, 120, 36);
    assert!(frame.contains("e theme"), "{frame}");
}
