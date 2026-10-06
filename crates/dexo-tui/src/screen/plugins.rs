//! Plugins: what is installed and what the index offers, the pick in full beside the
//! list with what can be done to it.

use crossterm::event::KeyCode;
use dexo_app::plugin::manifest::Kind;
use dexo_app::plugin::state::Origin;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use super::Button;
use super::widgets::{self, Chip, FieldRow};
use crate::model::Model;
use crate::mouse::HitMap;
use crate::screens::plugins::{FromFile, PluginRow, PluginsView, StagedInfo, TypeFilter};
use crate::theme::Role;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let state = &model.plugins;
    let views = [
        (
            "Installed".to_string(),
            state.view == PluginsView::Installed,
        ),
        (
            "Available".to_string(),
            state.view == PluginsView::Available,
        ),
    ];
    let chips = [Chip {
        key: KeyCode::Char('t'),
        label: format!("Type: {}", state.filter.label()),
        active: state.filter != TypeFilter::All,
    }];
    let rest = super::views_and_toolbar(
        frame,
        area,
        model,
        hits,
        &views,
        Some(&state.search),
        &chips,
        &[],
    );
    let rest = notice(frame, rest, model);
    let rows = state.shown();
    if rows.is_empty() && state.from_file.is_none() {
        widgets::empty_with_buttons(
            frame,
            rest,
            model,
            hits,
            &empty_lines(model),
            &list_buttons(model),
        );
        return;
    }
    let picked = state.selected.min(rows.len().saturating_sub(1));
    let (list, detail) = super::list_and_detail(model, hits, rest, rows.len() + 2);
    let width = rows
        .iter()
        .map(|row| row.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(24);
    let unicode = model.capabilities.unicode;
    let lines: Vec<String> = rows
        .iter()
        .map(|row| list_line(row, width, unicode))
        .collect();
    let title = match state.view {
        PluginsView::Installed => format!("Installed ({})", rows.len()),
        PluginsView::Available => format!("Available ({})", rows.len()),
    };
    super::list_pane(
        frame,
        list,
        model,
        hits,
        &title,
        None,
        &lines,
        (!rows.is_empty()).then_some(picked),
    );
    let text_width = usize::from(detail.width.saturating_sub(2));
    let (title, text) = match (&state.from_file, &state.removing, rows.get(picked)) {
        (Some(from_file), _, _) => (
            "Install from file".to_string(),
            from_file_lines(model, from_file, text_width),
        ),
        (None, Some(_), Some(row)) => (format!("Remove {}?", row.name), removal_lines(row)),
        (None, _, Some(row)) => (
            row.name.clone(),
            detail_lines(model, row, detail.width.saturating_sub(2)),
        ),
        (None, _, None) => return,
    };
    let drawn = super::detail_pane(
        frame,
        detail,
        model,
        hits,
        &title,
        &buttons(model),
        Text::from(text),
        usize::from(super::detail_scroll(model)),
        &footer(model),
    );
    if let Some(from_file) = &state.from_file {
        input_cursor(frame, drawn.body, from_file, text_width);
    }
}

/// Where a file's path or the plugin's id is being typed, the cursor.
fn input_cursor(frame: &mut Frame, body: Rect, from_file: &FromFile, width: usize) {
    let (row, input) = match &from_file.staged {
        None => (1, &from_file.path),
        Some(staged) => (from_file_lines_count(staged, width) - 1, &from_file.confirm),
    };
    if row < usize::from(body.height) {
        let line = Rect::new(body.x, body.y + row as u16, body.width, 1);
        crate::render::show_input(frame, line, "> ", input, false);
    }
}

/// The index's state over the list, when there is something to say.
fn notice(frame: &mut Frame, area: Rect, model: &Model) -> Rect {
    let snapshot = &model.plugins.snapshot;
    let (text, role) = match (&snapshot.index_error, &snapshot.offline) {
        (Some(error), _) => (error.clone(), Role::Error),
        (None, Some(offline)) => (offline.clone(), Role::Warning),
        _ if model.plugins.loading => ("Reading the plugin index…".to_string(), Role::Muted),
        _ => return area,
    };
    if area.height < 3 {
        return area;
    }
    frame.render_widget(
        Paragraph::new(crate::model::truncate_cell(&text, usize::from(area.width)))
            .style(model.theme.style(role, model.capabilities)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    Rect::new(area.x, area.y + 1, area.width, area.height - 1)
}

fn empty_lines(model: &Model) -> Vec<String> {
    let state = &model.plugins;
    if !state.search.input.is_empty() || state.filter != TypeFilter::All {
        return vec!["No plugin matches the search or the type.".into()];
    }
    match state.view {
        PluginsView::Installed => vec![
            "No plugins are installed.".into(),
            "Tab shows what the index offers: DuckDB, themes.".into(),
        ],
        PluginsView::Available if state.snapshot.index_error.is_some() => {
            vec!["The plugin index could not be had; Refresh tries again.".into()]
        }
        PluginsView::Available => vec!["Every plugin this Dexo runs is installed.".into()],
    }
}

/// `↑ DuckDB  driver  1.0.0  local`.
fn list_line(row: &PluginRow, width: usize, unicode: bool) -> String {
    let version = row
        .installed
        .as_ref()
        .map(|installed| installed.version.as_str())
        .or(row.newest.as_deref())
        .unwrap_or("");
    let local = if row
        .installed
        .as_ref()
        .is_some_and(|installed| installed.origin == Origin::Local)
    {
        "  local"
    } else {
        ""
    };
    format!(
        "{} {:<width$}  {:<6}  {version}{local}",
        row.mark(unicode),
        row.name,
        row.kind.as_str()
    )
}

fn detail_lines(model: &Model, row: &PluginRow, width: u16) -> Vec<Line<'static>> {
    let mut fields = vec![FieldRow::Field("kind", row.kind.as_str().to_string())];
    if let Some(installed) = &row.installed {
        let disabled = if installed.enabled { "" } else { ", disabled" };
        fields.push(FieldRow::Field(
            "installed",
            format!("{}{disabled}", installed.version),
        ));
    }
    if let Some(newest) = &row.newest {
        fields.push(FieldRow::Field("newest", newest.clone()));
    }
    if !row.authors.is_empty() {
        fields.push(FieldRow::Field("author", row.authors.join(", ")));
    }
    if !row.license.is_empty() {
        fields.push(FieldRow::Field("license", row.license.clone()));
    }
    if let Some(min_dexo) = &row.min_dexo {
        fields.push(FieldRow::Field(
            "requires",
            format!("Dexo {min_dexo} or newer"),
        ));
    }
    if let Some(protocol) = row.protocol {
        fields.push(FieldRow::Field("protocol", protocol.to_string()));
    }
    if let Some(size) = row.size {
        fields.push(FieldRow::Field(
            "download",
            crate::screens::transfer::size_of(size),
        ));
    }
    let origin = match row.installed.as_ref().map(|installed| installed.origin) {
        Some(Origin::Local) => "local, not verified",
        _ => "official, signed",
    };
    fields.push(FieldRow::Field("origin", origin.into()));
    if !row.provides.is_empty() {
        let label = if row.kind == Kind::Driver {
            "drivers"
        } else {
            "themes"
        };
        fields.push(FieldRow::Field(label, row.provides.join(", ")));
    }
    if !row.description.is_empty() {
        fields.push(FieldRow::Blank);
        fields.push(FieldRow::Text(row.description.clone()));
    }
    if !row.readme.is_empty() {
        fields.push(FieldRow::Blank);
        fields.extend(row.readme.iter().map(|line| FieldRow::Text(line.clone())));
    }
    widgets::field_lines(model, &fields, width)
}

fn removal_lines(row: &PluginRow) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw(format!("Remove {} and its files?", row.name))];
    if row.kind == Kind::Driver {
        lines.push(Line::raw(
            "Connections that use its driver stay saved, and say it is not installed.",
        ));
    }
    lines.push(Line::default());
    lines.push(Line::raw("Enter removes it, Esc keeps it."));
    lines
}

fn from_file_lines(model: &Model, from_file: &FromFile, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw("A plugin's .tar.gz, or its folder:"),
        Line::raw(format!("> {}", from_file.path.as_str())),
    ];
    if let Some(staged) = &from_file.staged {
        let warning = model.theme.style(Role::Warning, model.capabilities);
        lines.push(Line::default());
        for line in crate::model::wrap_words(&staged.trust, width.max(20)) {
            lines.push(Line::styled(line, warning));
        }
        lines.push(Line::default());
        lines.push(Line::raw(format!("Type {} to install it:", staged.id)));
        lines.push(Line::raw(format!("> {}", from_file.confirm.as_str())));
    }
    if let Some(error) = &from_file.error {
        lines.push(Line::default());
        lines.push(Line::styled(
            error.clone(),
            model.theme.style(Role::Error, model.capabilities),
        ));
    }
    lines
}

/// The rows the trust prompt takes up to its confirmation input, that one included.
fn from_file_lines_count(staged: &StagedInfo, width: usize) -> usize {
    2 + 1 + crate::model::wrap_words(&staged.trust, width.max(20)).len() + 1 + 2
}

/// Under the detail: the download under way, a bar with bytes and percent. The status
/// line says Esc cancels it.
fn footer(model: &Model) -> Vec<String> {
    let Some(busy) = &model.plugins.busy else {
        return Vec::new();
    };
    let (full, empty) = if model.capabilities.unicode {
        ('█', '░')
    } else {
        ('#', '-')
    };
    let line = match busy.total {
        Some(total) if total > 0 => {
            let percent = (busy.done * 100 / total).min(100);
            let filled = (percent / 10) as usize;
            format!(
                "{} {} [{}{}] {} of {} · {percent}%",
                busy.verb,
                busy.id,
                full.to_string().repeat(filled),
                empty.to_string().repeat(10 - filled),
                crate::screens::transfer::size_of(busy.done),
                crate::screens::transfer::size_of(total)
            )
        }
        _ => format!(
            "{} {}… {}",
            busy.verb,
            busy.id,
            crate::screens::transfer::size_of(busy.done)
        ),
    };
    vec![line]
}

/// What acts on the whole screen: the index fetched again, a package from a file.
pub fn list_buttons(model: &Model) -> Vec<Button> {
    let idle = model.plugins.busy.is_none();
    let wait = "Wait for the download to finish, or Esc to cancel it.";
    vec![
        Button::new(KeyCode::Char('r'), "Refresh").enabled_if(idle, wait),
        Button::new(KeyCode::Char('f'), "Install from file…").enabled_if(idle, wait),
    ]
}

/// What can be done to the pick.
pub fn buttons(model: &Model) -> Vec<Button> {
    let state = &model.plugins;
    if state.from_file.is_some() || state.removing.is_some() {
        return Vec::new();
    }
    let Some(row) = state.picked() else {
        return Vec::new();
    };
    let idle = state.busy.is_none();
    let wait = "Wait for the download to finish, or Esc to cancel it.";
    let Some(installed) = &row.installed else {
        return vec![Button::new(KeyCode::Char('i'), "Install").enabled_if(idle, wait)];
    };
    let mut buttons = Vec::new();
    if let Some(newest) = row.update() {
        buttons.push(
            Button::new(KeyCode::Char('u'), format!("Update to {newest}")).enabled_if(idle, wait),
        );
    }
    let toggle = if installed.enabled {
        "Disable"
    } else {
        "Enable"
    };
    buttons.push(Button::new(KeyCode::Char('e'), toggle).enabled_if(idle, wait));
    if row.kind == Kind::Theme && installed.enabled {
        buttons.push(Button::new(KeyCode::Char('a'), "Apply"));
    }
    buttons.push(Button::new(KeyCode::Char('x'), "Remove…").enabled_if(idle, wait));
    buttons
}

pub fn hints(model: &Model) -> String {
    let state = &model.plugins;
    if state.from_file.is_some() {
        return "Enter goes on  Esc cancels".into();
    }
    if state.removing.is_some() {
        return "Enter removes  Esc keeps it".into();
    }
    if state.busy.is_some() {
        return "Esc cancels the download".into();
    }
    "Up/Down pick  Tab view  / search  t type  Esc back".into()
}
