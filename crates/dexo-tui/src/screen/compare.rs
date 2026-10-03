//! Compare: two schemas side by side as differences, the statement for the picked one or
//! the whole migration script beside them. It was Compare Schema, one dialog of
//! eighty-eight columns that held the sources, the list and the script in turn.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use super::Button;
use super::widgets::{self, Chip, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::schema_diff::kind_heading;
use crate::theme::Role;
use crate::widgets::form::FooterFocus;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let diff = &model.schema_diff;
    let rest = sources_row(frame, area, model, hits);
    if diff.source_prompt {
        picking(frame, rest, model, hits);
        return;
    }
    let shown = diff.filtered();
    if shown.is_empty() {
        if diff.loading {
            super::empty_state(frame, rest, model, &["Reading both schemas...".into()]);
        } else if diff.entries.is_empty() {
            let mut lines = vec!["✓ The schemas match.".to_string()];
            lines.extend(diff.error.clone());
            super::empty_state(frame, rest, model, &lines);
        } else {
            widgets::empty_with_buttons(
                frame,
                rest,
                model,
                hits,
                &["Every difference is hidden.".to_string()],
                &[Button::new(KeyCode::Esc, "Show all")],
            );
        }
        return;
    }
    let (list, detail) = super::list_and_detail(model, hits, rest, shown.len() + 4);
    list_pane(frame, list, model, hits);
    detail_pane(frame, detail, model, hits);
}

/// From and To, each a picker its arrows step, beside the filters and what compares:
/// they stay in sight with the result under them.
fn sources_row(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) -> Rect {
    let diff = &model.schema_diff;
    if area.height < 2 {
        return area;
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut spans = Vec::new();
    let mut x = 0u16;
    for (side, label) in [(0usize, "From"), (1, "To")] {
        if side == 1 {
            spans.push(Span::styled(" ⇄ ", muted));
            x += 3;
        }
        let focused = diff.source_prompt && diff.footer == FooterFocus::Input && diff.row == side;
        let name = crate::model::truncate_cell(&diff.side_name(side), 28);
        let text = format!("{label} ‹ {name} ›");
        let width = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;
        let style = if focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(if focused { ">" } else { " " }, muted));
        spans.push(Span::styled(text.clone(), style));
        let start = row.x + x + 1;
        let label_width = label.len() as u16 + 1;
        hits.register(
            HitTarget::FormField(side),
            Rect::new(start, row.y, label_width, 1),
        );
        // The arrows step the side; `‹` sits after the label, `›` at the end.
        hits.register(
            HitTarget::FormChoice {
                index: side,
                step: -1,
            },
            Rect::new(start + label_width, row.y, 2, 1),
        );
        hits.register(
            HitTarget::FormChoice {
                index: side,
                step: 1,
            },
            Rect::new(start + width.saturating_sub(2), row.y, 2, 1),
        );
        x += width + 1;
    }
    spans.push(Span::raw("  "));
    x += 2;
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
    let chips = if diff.source_prompt || diff.entries.is_empty() {
        Vec::new()
    } else {
        chips(model)
    };
    widgets::toolbar(
        frame,
        Rect::new(
            area.x + x.min(area.width),
            area.y,
            area.width.saturating_sub(x),
            area.height,
        ),
        model,
        hits,
        None,
        &chips,
        &toolbar_buttons(model),
    );
    Rect::new(area.x, area.y + 1, area.width, area.height - 1)
}

/// The kinds of difference with how many there are, each a filter.
fn chips(model: &Model) -> Vec<Chip> {
    let diff = &model.schema_diff;
    let count = |kind: &str| {
        diff.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .count()
    };
    [
        ('a', '+', "added", diff.show_added),
        ('r', '−', "removed", diff.show_removed),
        ('c', '~', "changed", diff.show_changed),
    ]
    .into_iter()
    .map(|(key, sign, kind, shown)| Chip {
        key: KeyCode::Char(key),
        label: format!(
            "{sign}{} {kind}{}",
            count(kind),
            if shown { "" } else { " (hidden)" }
        ),
        // Lit when off its default, as every filter is: a kind hidden.
        active: !shown,
    })
    .collect()
}

/// Exchange the sides, and compare them.
pub fn toolbar_buttons(model: &Model) -> Vec<Button> {
    let diff = &model.schema_diff;
    if diff.loading {
        return vec![
            Button::new(KeyCode::Char('s'), "Swap").disabled("Both schemas are being read."),
            Button::new(KeyCode::Char('e'), "Compare").disabled("Both schemas are being read."),
        ];
    }
    vec![
        Button::new(KeyCode::Char('s'), "Swap"),
        Button::new(KeyCode::Char('e'), "Compare"),
    ]
}

/// What can be done with the result: open the script, read all of it, copy it.
pub fn buttons(model: &Model) -> Vec<Button> {
    let diff = &model.schema_diff;
    if diff.source_prompt || diff.filtered().is_empty() {
        return Vec::new();
    }
    // Being compared again, the result shown is about to go: nothing is taken from it.
    let reading = "Both schemas are being read again.";
    let open = if diff.loading {
        Button::new(KeyCode::Enter, "Open script").disabled(reading)
    } else {
        Button::new(KeyCode::Enter, "Open script")
            .enabled_if(!diff.script.is_empty(), "There is no script to open.")
    };
    let copy = if diff.loading {
        Button::new(KeyCode::Char('y'), "Copy").disabled(reading)
    } else {
        Button::new(KeyCode::Char('y'), "Copy").enabled_if(
            !diff.shown_sql().trim().is_empty(),
            "This difference has no statement to copy.",
        )
    };
    vec![
        open,
        Button::new(
            KeyCode::Char('w'),
            if diff.whole_script {
                "This change"
            } else {
                "Whole script"
            },
        ),
        copy,
    ]
}

/// The sources being picked: a file's path when a side is one, what is wrong, and the
/// buttons.
fn picking(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let diff = &model.schema_diff;
    let width = area.width.min(100);
    let mut lines: Vec<String> = Vec::new();
    let mut file_row = None;
    if diff.uses_file() {
        let focused = diff.footer == FooterFocus::Input && diff.row == 2;
        file_row = Some(lines.len());
        lines.push(diff.file.inline_line_within(
            &format!("{} File: ", if focused { ">" } else { " " }),
            focused,
            usize::from(width.saturating_sub(2)),
        ));
        lines.push(String::new());
    }
    lines.push(match &diff.error {
        Some(error) if !diff.loading => error.clone(),
        _ if diff.loading => "Reading both schemas...".into(),
        _ if diff.options.len() < 2 => {
            "Connect a second connection or save a snapshot (dexo schema snapshot) to compare."
                .into()
        }
        _ => "Pick two sources and Compare: the script makes From like To.".into(),
    });
    lines.push(String::new());
    let footer_row = lines.len();
    let footer = crate::widgets::form::footer_line(diff.submit_label(), diff.footer);
    lines.push(footer.clone());
    let height = (lines.len() as u16 + 2).min(area.height);
    let pane = Rect::new(area.x, area.y, width, height);
    let block = crate::render::pane_block(model, "Sources", true);
    let inner = block.inner(pane);
    frame.render_widget(block, pane);
    hits.register(HitTarget::ScreenList, pane);
    frame.render_widget(Paragraph::new(lines.join("\n")), inner);
    for index in 0..lines.len().min(usize::from(inner.height)) {
        let rect = crate::mouse::line_rect(inner, index);
        if index == footer_row {
            crate::widgets::form::register_footer(hits, rect, &footer, diff.submit_label());
        } else if Some(index) == file_row {
            hits.register(HitTarget::FormField(2), rect);
            if diff.row == 2 && diff.footer == FooterFocus::Input {
                crate::render::show_input(frame, rect, "> File: ", &diff.file, false);
            }
        }
    }
}

/// The differences under the kind of object each is about, with its sign and risk.
fn list_pane(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let diff = &model.schema_diff;
    let shown = diff.filtered();
    let title = if diff.filtering() {
        format!("Differences ({} of {})", shown.len(), diff.entries.len())
    } else {
        format!("Differences ({})", shown.len())
    };
    let block =
        crate::render::pane_block(model, &title, super::section(model) == super::Section::List);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    let style = |role: Role| model.theme.style(role, model.capabilities);
    let mut entries = Vec::new();
    let mut kind = "";
    for (index, entry) in shown.iter().enumerate() {
        if entry.object_kind() != kind {
            kind = entry.object_kind();
            entries.push(Entry::text(kind_heading(kind), true));
        }
        let (sign, role) = match entry.kind {
            "added" => ("+", Role::Success),
            "removed" => ("−", Role::Error),
            _ => ("~", Role::Warning),
        };
        let mut spans = vec![
            Span::styled(sign, style(role)),
            Span::raw(format!(" {}", entry.name())),
        ];
        if !entry.risk.is_empty() {
            spans.push(Span::styled(
                format!("  {}", entry.risk),
                style(Role::Warning),
            ));
        }
        entries.push(Entry {
            spans,
            target: Some(HitTarget::ListRow(index)),
            picked: index == diff.selected,
            heading: false,
        });
    }
    widgets::entries(frame, inner, model, hits, &entries);
}

/// The picked difference -- what changes and its risk -- and its statement in colour, or
/// the whole script.
fn detail_pane(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let diff = &model.schema_diff;
    let width = area.width.saturating_sub(2);
    let mut text: Vec<Line> = Vec::new();
    let title = if diff.whole_script {
        "Migration script".to_string()
    } else if let Some(entry) = diff.picked() {
        text.extend(widgets::field_lines(
            model,
            &[
                FieldRow::Field("Change", entry.kind.to_string()),
                FieldRow::Field("Object", entry.object.clone()),
                FieldRow::Field(
                    "Risk",
                    if entry.risk.is_empty() {
                        "none".into()
                    } else {
                        entry.risk.clone()
                    },
                ),
            ],
            width,
        ));
        text.push(Line::default());
        entry.name().to_string()
    } else {
        String::new()
    };
    let sql = diff.shown_sql();
    if sql.trim().is_empty() {
        text.push(Line::raw(
            " No statement: this difference is left to be made by hand.",
        ));
    } else {
        let driver = diff
            .from_connection
            .as_ref()
            .and_then(|name| {
                model
                    .connections
                    .profiles
                    .iter()
                    .find(|row| &row.profile.name == name)
            })
            .map_or("postgres", |row| row.profile.driver.as_str());
        text.extend(
            crate::widgets::editor::sql_lines(
                &sql,
                usize::from(width.saturating_sub(1)),
                dexo_app::dialect_for_driver(driver),
            )
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(line.spans);
                Line::from(spans)
            }),
        );
    }
    let drawn = super::detail_pane(
        frame,
        area,
        model,
        hits,
        &title,
        &buttons(model),
        Text::from(text),
        usize::from(diff.scroll),
        &diff.error.clone().into_iter().collect::<Vec<_>>(),
    );
    hits.set_scroll_limit(crate::mouse::ScrollArea::SchemaDiff, drawn.max_scroll);
    hits.set_page(crate::mouse::ScrollArea::SchemaDiff, drawn.page);
}

pub fn hints(model: &Model) -> String {
    let diff = &model.schema_diff;
    if diff.source_prompt {
        "Up/Down row  Left/Right change a source  s swap  Enter compare  Esc back".into()
    } else {
        "Up/Down pick  a/r/c filter  p sources  PgUp/PgDn read  Esc back".into()
    }
}
