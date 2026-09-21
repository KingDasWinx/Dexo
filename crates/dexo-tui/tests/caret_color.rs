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
