//! The pieces every screen is drawn with: buttons that say their key, a bar of them over
//! a detail, a toolbar, fields in sections. A screen's actions were hotkeys printed as a
//! run of text at the bottom of a pane, and nothing on screen looked pressable.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;

/// One action of a screen: the key that does it, printed inside it, and its label. A
/// button that cannot act now keeps its place, dimmed, and says why when pressed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button {
    pub key: KeyCode,
    /// The key is pressed with Shift: `C` beside `c`.
    pub shift: bool,
    pub label: String,
    pub enabled: Result<(), String>,
}

impl Button {
    pub fn new(key: KeyCode, label: impl Into<String>) -> Self {
        let shift = matches!(key, KeyCode::Char(ch) if ch.is_ascii_uppercase());
        Self {
            key,
            shift,
            label: label.into(),
            enabled: Ok(()),
        }
    }

    /// Dimmed, and `why` is what pressing it says.
    pub fn disabled(mut self, why: impl Into<String>) -> Self {
        self.enabled = Err(why.into());
        self
    }

    /// Dimmed unless `ok`.
    pub fn enabled_if(self, ok: bool, why: impl Into<String>) -> Self {
        if ok { self } else { self.disabled(why) }
    }

    /// Whether `key`, with `shift`, presses it.
    pub fn answers(&self, key: KeyCode, shift: bool) -> bool {
        self.key == key && (self.shift == shift || !matches!(key, KeyCode::Char(_)))
    }

    /// `[k Label]`, as drawn.
    pub fn text(&self) -> String {
        format!("[{} {}]", key_label(self.key), self.label)
    }
}

/// How a key is printed in a button.
pub fn key_label(key: KeyCode) -> String {
    match key {
        KeyCode::Enter => "⏎".into(),
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Delete => "Del".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Backspace => "Bksp".into(),
        other => format!("{other:?}"),
    }
}

/// The buttons in `area`, left to right, wrapping to the next row before one that does
/// not fit -- a key is never split from its label. `focused` is reversed and marked `>`,
/// so the focus reads without colour; a disabled one is dimmed. Each answers a click.
/// Returns the rows used.
pub fn action_bar(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    buttons: &[Button],
    focused: Option<usize>,
) -> u16 {
    if buttons.is_empty() || area.width == 0 || area.height == 0 {
        return 0;
    }
    let key_style = model
        .theme
        .style(Role::Focus, model.capabilities)
        .add_modifier(Modifier::BOLD);
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut row = 0u16;
    let mut x = 0u16;
    let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
    for (index, button) in buttons.iter().enumerate() {
        let marker = if focused == Some(index) { ">" } else { " " };
        let width = (marker.chars().count() + button.text().chars().count()) as u16;
        if x > 0 && x + width > area.width {
            row += 1;
            x = 0;
            if row >= area.height {
                break;
            }
            lines.push(Vec::new());
        }
        let line = lines.last_mut().expect("a row");
        let enabled = button.enabled.is_ok();
        let reverse = |style: Style| {
            if focused == Some(index) {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            }
        };
        line.push(Span::styled(marker.to_string(), reverse(muted)));
        line.push(Span::styled("[".to_string(), reverse(muted)));
        line.push(Span::styled(
            key_label(button.key),
            reverse(if enabled { key_style } else { muted }),
        ));
        line.push(Span::styled(
            format!(" {}", button.label),
            reverse(if enabled { Style::default() } else { muted }),
        ));
        line.push(Span::styled("]".to_string(), reverse(muted)));
        hits.register(
            HitTarget::Press(button.key, button.shift),
            Rect::new(
                area.x + x,
                area.y + row,
                width.min(area.width.saturating_sub(x)),
                1,
            ),
        );
        x += width + 1;
        line.push(Span::raw(" "));
    }
    let used = lines.len() as u16;
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()),
        Rect::new(area.x, area.y, area.width, used.min(area.height)),
    );
    used.min(area.height)
}

/// How many rows `buttons` take in `width` columns, as `action_bar` lays them.
pub fn action_bar_rows(buttons: &[Button], width: u16) -> u16 {
    if buttons.is_empty() || width == 0 {
        return 0;
    }
    let mut rows = 1u16;
    let mut x = 0u16;
    for button in buttons {
        let width_of = (1 + button.text().chars().count()) as u16;
        if x > 0 && x + width_of > width {
            rows += 1;
            x = 0;
        }
        x += width_of + 1;
    }
    rows
}
