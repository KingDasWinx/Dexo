use dexo_tui::Model;

#[test]
fn the_session_recovery_dialog_speaks_in_words_and_has_buttons() {
    let model = Model {
        recovery: dexo_tui::screens::recovery::RecoveryScreen::fixture(),
        ..Model::default()
    };
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    for wanted in ["closed unexpectedly", "scratch.sql", "[Keep]", "[Discard]"] {
        assert!(frame.contains(wanted), "{wanted}:\n{frame}");
    }
    for raw in ["recovery open=", "confirm_discard=", "transaction="] {
        assert!(!frame.contains(raw), "{raw}:\n{frame}");
    }
}
