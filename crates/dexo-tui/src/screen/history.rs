//! History: every statement run and every query saved, searched as you type, with the
//! picked one in full beside the list. They were two dialogs, one a statement to a line
//! and cut at its edge.

use crossterm::event::KeyCode;
use dexo_storage::HistoryOutcome;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use super::Button;
use super::widgets::{self, Chip, Entry, FieldRow};
use crate::model::Model;
use crate::mouse::{HitMap, HitTarget};
use crate::screens::editor::StatusFilter;
use crate::theme::Role;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HistoryView {
    #[default]
    History,
    Saved,
}

impl HistoryView {
    pub const ALL: [HistoryView; 2] = [HistoryView::History, HistoryView::Saved];

    pub fn other(self) -> Self {
        match self {
            HistoryView::History => HistoryView::Saved,
            HistoryView::Saved => HistoryView::History,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let views = [
        (
            "History".to_string(),
            model.history_view == HistoryView::History,
        ),
        (
            "Saved".to_string(),
            model.history_view == HistoryView::Saved,
        ),
    ];
    // The views on the left of the toolbar's row.
    let views_width = views
        .iter()
        .map(|(label, _)| label.chars().count() as u16 + 5)
        .sum::<u16>()
        .min(area.width);
    super::views_bar(
        frame,
        Rect::new(area.x, area.y, views_width, area.height),
        model,
        hits,
        &views,
    );
    let tools = Rect::new(
        area.x + views_width,
        area.y,
        area.width - views_width,
        area.height,
    );
    let rest = match model.history_view {
        HistoryView::History => widgets::toolbar(
            frame,
            tools,
            model,
            hits,
            Some(&model.editor.history_search),
            &chips(model),
            &[],
        ),
        HistoryView::Saved => {
            // The search, typed into from anywhere on the screen.
            let row = Rect::new(tools.x, tools.y, tools.width, 1.min(tools.height));
            let search = &model.saved_queries.search;
            let before = "Search: ";
            frame.render_widget(Paragraph::new(format!("{before}{}", search.as_str())), row);
            if model.saved_queries.renaming.is_none() {
                crate::render::paint_selection(frame, row, before, search, true);
                crate::render::show_input(frame, row, before, search, false);
            }
            tools
        }
    };
    let rest = Rect::new(
        area.x,
        rest.y,
        area.width,
        area.bottom().saturating_sub(rest.y),
    );
    if rest.y == area.y {
        // No room for the toolbar: the views' row is all there is.
        let rest = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        return draw_view(frame, rest, model, hits);
    }
    draw_view(frame, rest, model, hits);
}

fn draw_view(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    match model.history_view {
        HistoryView::History => history(frame, area, model, hits),
        HistoryView::Saved => saved(frame, area, model, hits),
    }
}

/// The filters over the list: which connection's runs, and which outcome.
fn chips(model: &Model) -> Vec<Chip> {
    let editor = &model.editor;
    vec![
        Chip {
            key: KeyCode::Char('c'),
            label: format!(
                "Connection: {}",
                editor.history_connection.as_deref().unwrap_or("all")
            ),
            active: editor.history_connection.is_some(),
        },
        Chip {
            key: KeyCode::Char('f'),
            label: format!("Status: {}", editor.history_status.label()),
            active: editor.history_status != StatusFilter::All,
        },
    ]
}

fn history(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let editor = &model.editor;
    let lines = editor.history_lines();
    if lines.is_empty() {
        if editor.history.is_empty() {
            super::empty_state(
                frame,
                area,
                model,
                &[
                    "Nothing has run yet.".to_string(),
                    "What runs on any connection is kept here.".to_string(),
                ],
            );
        } else {
            widgets::empty_with_buttons(
                frame,
                area,
                model,
                hits,
                &["Nothing matches the filters.".to_string()],
                &[Button::new(KeyCode::Esc, "Clear filters")],
            );
        }
        return;
    }
    let picked = editor.history_selected.min(lines.len() - 1);
    let (list, detail) = super::list_and_detail(area, lines.len() + 2);
    list_pane(frame, list, model, hits, &lines, picked);
    let line = &lines[picked];
    let row = line.row;
    let width = detail.width.saturating_sub(2);
    let mut text =
        crate::widgets::editor::sql_lines(&row.sql, usize::from(width), dialect(model, row))
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(line.spans);
                Line::from(spans)
            })
            .collect::<Vec<_>>();
    text.push(Line::default());
    text.extend(widgets::field_lines(model, &run_fields(line), width));
    let title = format!(
        "{} · {} · {} {}",
        row.connection_id.as_deref().unwrap_or("no connection"),
        when(&row.created_at),
        outcome_glyph(row.outcome),
        outcome_word(row.outcome)
    );
    super::detail_pane(
        frame,
        detail,
        model,
        hits,
        &title,
        &[Button::new(KeyCode::Enter, "Open")],
        Text::from(text),
        usize::from(super::detail_scroll(model)),
        &[],
    );
}

/// The statements under their day, newest first; the pick kept in sight.
fn list_pane(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    lines: &[crate::screens::editor::HistoryLine<'_>],
    picked: usize,
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let editor = &model.editor;
    let title = if editor.history_filtered() {
        format!("Statements ({} shown)", lines.len())
    } else {
        format!("Statements ({})", lines.len())
    };
    let block =
        crate::render::pane_block(model, &title, super::section(model) == super::Section::List);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    let style = |role: Role| model.theme.style(role, model.capabilities);
    // The rows column goes first when the pane is narrow; the statement takes the rest.
    let with_rows = inner.width >= 70;
    let widest = |text: &dyn Fn(&dexo_storage::HistoryRow) -> String, most: usize| {
        lines
            .iter()
            .map(|line| unicode_width::UnicodeWidthStr::width(text(line.row).as_str()))
            .max()
            .unwrap_or(0)
            .min(most)
    };
    let connection_width = widest(
        &|row| row.connection_id.clone().unwrap_or_else(|| "-".into()),
        16,
    );
    let took_width = widest(&|row| row.duration_ms.map(took).unwrap_or_default(), 8);
    let rows_width = widest(&|row| row.rows.map(rows).unwrap_or_default(), 10);
    let mut entries = Vec::new();
    let mut day = String::new();
    for (index, line) in lines.iter().enumerate() {
        let row = line.row;
        let this_day = day_of(&row.created_at);
        if this_day != day {
            entries.push(Entry::text(this_day.clone(), true));
            day = this_day;
        }
        let glyph_role = match row.outcome {
            HistoryOutcome::Ok => Role::Success,
            HistoryOutcome::Failed => Role::Error,
            HistoryOutcome::Cancelled => Role::Warning,
        };
        let mut spans = vec![
            Span::styled(outcome_glyph(row.outcome), style(glyph_role)),
            Span::raw(format!(
                " {}  {}  {:>took_width$}  ",
                time_of(&row.created_at),
                cell(
                    row.connection_id.as_deref().unwrap_or("-"),
                    connection_width
                ),
                row.duration_ms.map(took).unwrap_or_default()
            )),
        ];
        if with_rows && rows_width > 0 {
            spans.push(Span::raw(format!(
                "{:>rows_width$}  ",
                row.rows.map(rows).unwrap_or_default()
            )));
        }
        spans.push(Span::raw(one_line(&row.sql)));
        entries.push(Entry {
            spans,
            target: Some(HitTarget::ListRow(index)),
            picked: index == picked,
            heading: false,
        });
    }
    widgets::entries(frame, inner, model, hits, &entries);
}

/// The run in fields: where, when, how long, what came of it, and how often it ran.
fn run_fields(line: &crate::screens::editor::HistoryLine<'_>) -> Vec<FieldRow> {
    let row = line.row;
    let mut fields = vec![FieldRow::Section("Run")];
    let connection = row.connection_id.as_deref().unwrap_or("-");
    fields.push(FieldRow::Field(
        "Connection",
        match &row.database {
            Some(database) => format!("{connection} ({database})"),
            None => connection.to_string(),
        },
    ));
    fields.push(FieldRow::Field("When", full_time(&row.created_at)));
    if let Some(ms) = row.duration_ms {
        fields.push(FieldRow::Field("Took", took(ms)));
    }
    if let Some(count) = row.rows {
        fields.push(FieldRow::Field("Rows", count.to_string()));
    }
    fields.push(FieldRow::Field(
        "Result",
        match row.outcome {
            HistoryOutcome::Ok => "ok".into(),
            HistoryOutcome::Cancelled => "cancelled".into(),
            HistoryOutcome::Failed => row.error.clone().unwrap_or_else(|| "failed".into()),
        },
    ));
    fields.push(FieldRow::Field("Runs", line.ids.len().to_string()));
    fields
}

/// The dialect the run's connection speaks, for its colours.
fn dialect(model: &Model, row: &dexo_storage::HistoryRow) -> dexo_sql::Dialect {
    let driver = model
        .connections
        .profiles
        .iter()
        .find(|profile| Some(&profile.profile.name) == row.connection_id.as_ref())
        .map_or("postgres", |profile| profile.profile.driver.as_str());
    dexo_app::dialect_for_driver(driver)
}

fn outcome_glyph(outcome: HistoryOutcome) -> &'static str {
    match outcome {
        HistoryOutcome::Ok => "✓",
        HistoryOutcome::Failed => "✗",
        HistoryOutcome::Cancelled => "⊘",
    }
}

fn outcome_word(outcome: HistoryOutcome) -> &'static str {
    match outcome {
        HistoryOutcome::Ok => "ok",
        HistoryOutcome::Failed => "failed",
        HistoryOutcome::Cancelled => "cancelled",
    }
}

/// How long a run took, in the unit that reads.
fn took(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1_000.0),
        _ => format!("{}m{:02}s", ms / 60_000, ms / 1_000 % 60),
    }
}

fn rows(count: u64) -> String {
    if count == 1 {
        "1 row".into()
    } else {
        format!("{count} rows")
    }
}

/// `text` in exactly `width` cells.
fn cell(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let text = crate::model::truncate_cell(text, width);
    let pad = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}

fn saved(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let picker = &model.saved_queries;
    let filtered = picker.filtered();
    if filtered.is_empty() {
        let line = if picker.items.is_none() {
            "Reading the saved queries...".to_string()
        } else if picker.items.as_ref().is_some_and(Vec::is_empty) {
            match crate::palette::shortcut_for(model, "editor.save_query", None) {
                Some(key) => format!("No saved queries yet; {key} saves one from the editor."),
                None => "No saved queries yet; Save Query As saves one.".into(),
            }
        } else {
            "No saved query matches.".into()
        };
        let mut lines = vec![line];
        lines.extend(picker.error.clone());
        super::empty_state(frame, area, model, &lines);
        return;
    }
    // A query of another connection says whose it is.
    let current = crate::update::query_connection(model);
    let rows: Vec<String> = filtered
        .iter()
        .enumerate()
        .map(|(index, query)| {
            let name = match (&picker.renaming, index == picker.selected) {
                (Some(input), true) => input.inline_line("", true),
                _ => query.name.clone(),
            };
            if current.as_deref() == Some(query.connection_id.as_str()) {
                name
            } else {
                let owner = model
                    .connections
                    .profiles
                    .iter()
                    .find(|row| row.profile.id.0.to_string() == query.connection_id)
                    .map_or("another connection".to_string(), |row| {
                        row.profile.name.clone()
                    });
                format!("{name} · {owner}")
            }
        })
        .collect();
    let picked = picker.selected.min(rows.len() - 1);
    let (list, detail) = super::list_and_detail(area, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Saved queries ({})", rows.len()),
        None,
        &rows,
        Some(picked),
    );
    let width = usize::from(detail.width.saturating_sub(2)).max(8);
    let lines = statement_lines(&filtered[picked].sql, width);
    let mut footer = Vec::new();
    if let Some(focus) = picker.deleting {
        footer.push(format!(
            "Delete {}? It cannot be undone.",
            filtered[picked].name
        ));
        footer.push(crate::widgets::form::footer_line("Delete", focus));
    } else {
        footer.extend(picker.error.clone());
    }
    let (footer_area, _, _) = super::text_pane(
        frame,
        detail,
        model,
        hits,
        &filtered[picked].name,
        &lines,
        usize::from(super::detail_scroll(model)),
        &footer,
    );
    if picker.deleting.is_some() {
        for (index, line) in footer
            .iter()
            .enumerate()
            .take(usize::from(footer_area.height))
        {
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(
                    hits,
                    crate::mouse::line_rect(footer_area, index),
                    line,
                    "Delete",
                );
            }
        }
    }
}

/// A statement whole, wrapped to `width`.
fn statement_lines(sql: &str, width: usize) -> Vec<String> {
    sql.lines()
        .flat_map(|line| crate::model::wrap_display_text(line, width))
        .collect()
}

fn one_line(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn local(created_at: &str) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|utc| utc.and_utc().with_timezone(&chrono::Local))
}

/// When a statement ran, in local time: the hour today, the day and hour before.
fn when(created_at: &str) -> String {
    let Some(local) = local(created_at) else {
        return created_at.chars().take(11).collect();
    };
    if local.date_naive() == chrono::Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%m-%d %H:%M").to_string()
    }
}

fn time_of(created_at: &str) -> String {
    local(created_at).map_or_else(
        || created_at.chars().take(5).collect(),
        |local| local.format("%H:%M").to_string(),
    )
}

fn full_time(created_at: &str) -> String {
    local(created_at).map_or_else(
        || created_at.to_string(),
        |local| local.format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// The day heading a run goes under: Today, Yesterday, or its weekday and date.
fn day_of(created_at: &str) -> String {
    let Some(local) = local(created_at) else {
        return created_at.chars().take(10).collect();
    };
    let today = chrono::Local::now().date_naive();
    let day = local.date_naive();
    if day == today {
        "Today".into()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".into()
    } else {
        local.format("%a %d %b %Y").to_string()
    }
}

pub fn hints(model: &Model) -> String {
    match model.history_view {
        HistoryView::History if model.editor.history_search.typing => {
            "Type to search  Up/Down pick  Enter keep  Esc clear".into()
        }
        HistoryView::History => {
            "Up/Down pick  Enter open  / search  c connection  f status  Tab saved  Esc back"
                .into()
        }
        HistoryView::Saved if model.saved_queries.renaming.is_some() => {
            "type the name  Enter rename  Esc cancel".into()
        }
        HistoryView::Saved if model.saved_queries.deleting.is_some() => {
            "Left/Right pick  Enter answer  Esc cancel".into()
        }
        HistoryView::Saved => {
            "type to search  Up/Down pick  Enter open  F2 rename  Delete delete  Tab history  Esc back"
                .into()
        }
    }
}
