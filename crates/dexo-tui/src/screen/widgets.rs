//! The pieces every screen is drawn with: buttons that say their key, a bar of them over
//! a detail, a toolbar, fields in sections. A screen's actions were hotkeys printed as a
//! run of text at the bottom of a pane, and nothing on screen looked pressable.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;
use crate::widgets::text_input::TextInput;

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

/// How wide `buttons` are on one row, as `action_bar` lays them: a marker column before
/// each and a space between.
fn bar_width(buttons: &[Button]) -> u16 {
    buttons
        .iter()
        .map(|button| button.text().chars().count() as u16 + 2)
        .sum::<u16>()
        .saturating_sub(1)
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

/// A list's search box. `/` starts typing into it; Enter keeps what was typed and Esc
/// drops it. Lower case matches either case; a capital makes the case count.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Search {
    pub input: TextInput,
    pub typing: bool,
}

impl Search {
    /// A key while typing. False for the keys that still walk the list: Up and Down.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => return false,
            KeyCode::Enter => self.typing = false,
            KeyCode::Esc => {
                self.input.clear();
                self.typing = false;
            }
            _ => {
                self.input.handle_key(key);
            }
        }
        true
    }

    /// Whether one of `fields` holds what was typed.
    pub fn matches<'a>(&self, fields: impl IntoIterator<Item = &'a str>) -> bool {
        let query = self.input.trim();
        if query.is_empty() {
            return true;
        }
        let exact = query.chars().any(char::is_uppercase);
        fields.into_iter().any(|field| {
            if exact {
                field.contains(query)
            } else {
                field.to_lowercase().contains(query)
            }
        })
    }
}

/// A filter on the toolbar: its key and what it is set to. One off its default is in the
/// accent colour and underlined, so it reads without colour too.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chip {
    pub key: KeyCode,
    pub label: String,
    pub active: bool,
}

impl Chip {
    fn text(&self) -> String {
        format!("[{} {}]", key_label(self.key), self.label)
    }
}

/// The row over a screen's panes: its search box, its filters, and on the right the
/// buttons that act on the whole screen. Narrow, the buttons go first -- their keys stay
/// on the status line -- then the filters become a count of those on. Returns the rest of
/// `area`.
pub fn toolbar(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    search: Option<&Search>,
    chips: &[Chip],
    buttons: &[Button],
) -> Rect {
    if area.height < 2 || area.width == 0 {
        return area;
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    let rest = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    let width = row.width;
    let key_style = model
        .theme
        .style(Role::Focus, model.capabilities)
        .add_modifier(Modifier::BOLD);
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut spans: Vec<Span> = Vec::new();
    let mut x = 0u16;
    if let Some(search) = search {
        let box_width = (width / 3).clamp(14, 30).min(width);
        let room = usize::from(box_width.saturating_sub(2));
        spans.push(Span::styled("/ ", key_style));
        if search.input.is_empty() && !search.typing {
            spans.push(Span::styled(format!("{:<room$}", "search"), muted));
        } else {
            let (shown, at) = search.input.window(room.max(1));
            spans.push(Span::styled(
                shown,
                Style::default().add_modifier(Modifier::UNDERLINED),
            ));
            if search.typing {
                frame.set_cursor_position(ratatui::layout::Position::new(
                    row.x + 2 + at as u16,
                    row.y,
                ));
            }
        }
        hits.register(
            HitTarget::Press(KeyCode::Char('/'), false),
            Rect::new(row.x, row.y, box_width, 1),
        );
        spans.push(Span::raw("  "));
        x = box_width + 2;
    }
    let chips_width: u16 = chips
        .iter()
        .map(|chip| chip.text().chars().count() as u16 + 1)
        .sum();
    let buttons_width = bar_width(buttons);
    if x + chips_width <= width {
        for chip in chips {
            let label = if chip.active {
                key_style
                    .remove_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default()
            };
            spans.extend([
                Span::styled("[", muted),
                Span::styled(key_label(chip.key), key_style),
                Span::styled(format!(" {}", chip.label), label),
                Span::styled("] ", muted),
            ]);
            let chip_width = chip.text().chars().count() as u16;
            hits.register(
                HitTarget::Press(chip.key, false),
                Rect::new(row.x + x, row.y, chip_width, 1),
            );
            x += chip_width + 1;
        }
    } else if !chips.is_empty() {
        let on = chips.iter().filter(|chip| chip.active).count();
        let text = format!("Filters ({on})");
        x += text.chars().count() as u16 + 1;
        spans.push(Span::styled(text, if on > 0 { key_style } else { muted }));
    }
    if x + buttons_width <= width && !buttons.is_empty() {
        let start = width - buttons_width;
        let bar = Rect::new(row.x + start, row.y, buttons_width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
        action_bar(frame, bar, model, hits, buttons, None);
    } else {
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
    }
    rest
}

/// What an empty list or screen says, centred, with the buttons that change it under it.
pub fn empty_with_buttons(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    lines: &[String],
    buttons: &[Button],
) {
    let width = usize::from(area.width.saturating_sub(4)).max(8);
    let wrapped: Vec<String> = lines
        .iter()
        .flat_map(|line| crate::model::wrap_words(line, width))
        .collect();
    let bar_rows = action_bar_rows(buttons, area.width);
    let height = wrapped.len() as u16 + if bar_rows > 0 { bar_rows + 1 } else { 0 };
    let top = area.y + area.height.saturating_sub(height) / 3;
    let muted = model.theme.style(Role::Muted, model.capabilities);
    frame.render_widget(
        Paragraph::new(wrapped.join("\n"))
            .alignment(ratatui::layout::Alignment::Center)
            .style(muted),
        Rect::new(area.x, top, area.width, area.bottom().saturating_sub(top)),
    );
    let bar_top = top + wrapped.len() as u16 + 1;
    if bar_rows == 0 || bar_top >= area.bottom() {
        return;
    }
    let left = area.x + (area.width - bar_width(buttons).min(area.width)) / 2;
    action_bar(
        frame,
        Rect::new(left, bar_top, area.right() - left, area.bottom() - bar_top),
        model,
        hits,
        buttons,
        None,
    );
}

/// One row of a detail: a labelled value, a section's heading, a line of text, or a gap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldRow {
    Field(&'static str, String),
    Section(&'static str),
    Text(String),
    Blank,
}

/// `rows` as lines `width` columns wide: the labels dimmed in one column, a long value
/// wrapped under itself, a section's heading styled as the connection form's.
pub fn field_lines(model: &Model, rows: &[FieldRow], width: u16) -> Vec<Line<'static>> {
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let heading = muted.add_modifier(Modifier::BOLD);
    let label_width = rows
        .iter()
        .filter_map(|row| match row {
            FieldRow::Field(label, _) => Some(label.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let indent = 1 + label_width + 2;
    let room = usize::from(width).saturating_sub(indent).max(8);
    let mut lines = Vec::new();
    for row in rows {
        match row {
            FieldRow::Field(label, value) => {
                for (index, part) in crate::model::wrap_words(value, room)
                    .into_iter()
                    .enumerate()
                {
                    let head = if index == 0 {
                        format!(" {label:<label_width$}  ")
                    } else {
                        " ".repeat(indent)
                    };
                    lines.push(Line::from(vec![Span::styled(head, muted), Span::raw(part)]));
                }
            }
            FieldRow::Section(name) => lines.push(Line::styled(format!(" ── {name}"), heading)),
            FieldRow::Text(text) => lines.extend(
                crate::model::wrap_words(text, usize::from(width).saturating_sub(1))
                    .into_iter()
                    .map(|part| Line::raw(format!(" {part}"))),
            ),
            FieldRow::Blank => lines.push(Line::default()),
        }
    }
    lines
}
