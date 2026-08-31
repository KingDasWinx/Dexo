use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::entrance::LogoFrame;
use dexo_tui::{Action, Effect, Model, update};

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn onboarding_explains_the_first_steps() {
    let mut model = Model::default();
    model.onboarding.open = true;

    let screen = dexo_tui::render::render_to_string(&model, 80, 24);

    assert!(screen.contains("Welcome"));
    assert!(!screen.contains("Bem-vindo"));
    assert!(screen.contains("██████████"));
    assert!(screen.contains("░░░░░░░░░░    ░░░░░░"));
    assert!(screen.contains("Ctrl+P"));
    assert!(screen.contains("Ctrl+Enter"));
    assert!(screen.contains("F1"));
}

#[test]
fn enter_completes_onboarding() {
    let mut model = Model::default();
    model.onboarding.open = true;

    let effects = update(&mut model, key(KeyCode::Enter));

    assert!(!model.onboarding.open);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CompleteOnboarding))
    );
}

#[test]
fn f1_completes_onboarding_and_opens_full_help() {
    let mut model = Model::default();
    model.onboarding.open = true;

    let effects = update(&mut model, key(KeyCode::F(1)));

    assert!(!model.onboarding.open);
    assert!(model.help.open);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CompleteOnboarding))
    );
}

#[test]
fn animation_tick_advances_and_wraps_logo_frames() {
    let mut model = Model::default();
    model.onboarding.open = true;
    model.onboarding.logo_frames = Arc::new(vec![LogoFrame::default(), LogoFrame::default()]);

    update(&mut model, Action::OnboardingTick);
    assert_eq!(model.onboarding.logo_frame, 1);
    update(&mut model, Action::OnboardingTick);
    assert_eq!(model.onboarding.logo_frame, 0);
}

#[test]
fn onboarding_fits_a_compact_terminal() {
    let mut model = Model::default();
    model.onboarding.open = true;

    let screen = dexo_tui::render::render_to_string(&model, 40, 12);

    assert!(screen.contains("DEXO"));
    assert!(screen.contains("Get started"));
}
