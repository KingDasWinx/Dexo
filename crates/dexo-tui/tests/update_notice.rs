use dexo_tui::action::Action;
use dexo_tui::render::render_to_string;
use dexo_tui::{Model, update};

fn status_line(model: &Model, width: u16) -> String {
    let frame = render_to_string(model, width, 30);
    frame.lines().last().unwrap_or_default().to_string()
}

fn announce(model: &mut Model) {
    update(
        model,
        Action::UpdateAvailable {
            version: "1.3.0".into(),
            command: "brew upgrade dexo".into(),
        },
    );
}

/// A toast is gone before anyone reads a command, so the notice stays on the status
/// bar, and says how this install updates.
#[test]
fn a_newer_release_stays_on_the_status_bar_with_its_command() {
    let mut model = Model::default();
    announce(&mut model);

    assert_eq!(
        model.messages.last().map(|entry| entry.message.as_str()),
        Some("Dexo 1.3.0 is available. Update with: brew upgrade dexo")
    );
    let line = status_line(&model, 160);
    assert!(line.contains("↑ 1.3.0: brew upgrade dexo"), "{line}");
    assert!(line.contains("Ctrl+P  F1"), "{line}");
}

#[test]
fn a_command_too_long_for_the_bar_leaves_the_version() {
    let mut model = Model::default();
    model.capabilities.unicode = false;
    update(
        &mut model,
        Action::UpdateAvailable {
            version: "1.3.0".into(),
            command: "cargo install --locked --git https://github.com/kingdaswinx/Dexo dexo".into(),
        },
    );

    let line = status_line(&model, 100);
    assert!(line.contains("update 1.3.0"), "{line}");
    assert!(!line.contains("cargo install"), "{line}");
    assert!(line.contains("Ctrl+P  F1"), "{line}");
}

#[test]
fn no_notice_without_a_newer_release() {
    let line = status_line(&Model::default(), 160);
    assert!(!line.contains('↑'), "{line}");
}

#[test]
fn settings_offer_to_turn_the_check_off() {
    let screen = dexo_tui::screens::settings::SettingsScreen::default();
    let updates = screen
        .options()
        .into_iter()
        .find(|field| field.label == "Updates")
        .expect("no Updates row");
    assert_eq!(updates.values[updates.active], "On");
}
