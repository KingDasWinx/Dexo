use dexo_tui::capabilities::{ColorDepth, TerminalCapabilities};
use dexo_tui::{Action, Model, update};
use ratatui::style::Color;

/// A 16-colour terminal was sent 24-bit colours: every colour the frame paints follows
/// the depth the terminal reported.
#[test]
fn a_16_colour_terminal_gets_no_rgb_cells() {
    for depth in [ColorDepth::Ansi16, ColorDepth::Ansi256] {
        let mut model = Model {
            capabilities: TerminalCapabilities {
                color_depth: depth,
                unicode: true,
                mouse: true,
            },
            ..Model::default()
        };
        update(
            &mut model,
            Action::Resize {
                width: 120,
                height: 36,
            },
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 36)).unwrap();
        let mut hits = dexo_tui::mouse::HitMap::default();
        terminal
            .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
            .unwrap();
        let rgb = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| matches!(cell.fg, Color::Rgb(..)) || matches!(cell.bg, Color::Rgb(..)))
            .count();
        assert_eq!(rgb, 0, "{depth:?}: {rgb} cells carry 24-bit colour");
    }
}

/// Unicode off still drew the pointer, the folder arrows, the ellipsis, the dot and the
/// em dash.
#[test]
fn unicode_off_draws_no_glyphs_beyond_box_drawing() {
    let mut model = Model {
        capabilities: TerminalCapabilities {
            color_depth: ColorDepth::TrueColor,
            unicode: false,
            mouse: true,
        },
        ..Model::default()
    };
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    let frame = dexo_tui::render::render_to_string(&model, 120, 36);
    for ch in frame.chars() {
        let boxed = ('\u{2500}'..='\u{257f}').contains(&ch);
        assert!(ch.is_ascii() || boxed, "{ch:?} on screen:\n{frame}");
    }
}
