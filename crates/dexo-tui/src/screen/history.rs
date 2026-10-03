//! History: every statement run and every query saved, searched as you type, with the
//! picked one in full beside the list. They were two dialogs, one a statement to a line
//! and cut at its edge.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::model::Model;
use crate::mouse::HitMap;

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
    let body = super::views_bar(frame, area, model, hits, &views);
    // The search, typed into from anywhere on the screen.
    let row = Rect::new(body.x, body.y, body.width, 1.min(body.height));
    let (search, editing) = match model.history_view {
        HistoryView::History => (&model.editor.history_search, true),
        HistoryView::Saved => (
            &model.saved_queries.search,
            model.saved_queries.renaming.is_none(),
        ),
    };
    let before = "Search: ";
    frame.render_widget(Paragraph::new(format!("{before}{}", search.as_str())), row);
    if editing {
        crate::render::paint_selection(frame, row, before, search, true);
        crate::render::show_input(frame, row, before, search, false);
    }
    let rest = Rect::new(
        body.x,
        body.y + row.height,
        body.width,
        body.height.saturating_sub(row.height),
    );
    match model.history_view {
        HistoryView::History => history(frame, rest, model, hits),
        HistoryView::Saved => saved(frame, rest, model, hits),
    }
}

fn history(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let editor = &model.editor;
    let matches = editor.history_matches();
    if matches.is_empty() {
        let line = if editor.history.is_empty() {
            match model.connection.name.as_str() {
                "" => "Nothing has run yet.".to_string(),
                name => format!("Nothing has run yet on {name}."),
            }
        } else {
            "No statement matches.".to_string()
        };
        super::empty_state(frame, area, model, &[line]);
        return;
    }
    let rows: Vec<String> = matches
        .iter()
        .map(|row| {
            format!(
                "{:<11}  {:<12}  {}",
                when(&row.created_at),
                crate::model::truncate_cell(row.connection_id.as_deref().unwrap_or("-"), 12),
                one_line(&row.sql)
            )
        })
        .collect();
    let picked = editor.history_selected.min(rows.len() - 1);
    let (list, detail) = super::list_and_detail(area, rows.len());
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &format!("Statements ({})", rows.len()),
        None,
        &rows,
        Some(picked),
    );
    let width = usize::from(detail.width.saturating_sub(2)).max(8);
    let lines = statement_lines(&matches[picked].sql, width);
    super::text_pane(
        frame,
        detail,
        model,
        hits,
        "Statement",
        &lines,
        usize::from(super::detail_scroll(model)),
        &[],
    );
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

/// When a statement ran, in local time: the hour today, the day and hour before.
fn when(created_at: &str) -> String {
    let Ok(utc) = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S") else {
        return created_at.chars().take(11).collect();
    };
    let local = utc.and_utc().with_timezone(&chrono::Local);
    if local.date_naive() == chrono::Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%m-%d %H:%M").to_string()
    }
}

pub fn hints(model: &Model) -> String {
    match model.history_view {
        HistoryView::History => {
            "type to search  Up/Down pick  Enter open  Tab saved  Esc back".into()
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
