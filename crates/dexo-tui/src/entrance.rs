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

pub fn logo_frames(animated: bool) -> Vec<LogoFrame> {
    if animated
        && let Ok(frames) = animated_logo_frames_platform()
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

pub fn play_animation() -> Result<(), String> {
    play_animation_platform()
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
fn play_animation_platform() -> Result<(), String> {
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

    let colors = ["03a9f4", "00d1ff", "ffffff"]
        .into_iter()
        .map(Color::from_hex)
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
fn play_animation_platform() -> Result<(), String> {
    // ttfx currently targets Linux and macOS. The onboarding screen remains
    // available on other platforms; only its animated prelude is skipped.
    Ok(())
}

#[cfg(unix)]
fn animated_logo_frames_platform() -> Result<Vec<LogoFrame>, String> {
    use ttfx::effects::colorshift::{ColorShift, ColorShiftConfig};
    use ttfx::engine::ctx::{Clock, EngineCtx};
    use ttfx::engine::effect::Effect;
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::graphics::{Color, GradientDirection};
    use ttfx::utils::rng::Rng;

    let colors = ["03a9f4", "00d1ff", "ffffff", "03a9f4"]
        .into_iter()
        .map(Color::from_hex)
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
fn animated_logo_frames_platform() -> Result<Vec<LogoFrame>, String> {
    Ok(vec![static_logo_frame()])
}

#[cfg(test)]
mod tests {
    use super::{LOGO_ART, is_complete, logo_frames, mark_complete, static_logo_frame};

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

    #[cfg(unix)]
    #[test]
    fn ttfx_builds_a_loop_of_colored_logo_frames() {
        let frames = logo_frames(true);
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
