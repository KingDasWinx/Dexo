//! Dexo repaints the whole surface, so the cursor colour the terminal was configured
//! with is about a background that is no longer there. A light terminal theme pins a
//! near-black caret; on Dexo's dark background that caret is invisible, which is how the
//! SQL editor came to look like it had none.
use dexo_tui::capabilities::{ColorDepth, TerminalCapabilities};
use dexo_tui::terminal::{RecordingTerminal, TerminalGuard};
use dexo_tui::theme::{Mode, theme_for};

fn caps(depth: ColorDepth) -> TerminalCapabilities {
    TerminalCapabilities {
        color_depth: depth,
        unicode: true,
        mouse: true,
    }
}

#[test]
fn every_theme_offers_the_terminal_a_caret_colour() {
    for mode in dexo_tui::theme::MODES {
        let theme = theme_for(*mode, "blue");
        assert!(
            theme.caret_rgb(caps(ColorDepth::TrueColor)).is_some(),
            "{mode:?} leaves the caret to the terminal's own guess"
        );
    }
}

/// The dark theme's caret has to be light, or setting it changes nothing: that is the
/// whole failure, a dark caret on a dark ground.
#[test]
fn the_dark_theme_asks_for_a_light_caret() {
    let (r, g, b) = theme_for(Mode::Dark, "blue")
        .caret_rgb(caps(ColorDepth::TrueColor))
        .expect("dark theme has a caret colour");
    let luminance = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    assert!(
        luminance > 128.0,
        "the dark theme asks for a dark caret ({r},{g},{b})"
    );
}

/// Without colour the terminal is left exactly as it was found.
#[test]
fn a_colourless_terminal_keeps_its_own_caret() {
    assert_eq!(
        theme_for(Mode::Dark, "blue").caret_rgb(caps(ColorDepth::None)),
        None
    );
}

#[test]
fn the_caret_colour_is_set_once_and_handed_back_on_exit() {
    let backend = RecordingTerminal::default();
    let calls = backend.clone();
    {
        let mut guard = TerminalGuard::enter(backend).unwrap();
        guard.set_cursor_color(Some((1, 2, 3))).unwrap();
        guard.set_cursor_color(Some((1, 2, 3))).unwrap();
        guard.restore();
    }
    let calls = calls.calls();
    assert_eq!(
        calls.iter().filter(|c| **c == "cursor_color_set").count(),
        1,
        "the unchanged colour was sent again: {calls:?}"
    );
    assert!(
        calls.contains(&"cursor_color_reset"),
        "the terminal kept Dexo's caret colour after exit: {calls:?}"
    );
}

/// The entrance resets to the terminal's default background after every cell, so a
/// painted ground does not survive it; only the default does. It is handed over once,
/// like the caret, and given back when Dexo leaves.
#[test]
fn the_background_is_set_once_and_handed_back_on_exit() {
    let backend = RecordingTerminal::default();
    let calls = backend.clone();
    {
        let mut guard = TerminalGuard::enter(backend).unwrap();
        guard.set_background_color(Some((18, 18, 18))).unwrap();
        guard.set_background_color(Some((18, 18, 18))).unwrap();
        guard.restore();
    }
    let calls = calls.calls();
    assert_eq!(
        calls.iter().filter(|c| **c == "background_set").count(),
        1,
        "the unchanged background was sent again: {calls:?}"
    );
    assert!(
        calls.contains(&"background_reset"),
        "the terminal kept Dexo's background after exit: {calls:?}"
    );
}

/// The entrance runs before the workbench exists, so it reads the theme from the saved
/// settings -- through the same mapping the workbench applies.
#[test]
fn the_saved_theme_is_what_the_settings_file_says() {
    use dexo_tui::theme::{Mode, Role, saved_theme, theme_for};

    let dir = tempfile::tempdir().unwrap();
    let mut settings = dexo_app::settings::load_settings(dir.path());
    settings.mode = dexo_app::settings::ModeId::Light;
    settings.accent = "green".into();
    dexo_app::settings::save_settings(dir.path(), &settings).unwrap();

    let saved = saved_theme(dir.path());
    let expected = theme_for(Mode::Light, "green");
    assert_eq!(saved.mode, Mode::Light);
    assert_eq!(saved.rgb(Role::Background), expected.rgb(Role::Background));
    assert_eq!(saved.rgb(Role::Focus), expected.rgb(Role::Focus));
}
