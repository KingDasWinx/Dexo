use std::path::{Path, PathBuf};

const COMPLETION_MARKER: &str = "onboarding-v1.complete";

pub const LOGO_ART: &str = r#" ██████████
░░███░░░░███
 ░███   ░░███  ██████  █████ █████  ██████
 ░███    ░███ ███░░███░░███ ░░███  ███░░███
 ░███    ░███░███████  ░░░█████░  ░███ ░███
 ░███    ███ ░███░░░    ███░░░███ ░███ ░███
 ██████████  ░░██████  █████ █████░░██████
░░░░░░░░░░    ░░░░░░  ░░░░░ ░░░░░  ░░░░░░"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogoCell {
    pub symbol: String,
    pub foreground: Option<(u8, u8, u8)>,
}

impl Default for LogoCell {
    fn default() -> Self {
        Self {
            symbol: " ".into(),
            foreground: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogoFrame {
    pub rows: Vec<Vec<LogoCell>>,
}

pub fn logo_frames(animated: bool, theme: &crate::theme::Theme) -> Vec<LogoFrame> {
    if animated
        && let Ok(frames) = animated_logo_frames_platform(&gradient(theme, true))
        && !frames.is_empty()
    {
        return frames;
    }
    vec![static_logo_frame()]
}

pub fn static_logo_frame() -> LogoFrame {
    let width = LOGO_ART
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    LogoFrame {
        rows: LOGO_ART
            .lines()
            .map(|line| {
                line.chars()
                    .map(|symbol| LogoCell {
                        symbol: symbol.to_string(),
                        foreground: None,
                    })
                    .chain(std::iter::repeat_with(LogoCell::default))
                    .take(width)
                    .collect()
            })
            .collect(),
    }
}

pub fn is_complete(data_dir: &Path) -> bool {
    marker_path(data_dir).is_file()
}

pub fn mark_complete(data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let marker = marker_path(data_dir);
    if marker.is_file() {
        return Ok(());
    }
    let temporary = marker.with_extension("tmp");
    std::fs::write(&temporary, b"completed\n")?;
    std::fs::rename(temporary, marker)
}

pub fn should_animate(data_dir: &Path) -> bool {
    std::env::var_os("DEXO_NO_ANIMATION").is_none()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
        && dexo_app::settings::load_settings(data_dir).animation
}

/// The entrance, in the colours of the theme the user saved. It used to be a fixed
/// blue-to-white, whose white end vanished against a light theme's ground.
pub fn play_animation(theme: &crate::theme::Theme) -> Result<(), String> {
    play_animation_platform(&gradient(theme, false))
}

/// Accent to text colour, as hex: the accent leads because it is the theme's own
/// colour, and the text colour closes because it is the one guaranteed to read on the
/// theme's ground. `looped` returns to the accent so a cycling gradient has no seam.
fn gradient(theme: &crate::theme::Theme, looped: bool) -> Vec<String> {
    use crate::theme::Role;
    let hex = |role| {
        theme
            .rgb(role)
            .map(|(r, g, b)| format!("{r:02x}{g:02x}{b:02x}"))
    };
    let (Some(accent), Some(text)) = (hex(Role::Focus), hex(Role::Foreground)) else {
        return vec!["03a9f4".into(), "ffffff".into()];
    };
    if looped {
        vec![accent.clone(), text, accent]
    } else {
        vec![accent, text]
    }
}

pub fn clear_animation() -> std::io::Result<()> {
    use std::io::Write;

    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;
    std::io::stdout().flush()
}

fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(COMPLETION_MARKER)
}

#[cfg(unix)]
fn play_animation_platform(stops: &[String]) -> Result<(), String> {
    use std::io::IsTerminal;

    use ttfx::effects::wipe::{Wipe, WipeConfig};
    use ttfx::engine::canvas::Anchor;
    use ttfx::engine::ctx::{Clock, EngineCtx};
    use ttfx::engine::effect::run_effect;
    use ttfx::engine::terminal::{CharacterGroup, TerminalConfig};
    use ttfx::utils::easing::Easing;
    use ttfx::utils::graphics::{Color, GradientDirection};
    use ttfx::utils::rng::Rng;

    if !std::io::stdout().is_terminal() {
        return Ok(());
    }

    let colors = stops
        .iter()
        .map(|stop| Color::from_hex(stop))
        .collect::<Result<Vec<_>, _>>()?;
    let effect = WipeConfig {
        wipe_direction: CharacterGroup::DiagonalTopLeftToBottomRight,
        wipe_delay: 0,
        wipe_ease: Easing::InOutCirc,
        final_gradient_stops: colors,
        final_gradient_steps: vec![8],
        final_gradient_frames: 1,
        final_gradient_direction: GradientDirection::Horizontal,
    };
    let terminal = TerminalConfig {
        no_color: std::env::var_os("NO_COLOR").is_some(),
        frame_rate: 60,
        canvas_width: 0,
        canvas_height: 0,
        anchor_canvas: Anchor::C,
        anchor_text: Anchor::C,
        ..TerminalConfig::default()
    };
    let entrance = format!("{LOGO_ART}\n\nLocal database workbench");
    let mut context = EngineCtx::new(&entrance, terminal, Rng::seeded(0xD3E0), Clock::real())
        .map_err(|error| error.to_string())?;
    let mut effect = Wipe::new(effect);
    run_effect(&mut effect, &mut context, true)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn play_animation_platform(_stops: &[String]) -> Result<(), String> {
    // ttfx currently targets Linux and macOS. The onboarding screen remains
    // available on other platforms; only its animated prelude is skipped.
    Ok(())
}

#[cfg(unix)]
fn animated_logo_frames_platform(stops: &[String]) -> Result<Vec<LogoFrame>, String> {
    use ttfx::effects::colorshift::{ColorShift, ColorShiftConfig};
    use ttfx::engine::ctx::{Clock, EngineCtx};
    use ttfx::engine::effect::Effect;
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::graphics::{Color, GradientDirection};
    use ttfx::utils::rng::Rng;

    let colors = stops
        .iter()
        .map(|stop| Color::from_hex(stop))
        .collect::<Result<Vec<_>, _>>()?;
    let config = ColorShiftConfig {
        gradient_stops: colors.clone(),
        gradient_steps: vec![8],
        gradient_frames: 1,
        no_travel: false,
        travel_direction: GradientDirection::Horizontal,
        reverse_travel_direction: false,
        no_loop: false,
        cycles: 0,
        skip_final_gradient: true,
        final_gradient_stops: colors,
        final_gradient_steps: vec![8],
        final_gradient_direction: GradientDirection::Horizontal,
    };
    let terminal = TerminalConfig {
        frame_rate: 0,
        ignore_terminal_dimensions: true,
        ..TerminalConfig::default()
    };
    let mut context = EngineCtx::new(
        LOGO_ART,
        terminal,
        Rng::seeded(0xD3E0),
        Clock::virtual_with_frame_rate(15),
    )
    .map_err(|error| error.to_string())?;
    let mut effect = ColorShift::new(config);
    effect
        .build(&mut context)
        .map_err(|error| error.to_string())?;

    let mut frames = Vec::new();
    for _ in 0..256 {
        let Some(_rendered) = effect.next_frame(&mut context) else {
            break;
        };
        let frame = capture_logo_frame(&context);
        if frames.len() > 8 && frames.first() == Some(&frame) {
            break;
        }
        frames.push(frame);
    }
    Ok(frames)
}

#[cfg(unix)]
fn capture_logo_frame(context: &ttfx::engine::ctx::EngineCtx) -> LogoFrame {
    let width = context.terminal.canvas.width.max(0) as usize;
    let height = context.terminal.canvas.height.max(0) as usize;
    let mut rows = vec![vec![LogoCell::default(); width]; height];
    for id in &context.terminal.input_characters {
        let character = &context.terminal.arena[id.0 as usize];
        if !character.is_visible {
            continue;
        }
        let coordinate = character.input_coord;
        if coordinate.column < 1 || coordinate.row < 1 {
            continue;
        }
        let row = height.saturating_sub(coordinate.row as usize);
        let column = coordinate.column as usize - 1;
        let Some(cell) = rows.get_mut(row).and_then(|row| row.get_mut(column)) else {
            continue;
        };
        let visual = &character.animation.current_character_visual;
        cell.symbol = visual.symbol.clone();
        cell.foreground = visual
            .colors
            .as_ref()
            .and_then(|colors| colors.fg_color.as_ref())
            .map(|color| color.rgb_ints());
    }
    LogoFrame { rows }
}

#[cfg(not(unix))]
fn animated_logo_frames_platform(_stops: &[String]) -> Result<Vec<LogoFrame>, String> {
    Ok(vec![static_logo_frame()])
}

#[cfg(test)]
mod tests {
    use super::{LOGO_ART, is_complete, mark_complete, static_logo_frame};

    #[test]
    fn static_logo_keeps_its_shape() {
        let frame = static_logo_frame();
        assert_eq!(frame.rows.len(), LOGO_ART.lines().count());
        assert!(
            frame
                .rows
                .iter()
                .all(|row| row.len() == frame.rows[0].len())
        );
        assert!(frame.rows[0].iter().any(|cell| cell.symbol == "█"));
    }

    fn luminance(hex: &str) -> f32 {
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap() as f32;
        0.2126 * byte(0) + 0.7152 * byte(2) + 0.0722 * byte(4)
    }

    /// The entrance was a fixed blue-to-white. Against the light theme's ground its
    /// white end had a luminance contrast of 5 out of 255 -- the logo finished by
    /// vanishing. Every theme's gradient now closes on its own text colour.
    #[test]
    fn the_entrance_gradient_ends_legible_on_its_own_theme() {
        use crate::theme::{MODES, Role, theme_for};
        for mode in MODES {
            let theme = theme_for(*mode, "blue");
            let stops = super::gradient(&theme, false);
            let (r, g, b) = theme.rgb(Role::Background).unwrap();
            let ground = luminance(&format!("{r:02x}{g:02x}{b:02x}"));
            let end = luminance(stops.last().unwrap());
            assert!(
                (end - ground).abs() > 128.0,
                "{mode:?} ends on {} against its ground",
                stops.last().unwrap()
            );
        }
    }

    #[test]
    fn the_entrance_takes_the_saved_accent() {
        use crate::theme::{Mode, Role, theme_for};
        for accent in ["blue", "green", "orange"] {
            let theme = theme_for(Mode::Dark, accent);
            let (r, g, b) = theme.rgb(Role::Focus).unwrap();
            assert_eq!(
                super::gradient(&theme, false)[0],
                format!("{r:02x}{g:02x}{b:02x}"),
                "the gradient does not open on the {accent} accent"
            );
        }
    }

    /// A cycling gradient returns to where it started, or the loop shows a seam.
    #[test]
    fn the_logo_loop_closes_on_its_first_colour() {
        let theme = crate::theme::theme_for(crate::theme::Mode::Light, "blue");
        let stops = super::gradient(&theme, true);
        assert_eq!(stops.first(), stops.last());
    }

    #[cfg(unix)]
    #[test]
    fn ttfx_builds_a_loop_of_colored_logo_frames() {
        let frames = super::logo_frames(
            true,
            &crate::theme::theme_for(crate::theme::Mode::Dark, "blue"),
        );
        assert!(frames.len() > 8);
        assert!(frames.iter().any(|frame| {
            frame
                .rows
                .iter()
                .flatten()
                .any(|cell| cell.foreground.is_some())
        }));
        assert!(frames.iter().skip(1).any(|frame| frame != &frames[0]));
    }

    #[test]
    fn completion_marker_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!is_complete(directory.path()));
        mark_complete(directory.path()).unwrap();
        mark_complete(directory.path()).unwrap();
        assert!(is_complete(directory.path()));
    }
}
