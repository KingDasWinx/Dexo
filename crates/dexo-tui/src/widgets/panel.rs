//! The panel every form is drawn in, and its inputs: `docs/design.md`. A form is built as
//! [`Lines`], one row after another, each row saying what a click on it does; [`draw`]
//! frames them, keeps the focused row in view, and puts the buttons and the keys under
//! them.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;
use crate::widgets::text_input::TextInput;

/// A label's width.
pub const LABEL: usize = 16;
/// Where a value starts: ` › `, then the label.
pub const VALUE: usize = 3 + LABEL;
/// A dropdown, and the box a confirmation is typed in, are at most this wide.
pub const NARROW: usize = 44;

pub fn fg(model: &Model, role: Role) -> Style {
    model.theme.style(role, model.capabilities)
}

pub fn bold(model: &Model, role: Role) -> Style {
    fg(model, role).add_modifier(Modifier::BOLD)
}

/// `front` on `back`; reversed where the terminal has no colour.
pub fn on(model: &Model, front: Role, back: Role) -> Style {
    match model.theme.color(back, model.capabilities) {
        Some(color) => fg(model, front).bg(color),
        None => Style::default().add_modifier(Modifier::REVERSED),
    }
}

/// A box's border: red when its value was refused, the accent when it has the focus.
pub fn border(focused: bool, invalid: bool) -> Role {
    if invalid {
        Role::Error
    } else if focused {
        Role::Focus
    } else {
        Role::Border
    }
}

/// How wide a value can be in a form drawn in `area`.
pub fn value_width(area: Rect) -> usize {
    usize::from(area.width.saturating_sub(2)).saturating_sub(VALUE + 1)
}

/// The list a dropdown opened: the line it hangs from, its rows, the highlighted one,
/// and what a click on a row is.
pub struct Dropdown {
    line: usize,
    rows: Vec<Vec<Span<'static>>>,
    highlight: usize,
    field: usize,
}

/// A form's rows, in order.
pub struct Lines {
    width: usize,
    lines: Vec<Line<'static>>,
    /// What a click does: on `line`, from `column`, `width` columns.
    targets: Vec<(usize, usize, usize, HitTarget)>,
    focus_line: usize,
    /// The focused input: its line, its first column, its width, and its text.
    input: Option<(usize, usize, usize, TextInput)>,
    dropdown: Option<Dropdown>,
}

fn columns(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

impl Lines {
    /// Rows for a form whose values are `width` columns wide: [`value_width`].
    pub fn new(width: usize) -> Self {
        Self {
            width,
            lines: vec![Line::raw("")],
            targets: Vec::new(),
            focus_line: 0,
            input: None,
            dropdown: None,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    fn push(&mut self, line: Line<'static>) -> usize {
        self.lines.push(line);
        self.lines.len() - 1
    }

    fn target(&mut self, line: usize, column: usize, width: usize, target: HitTarget) {
        self.targets.push((line, column, width, target));
    }

    /// ` › Label`, padded to where values start; the marker only on the focused row.
    fn label(model: &Model, focused: bool, text: &str) -> Vec<Span<'static>> {
        vec![
            Span::styled(
                if focused { " › " } else { "   " },
                bold(model, Role::Focus),
            ),
            Span::styled(
                format!("{text:<LABEL$}"),
                if focused {
                    bold(model, Role::Focus)
                } else {
                    fg(model, Role::Muted)
                },
            ),
        ]
    }

    /// The focus is on the buttons: what is kept in view is the end, next to them.
    pub fn focus_end(&mut self) {
        self.focus_line = self.lines.len().saturating_sub(1);
    }

    pub fn blank(&mut self) {
        self.push(Line::raw(""));
    }

    /// An uppercase title over the rows that answer one question.
    pub fn section(&mut self, model: &Model, title: &str) {
        self.push(Line::styled(format!(" {title}"), bold(model, Role::Border)));
    }

    /// Plain text under the label column, wrapped to the form.
    pub fn note(&mut self, text: impl Into<String>, style: Style) {
        let room = (VALUE + self.width).saturating_sub(3).max(8);
        for part in crate::model::wrap_words(&text.into(), room) {
            self.push(Line::styled(format!("   {part}"), style));
        }
    }

    /// Plain text in the value column, under the row it is about, wrapped to it.
    pub fn aside(&mut self, text: impl Into<String>, style: Style) {
        for part in crate::model::wrap_words(&text.into(), self.width.max(8)) {
            self.push(Line::styled(
                format!("{}{part}", " ".repeat(VALUE + 1)),
                style,
            ));
        }
    }

    /// A choice among a few short options, side by side, the chosen one filled; what it
    /// means after them.
    #[allow(clippy::too_many_arguments)]
    pub fn segmented(
        &mut self,
        model: &Model,
        field: usize,
        label: &str,
        options: &[&str],
        chosen: usize,
        focused: bool,
        after: &str,
    ) {
        let mut spans = Self::label(model, focused, label);
        let mut column = VALUE;
        let mut hits = Vec::new();
        for (index, option) in options.iter().enumerate() {
            let style = match (index == chosen, focused) {
                (true, true) => on(model, Role::OnFocus, Role::Focus).add_modifier(Modifier::BOLD),
                (true, false) => on(model, Role::OnSelection, Role::Selection),
                _ => fg(model, Role::Muted),
            };
            let text = format!(" {option} ");
            let width = text.chars().count();
            hits.push((column, width, index));
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" "));
            column += width + 1;
        }
        if !after.is_empty() {
            spans.push(Span::styled(format!(" {after}"), fg(model, Role::Muted)));
        }
        let line = self.push(Line::from(spans));
        if focused {
            self.focus_line = line;
        }
        self.target(line, 0, VALUE + self.width, HitTarget::FormField(field));
        for (column, width, option) in hits {
            self.target(line, column, width, HitTarget::FormOption { field, option });
        }
    }

    /// A box at `indent`: its top, `label` beside its middle, and its foot with `foot` at
    /// the right.
    fn boxed(
        model: &Model,
        label: Vec<Span<'static>>,
        width: usize,
        content: Vec<Span<'static>>,
        foot: &str,
        edge: Role,
    ) -> [Line<'static>; 3] {
        let indent = columns(&label);
        let edge = fg(model, edge);
        let pad = " ".repeat(indent);
        let inside = width.saturating_sub(2);
        let top = Line::from(vec![
            Span::raw(pad.clone()),
            Span::styled(format!("┌{}┐", "─".repeat(inside)), edge),
        ]);
        let used = columns(&content);
        let mut middle = label;
        middle.push(Span::styled("│ ", edge));
        middle.extend(content);
        middle.push(Span::raw(" ".repeat(inside.saturating_sub(used + 1))));
        middle.push(Span::styled("│", edge));
        let foot = if foot.is_empty() {
            String::new()
        } else {
            format!(" {foot} ")
        };
        let rule = inside.saturating_sub(foot.chars().count() + 1);
        let bottom = Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("└{}", "─".repeat(rule)), edge),
            Span::styled(foot, fg(model, Role::Muted)),
            Span::styled("─┘", edge),
        ]);
        [top, Line::from(middle), bottom]
    }

    fn push_box(&mut self, lines: [Line<'static>; 3], field: usize, focused: bool) -> usize {
        let top = self.lines.len();
        self.lines.extend(lines);
        if focused {
            self.focus_line = top + 1;
        }
        for line in top..top + 3 {
            self.target(line, 0, VALUE + self.width, HitTarget::FormField(field));
        }
        top + 1
    }

    /// One value from a list that grows: the value in a box with `▼`; `open`, the rows
    /// of the list under it and the highlighted one.
    #[allow(clippy::too_many_arguments)]
    pub fn dropdown(
        &mut self,
        model: &Model,
        field: usize,
        label: &str,
        mut value: Vec<Span<'static>>,
        focused: bool,
        invalid: bool,
        open: Option<(Vec<Vec<Span<'static>>>, usize)>,
    ) {
        let narrow = self.width.min(NARROW);
        let used = columns(&value);
        value.push(Span::raw(" ".repeat(narrow.saturating_sub(used + 5))));
        value.push(Span::styled("▼ ", fg(model, Role::Muted)));
        let lines = Self::boxed(
            model,
            Self::label(model, focused, label),
            narrow,
            value,
            "",
            border(focused, invalid),
        );
        let middle = self.push_box(lines, field, focused);
        if let Some((rows, highlight)) = open {
            self.dropdown = Some(Dropdown {
                line: middle + 1,
                rows,
                highlight,
                field,
            });
        }
    }

    /// Several values from small sets, two to a row, each set under its title (none
    /// for ""); `cursor` is the box the focus is on, counted across the sets.
    pub fn grid(
        &mut self,
        model: &Model,
        field: usize,
        label: &str,
        groups: &[(String, Vec<(String, bool)>)],
        cursor: Option<usize>,
    ) {
        // Wide enough for a tool's or a connection's name, not half the form.
        let cell = (self.width / 2).clamp(8, 30);
        let focused = cursor.is_some();
        let lead = |model: &Model, first: bool| {
            if first {
                let mut spans = Self::label(model, focused, label);
                spans.push(Span::raw(" "));
                spans
            } else {
                vec![Span::raw(" ".repeat(VALUE + 1))]
            }
        };
        let mut first = true;
        let mut index = 0;
        for (title, items) in groups {
            if !title.is_empty() {
                let mut spans = lead(model, first);
                spans.push(Span::styled(title.clone(), fg(model, Role::Muted)));
                self.push(Line::from(spans));
                first = false;
            }
            for pair in items.chunks(2) {
                let line = self.lines.len();
                let mut spans = lead(model, first);
                first = false;
                for (column, (name, checked)) in pair.iter().enumerate() {
                    let picked = cursor == Some(index);
                    if picked {
                        self.focus_line = line;
                    }
                    let style = if picked {
                        on(model, Role::OnSelection, Role::Selection)
                    } else {
                        fg(model, Role::Foreground)
                    };
                    let room = cell.saturating_sub(5);
                    let name = crate::model::truncate_cell(name, room);
                    spans.push(Span::styled("[", style));
                    spans.push(Span::styled(
                        if *checked { "x" } else { " " },
                        if picked {
                            style
                        } else {
                            bold(model, Role::Focus)
                        },
                    ));
                    spans.push(Span::styled(format!("] {name:<room$}"), style));
                    spans.push(Span::raw(" "));
                    self.target(
                        line,
                        VALUE + 1 + column * cell,
                        cell,
                        HitTarget::FormOption {
                            field,
                            option: index,
                        },
                    );
                    index += 1;
                }
                self.push(Line::from(spans));
            }
        }
        if index == 0 {
            let mut spans = Self::label(model, focused, label);
            spans.push(Span::styled(" none", fg(model, Role::Muted)));
            self.push(Line::from(spans));
        }
    }

    /// A yes or no, with what the state means after it.
    pub fn toggle(
        &mut self,
        model: &Model,
        field: usize,
        label: &str,
        checked: bool,
        meaning: &str,
        focused: bool,
    ) {
        let mut spans = Self::label(model, focused, label);
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            if checked { "[x] On " } else { "[ ] Off" },
            if focused {
                on(model, Role::OnSelection, Role::Selection)
            } else {
                fg(model, Role::Foreground)
            },
        ));
        spans.push(Span::styled(format!("  {meaning}"), fg(model, Role::Muted)));
        let line = self.push(Line::from(spans));
        if focused {
            self.focus_line = line;
        }
        self.target(
            line,
            0,
            VALUE + self.width,
            HitTarget::FormOption { field, option: 0 },
        );
    }

    /// Free text in a box as wide as the form, `unit` after it and what it expects in
    /// its foot.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        model: &Model,
        field: usize,
        label: &str,
        input: &TextInput,
        unit: &str,
        foot: &str,
        focused: bool,
        invalid: bool,
    ) {
        let room = self.width.saturating_sub(3 + unit.chars().count());
        let (shown, _) = input.window(room);
        // The unit right after the text, not after the box's room.
        let shown = shown.trim_end().to_string();
        let lines = Self::boxed(
            model,
            Self::label(model, focused, label),
            self.width,
            vec![
                Span::styled(shown, fg(model, Role::Foreground)),
                Span::styled(unit.to_string(), fg(model, Role::Muted)),
            ],
            foot,
            border(focused, invalid),
        );
        let middle = self.push_box(lines, field, focused);
        if focused {
            self.input = Some((middle, VALUE + 2, room, input.clone()));
        }
    }

    /// "Type 'target' to confirm", and the box it is typed in.
    pub fn confirm(
        &mut self,
        model: &Model,
        field: usize,
        target: &str,
        input: &TextInput,
        focused: bool,
        invalid: bool,
    ) {
        let style = if focused {
            bold(model, Role::Focus)
        } else {
            fg(model, Role::Foreground)
        };
        let line = self.push(Line::from(vec![
            Span::styled(
                if focused { " › " } else { "   " },
                bold(model, Role::Focus),
            ),
            Span::styled("Type '", style),
            Span::styled(target.to_string(), bold(model, Role::Foreground)),
            Span::styled("' to confirm", style),
        ]));
        self.target(line, 0, VALUE + self.width, HitTarget::FormField(field));
        let narrow = self.width.min(NARROW);
        let room = narrow.saturating_sub(3);
        let (shown, _) = input.window(room);
        let lines = Self::boxed(
            model,
            vec![Span::raw("   ")],
            narrow,
            vec![Span::styled(shown, fg(model, Role::Foreground))],
            "",
            border(focused, invalid),
        );
        let middle = self.push_box(lines, field, focused);
        if focused {
            self.input = Some((middle, 5, room, input.clone()));
        }
    }

    /// What was refused, wrapped, or an empty line.
    pub fn error(&mut self, model: &Model, message: Option<&str>) {
        let Some(message) = message else {
            self.push(Line::raw(""));
            return;
        };
        let room = (VALUE + self.width).saturating_sub(3);
        for (index, part) in crate::model::wrap_words(message, room)
            .into_iter()
            .enumerate()
        {
            let mark = if index == 0 { " × " } else { "   " };
            self.push(Line::styled(
                format!("{mark}{part}"),
                bold(model, Role::Error),
            ));
        }
    }
}

/// A grid's rows as drawn: sets of `sizes` boxes, two to a row, each set starting a row;
/// each row the indexes of its boxes, counted across the sets.
pub fn grid_rows(sizes: &[usize]) -> Vec<Vec<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    for size in sizes {
        for row in (start..start + size).collect::<Vec<_>>().chunks(2) {
            rows.push(row.to_vec());
        }
        start += size;
    }
    rows
}

/// The box a grid's cursor goes to from `at` with Up (`up`) or Down: the row above or
/// below, in the same column or the last there is; `None` past the first or last row,
/// where the focus leaves the grid.
pub fn grid_step(rows: &[Vec<usize>], at: usize, up: bool) -> Option<usize> {
    let (row, column) = rows
        .iter()
        .enumerate()
        .find_map(|(row, boxes)| boxes.iter().position(|b| *b == at).map(|c| (row, c)))?;
    let target = if up { row.checked_sub(1)? } else { row + 1 };
    let boxes = rows.get(target)?;
    Some(boxes[column.min(boxes.len() - 1)])
}

/// Where a grid's cursor lands as the focus comes in: its first row, or its last coming
/// from below, in the column it was in.
pub fn grid_enter(rows: &[Vec<usize>], at: usize, from_below: bool) -> usize {
    let column = rows
        .iter()
        .find_map(|boxes| boxes.iter().position(|b| *b == at))
        .unwrap_or(0);
    let boxes = if from_below {
        rows.last()
    } else {
        rows.first()
    };
    boxes.map_or(0, |boxes| boxes[column.min(boxes.len() - 1)])
}

/// A button: `[   Label ]`, or `[ > Label ]` focused, the marker's room kept either way.
pub struct Button<'a> {
    pub label: &'a str,
    pub focused: bool,
    /// Its colour: the accent, `Warning` for a write to production, `Error` to delete.
    pub tone: Role,
    pub primary: bool,
    pub target: HitTarget,
}

fn button_span(model: &Model, button: &Button) -> Span<'static> {
    let label = button.label;
    if button.focused {
        Span::styled(
            format!("[ > {label} ]"),
            on(model, Role::OnFocus, button.tone).add_modifier(Modifier::BOLD),
        )
    } else if button.primary {
        Span::styled(format!("[   {label} ]"), bold(model, button.tone))
    } else {
        Span::styled(format!("[   {label} ]"), fg(model, Role::Muted))
    }
}

/// Keys and what they do, the key in the accent.
pub fn hints(model: &Model, pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, what)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), bold(model, Role::Focus)));
        spans.push(Span::styled(format!(" {what}"), fg(model, Role::Muted)));
    }
    Line::from(spans)
}

/// The form in `area`, a screen's pane: framed, the focused row kept in view, then a
/// blank line, the buttons and the keys.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    title: &str,
    lines: Lines,
    buttons: &[Button],
    keys: &[(&str, &str)],
) {
    let Lines {
        width,
        lines,
        targets,
        focus_line,
        input,
        dropdown,
    } = lines;
    // The pane's own frame, focused: the form is where the keys go.
    let block = crate::render::pane_block(model, title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // The frame fills the pane; the buttons follow the last row, not the pane's foot.
    let rows = (lines.len() as u16).min(inner.height.saturating_sub(3));
    let [body, _, row, keys_row, _] = Layout::vertical([
        Constraint::Length(rows),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    // Scrolled only as far as keeps the focused row, and the box under it, in view.
    let offset = (focus_line + 2).saturating_sub(usize::from(body.height));
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), body);
    let shown = |line: usize| {
        line.checked_sub(offset)
            .filter(|line| *line < usize::from(body.height))
    };
    for (line, column, width, target) in targets {
        if let Some(line) = shown(line) {
            let rect = Rect {
                x: body.x + column as u16,
                y: body.y + line as u16,
                width: (width as u16).min(body.width.saturating_sub(column as u16)),
                height: 1,
            };
            hits.register(target, rect);
        }
    }
    if let Some((line, column, room, text)) = input
        && let Some(line) = shown(line)
    {
        let rect = Rect {
            x: body.x + column as u16,
            y: body.y + line as u16,
            width: (room as u16).min(body.width.saturating_sub(column as u16)),
            height: 1,
        };
        crate::render::show_windowed_input(frame, rect, "", &text);
    }

    let mut spans = Vec::new();
    let mut places = Vec::new();
    for (index, button) in buttons.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let span = button_span(model, button);
        places.push((columns(&spans), span.content.chars().count(), button.target));
        spans.push(span);
    }
    let used = columns(&spans) as u16;
    let start = row.x + row.width.saturating_sub(used) / 2;
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        row,
    );
    for (column, width, target) in places {
        hits.register(
            target,
            Rect {
                x: start + column as u16,
                y: row.y,
                width: width as u16,
                height: 1,
            },
        );
    }
    frame.render_widget(Paragraph::new(hints(model, keys)), keys_row);

    // The open list, over what is under its box.
    if let Some(dropdown) = dropdown
        && let Some(line) = shown(dropdown.line)
    {
        let screen = frame.area();
        let narrow = width.min(NARROW) as u16;
        let height = (dropdown.rows.len() as u16 + 2).min(screen.height);
        let list = Rect {
            x: body.x + VALUE as u16,
            y: (body.y + line as u16).min(screen.height.saturating_sub(height)),
            width: narrow.min(screen.width.saturating_sub(body.x + VALUE as u16)),
            height,
        };
        let inside = usize::from(narrow.saturating_sub(2));
        let rows: Vec<Line> = dropdown
            .rows
            .into_iter()
            .enumerate()
            .map(|(index, mut spans)| {
                let used = columns(&spans);
                spans.insert(0, Span::raw(" "));
                spans.push(Span::raw(" ".repeat(inside.saturating_sub(used + 1))));
                let line = Line::from(spans);
                if index == dropdown.highlight {
                    line.style(on(model, Role::OnFocus, Role::Focus))
                } else {
                    line
                }
            })
            .collect();
        frame.render_widget(Clear, list);
        frame.render_widget(
            Paragraph::new(rows).block(
                Block::bordered()
                    .border_style(fg(model, Role::Focus))
                    .style(model.theme.base(model.capabilities)),
            ),
            list,
        );
        for index in 0..list.height.saturating_sub(2) {
            hits.register(
                HitTarget::FormOption {
                    field: dropdown.field,
                    option: usize::from(index),
                },
                Rect {
                    x: list.x + 1,
                    y: list.y + 1 + index,
                    width: list.width.saturating_sub(2),
                    height: 1,
                },
            );
        }
    }
}
