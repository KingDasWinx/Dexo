//! The full screens next to the workbench: what each draws, which keys it takes and what
//! its status line says. The state each one shows lives in `screens`, where the dialogs
//! they grew out of kept it.

pub mod agents;
pub mod compare;
pub mod connections;
pub mod history;
pub mod server;
pub mod widgets;

pub use widgets::Button;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

use crate::model::{Model, Screen};
use crate::mouse::{HitMap, HitTarget};
use crate::theme::Role;

/// The buttons over the shown screen's detail, for its picked item.
pub fn buttons(model: &Model) -> Vec<Button> {
    match model.shown_screen() {
        Screen::Connections if !model.connection_form.open => connections::buttons(model),
        Screen::History => history::buttons(model),
        Screen::Agents if model.agents_view == agents::AgentsView::Setup => {
            agents::setup_buttons(model)
        }
        Screen::Agents if model.agents_view == agents::AgentsView::Profiles => {
            agents::profile_buttons(model)
        }
        Screen::Agents if model.agents_view == agents::AgentsView::Approvals => {
            agents::approval_buttons(model)
        }
        Screen::Server => server::buttons(model),
        Screen::Compare => compare::buttons(model),
        _ => Vec::new(),
    }
}

/// The buttons that act on all the shown screen: over its list, or -- Compare's -- beside
/// its sources.
pub fn screen_buttons(model: &Model) -> Vec<Button> {
    match model.shown_screen() {
        Screen::Connections if !model.connection_form.open => connections::list_buttons(),
        Screen::History => history::list_buttons(model),
        Screen::Agents => agents::list_buttons(model),
        Screen::Server => server::list_buttons(model),
        Screen::Compare => compare::toolbar_buttons(model),
        _ => Vec::new(),
    }
}

/// The detail's focused button: one only while the detail has the keys.
pub fn button_focus(model: &Model) -> Option<usize> {
    let count = buttons(model).len();
    // A form's keys are its own: Left and Right change its values.
    (count > 0
        && section(model) == Section::Detail
        && held(model).is_none()
        && !detail_is_form(model))
    .then(|| model.screen_button.min(count - 1))
}

/// Commands a key may run while a screen other than the workbench is up. The rest act on
/// the documents, the editor or the panes, none of which is on screen: Ctrl+W closed a
/// document nobody could see.
pub fn screen_safe(command: &str) -> bool {
    command == "workbench.quit"
        || [
            "screen.",
            "palette.",
            "help.",
            "settings.",
            "mcp.",
            "project.",
            "recovery.",
            "diagnostics.",
            "config.",
        ]
        .iter()
        .any(|prefix| command.starts_with(prefix))
}

/// Where a screen's keys go: its list, or the detail of the pick beside it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Section {
    #[default]
    List,
    Detail,
}

/// A form or a question open in one section keeps the keys there until it is closed,
/// and what says how.
pub fn held(model: &Model) -> Option<(Section, &'static str)> {
    use agents::AgentsView;
    use history::HistoryView;
    let question = "Answer the question first, or Esc.";
    let form = model.shown_screen() == Screen::Connections && model.connection_form.open;
    if !form && search(model).is_some_and(|search| search.typing) {
        return Some((Section::List, "Enter keeps the search, Esc clears it."));
    }
    match model.shown_screen() {
        Screen::Connections if model.connection_form.open => {
            Some((Section::Detail, "The form has the keys: Esc closes it."))
        }
        Screen::Agents => match model.agents_view {
            AgentsView::Approvals if model.mcp_audit.deciding.is_some() => {
                Some((Section::Detail, question))
            }
            AgentsView::Profiles if model.mcp_profiles.grant_form.is_some() => Some((
                Section::Detail,
                "The grant form has the keys: Esc closes it.",
            )),
            AgentsView::Profiles if model.mcp_profiles.confirm.is_some() => {
                Some((Section::Detail, question))
            }
            AgentsView::Profiles if model.mcp_profiles.checklist.is_some() => Some((
                Section::Detail,
                "Space checks, Enter saves, Esc keeps them as they were.",
            )),
            _ => None,
        },
        Screen::Server if model.admin.terminate.is_some() || model.admin.cancel.is_some() => {
            Some((Section::Detail, question))
        }
        Screen::History if model.history_view == HistoryView::Saved => {
            if model.saved_queries.deleting.is_some() {
                Some((Section::Detail, question))
            } else if model.saved_queries.renaming.is_some() {
                Some((Section::List, "Enter renames, Esc keeps the old name."))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The shown screen's search, where it has one.
pub fn search(model: &Model) -> Option<&widgets::Search> {
    match model.shown_screen() {
        Screen::Connections => Some(&model.connections.search),
        Screen::History => Some(match model.history_view {
            history::HistoryView::History => &model.editor.history_search,
            history::HistoryView::Saved => &model.saved_queries.search,
        }),
        Screen::Agents if model.agents_view == agents::AgentsView::Activity => {
            Some(&model.mcp_audit.search)
        }
        Screen::Server => Some(&model.admin.search),
        Screen::Compare if !model.schema_diff.source_prompt => Some(&model.schema_diff.search),
        _ => None,
    }
}

/// Whether a field has the keys on the shown screen, being typed in: a letter there is
/// text, never a button's key, and a paste goes into it.
pub fn takes_text(model: &Model) -> bool {
    use crate::widgets::form::FooterFocus;
    if search(model).is_some_and(|search| search.typing) {
        return true;
    }
    match model.shown_screen() {
        Screen::Connections => {
            let form = &model.connection_form;
            form.open && form.focus < form.fields.len() && !form.on_choice()
        }
        Screen::Agents => {
            (model.agents_view == agents::AgentsView::Setup
                && section(model) == Section::Detail
                && model.mcp_setup.focused() == Some(crate::screens::mcp_setup::Row::Name))
                || model.mcp_profiles.grant_form.is_some()
        }
        Screen::Compare => {
            let diff = &model.schema_diff;
            diff.source_prompt
                && diff.uses_file()
                && diff.row == 2
                && diff.footer == FooterFocus::Input
        }
        Screen::Server => model.admin.terminate.is_some(),
        Screen::History => model.saved_queries.renaming.is_some(),
        Screen::Workbench => false,
    }
}

/// Ends the typing into the shown screen's search, keeping what was typed: a click
/// elsewhere on the screen is done with it.
pub fn stop_typing(model: &mut Model) {
    match model.shown_screen() {
        Screen::Connections => model.connections.search.typing = false,
        Screen::History => {
            model.editor.history_search.typing = false;
            model.saved_queries.search.typing = false;
        }
        Screen::Agents => model.mcp_audit.search.typing = false,
        Screen::Server => model.admin.search.typing = false,
        Screen::Compare => model.schema_diff.search.typing = false,
        _ => {}
    }
}

/// Whether the detail is a form, whose keys are its own rather than reading it: Agents'
/// Setup.
pub fn detail_is_form(model: &Model) -> bool {
    model.shown_screen() == Screen::Agents && model.agents_view == agents::AgentsView::Setup
}

/// How many sections the screen drew: its list and its detail, one alone, or none on an
/// empty screen. Folded, it has both, one at a time.
pub fn section_count(model: &Model) -> usize {
    if model.hits.folded() {
        return 2;
    }
    usize::from(model.hits.has(HitTarget::ScreenList))
        + usize::from(model.hits.has(HitTarget::ScreenDetail))
}

/// The section the keys go to now.
pub fn section(model: &Model) -> Section {
    if let Some((section, _)) = held(model) {
        return section;
    }
    // Compare's sources are its one section.
    if model.shown_screen() == Screen::Compare && model.schema_diff.source_prompt {
        return Section::List;
    }
    model.sections[model.shown_screen().index()]
}

/// How far the shown detail is read. Most screens keep it beside their pick; the rest
/// share `detail_scroll`.
pub fn detail_scroll(model: &Model) -> u16 {
    use agents::AgentsView;
    match (model.shown_screen(), model.agents_view) {
        (Screen::Server, _) => model.admin.detail_scroll,
        (Screen::Compare, _) => model.schema_diff.scroll,
        (Screen::Agents, AgentsView::Approvals) => model.mcp_audit.scroll,
        (Screen::Agents, AgentsView::Profiles) => {
            u16::try_from(model.mcp_profiles.detail_scroll).unwrap_or(u16::MAX)
        }
        _ => model.detail_scroll,
    }
}

pub fn set_detail_scroll(model: &mut Model, scroll: u16) {
    use agents::AgentsView;
    match (model.shown_screen(), model.agents_view) {
        (Screen::Server, _) => model.admin.detail_scroll = scroll,
        (Screen::Compare, _) => model.schema_diff.scroll = scroll,
        (Screen::Agents, AgentsView::Approvals) => model.mcp_audit.scroll = scroll,
        (Screen::Agents, AgentsView::Profiles) => {
            model.mcp_profiles.detail_scroll = usize::from(scroll);
        }
        _ => model.detail_scroll = scroll,
    }
}

/// One name on the header's strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StripItem {
    pub screen: Screen,
    pub label: String,
    pub current: bool,
    pub waiting: usize,
}

/// Work waiting on a screen, said beside its name.
pub fn waiting(model: &Model, screen: Screen) -> usize {
    match screen {
        Screen::Agents => model.mcp_audit.announced.len(),
        _ => 0,
    }
}

/// The strip for `room` cells: every screen when they fit, else the current one and the
/// ones with work waiting, else the current one alone. The current one is bracketed, so
/// it reads without colour too.
pub fn strip(model: &Model, room: usize) -> Vec<StripItem> {
    let all: Vec<StripItem> = Screen::ALL
        .into_iter()
        .map(|screen| {
            let waiting = waiting(model, screen);
            let name = if waiting > 0 {
                format!("{} {waiting}", screen.title())
            } else {
                screen.title().to_string()
            };
            let current = model.screen == screen;
            StripItem {
                screen,
                label: if current {
                    format!("[{name}]")
                } else {
                    format!(" {name} ")
                },
                current,
                waiting,
            }
        })
        .collect();
    let width = |items: &[StripItem]| -> usize {
        items.iter().map(|item| item.label.chars().count()).sum()
    };
    if width(&all) <= room {
        return all;
    }
    let busy: Vec<StripItem> = all
        .iter()
        .filter(|item| item.current || item.waiting > 0)
        .cloned()
        .collect();
    if width(&busy) <= room {
        return busy;
    }
    all.into_iter().filter(|item| item.current).collect()
}

/// Draws the current screen in `area`, everything between the header and the status line.
pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    match model.shown_screen() {
        Screen::Agents => agents::render(frame, area, model, hits),
        Screen::Connections => connections::render(frame, area, model, hits),
        Screen::Compare => compare::render(frame, area, model, hits),
        Screen::History => history::render(frame, area, model, hits),
        Screen::Server => server::render(frame, area, model, hits),
        Screen::Workbench => {}
    }
}

/// What the status line says on the current screen, after its name.
/// `hits` is the frame being drawn: what it shows of the screen, not the last frame's.
pub fn hints(model: &Model, hits: &HitMap) -> String {
    let hints = match model.shown_screen() {
        Screen::Agents => agents::hints(model),
        Screen::Connections => connections::hints(model),
        Screen::Compare => compare::hints(model),
        Screen::History => history::hints(model),
        Screen::Server => server::hints(model),
        _ => "Esc back".into(),
    };
    let free = held(model).is_none() && !detail_is_form(model);
    // A button for all the screen that is not drawn -- no room, or its list is folded
    // away -- is said here, first: the line is cut at its end.
    let unseen: Vec<String> = screen_buttons(model)
        .iter()
        .filter(|button| free && !hits.has(HitTarget::Press(button.key, button.shift)))
        .map(|button| {
            format!(
                "{} {}",
                widgets::key_label(button.key),
                button.label.trim_end_matches('…').to_lowercase()
            )
        })
        .collect();
    let hints = if unseen.is_empty() {
        hints
    } else {
        format!("{}  {hints}", unseen.join("  "))
    };
    // Folded to one column, Enter shows the pick's detail and Esc goes back to the list.
    let folded = hits.folded() && free;
    // On the detail the arrows read it; the letters still act on the pick.
    if section(model) == Section::Detail && free {
        let hints = match hints.find("Up/Down pick") {
            Some(_) => hints.replacen("Up/Down pick", "Up/Down read", 1),
            None => format!("Up/Down read  {hints}"),
        };
        // Folded, the way back is said first: the line is short there.
        if folded {
            let rest = hints.replace("  Esc back", "").replace("Esc back", "");
            format!("Esc the list  {rest}")
        } else {
            hints
        }
    } else if folded {
        // Enter shows the detail here; what it does there is said there.
        let rest: Vec<&str> = hints
            .split("  ")
            .filter(|hint| !hint.starts_with("Enter "))
            .collect();
        format!("Enter details  {}", rest.join("  "))
    } else {
        hints
    }
}

/// A screen's views on one row, the current one bracketed so it reads without colour,
/// each answering a click. Returns the row below it.
pub fn views_bar(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    views: &[(String, bool)],
) -> Rect {
    let row = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    let current = model.theme.style(Role::Focus, model.capabilities);
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let mut spans = Vec::new();
    let mut x = row.x;
    for (index, (label, active)) in views.iter().enumerate() {
        let text = if *active {
            format!("[{} {label}]", index + 1)
        } else {
            format!(" {} {label} ", index + 1)
        };
        let width = text.chars().count() as u16;
        if x < row.right() {
            hits.register(
                HitTarget::ScreenView(index),
                Rect::new(x, row.y, width.min(row.right() - x), 1),
            );
        }
        x = x.saturating_add(width + 1);
        spans.push(Span::styled(text, if *active { current } else { muted }));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
    Rect::new(
        area.x,
        area.y + row.height,
        area.width,
        area.height.saturating_sub(row.height),
    )
}

/// Under this many columns a screen shows its list or its detail, not both.
pub const FOLD_WIDTH: u16 = 80;

/// A list beside its detail on a wide screen, above it on a narrower one; under
/// [`FOLD_WIDTH`] one of them, the one with the keys, and the other an empty rect.
pub fn list_and_detail(
    model: &Model,
    hits: &mut HitMap,
    area: Rect,
    list_rows: usize,
) -> (Rect, Rect) {
    if area.width < FOLD_WIDTH {
        hits.fold();
        let none = Rect::new(area.x, area.y, 0, 0);
        return if section(model) == Section::Detail {
            (none, area)
        } else {
            (area, none)
        };
    }
    if area.width >= 100 {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
                .areas(area);
        (list, detail)
    } else {
        // Its rows and borders, with room for its buttons and its header, at most half
        // the height, at least three rows of list.
        let wanted = (list_rows as u16 + 5).clamp(5, (area.height / 2).max(5));
        let [list, detail] =
            Layout::vertical([Constraint::Length(wanted), Constraint::Min(0)]).areas(area);
        (list, detail)
    }
}

/// A table over its detail, `detail_rows` tall; under [`FOLD_WIDTH`] one of them, the
/// one with the keys, and the other an empty rect -- as [`list_and_detail`] folds.
pub fn table_and_detail(
    model: &Model,
    hits: &mut HitMap,
    area: Rect,
    detail_rows: u16,
) -> (Rect, Rect) {
    if area.width < FOLD_WIDTH {
        hits.fold();
        let none = Rect::new(area.x, area.y, 0, 0);
        return if section(model) == Section::Detail {
            (none, area)
        } else {
            (area, none)
        };
    }
    let table = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(detail_rows),
    );
    let detail = Rect::new(area.x, table.bottom(), area.width, detail_rows);
    (table, detail)
}

/// A bordered list with the picked row reversed and kept in sight, under an optional
/// pinned header of column names; every row drawn answers a click as `ListRow(index)`.
#[allow(clippy::too_many_arguments)]
pub fn list_pane(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    title: &str,
    header: Option<&str>,
    rows: &[String],
    picked: Option<usize>,
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let block = crate::render::pane_block(model, title, section(model) == Section::List);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenList, area);
    inner = widgets::list_actions(frame, inner, model, hits, &screen_buttons(model));
    let width = usize::from(inner.width);
    if let Some(header) = header
        && inner.height > 1
    {
        let muted = model
            .theme
            .style(Role::Muted, model.capabilities)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(
            Paragraph::new(crate::model::truncate_cell(&format!("  {header}"), width)).style(muted),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    }
    let visible = usize::from(inner.height);
    let offset = crate::palette::scroll_to_selection(picked.unwrap_or(0), 0, rows.len(), visible);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(index, row)| {
            // The pick is marked as well as reversed, so it reads without colour.
            let marker = if Some(index) == picked { "> " } else { "  " };
            let text = crate::model::truncate_cell(&format!("{marker}{row}"), width);
            if Some(index) == picked {
                Line::styled(
                    format!("{text:<width$}"),
                    Style::default().add_modifier(Modifier::REVERSED),
                )
            } else {
                Line::raw(text)
            }
        })
        .collect();
    for (line, index) in (offset..rows.len()).take(visible).enumerate() {
        hits.register(
            HitTarget::ListRow(index),
            crate::mouse::line_rect(inner, line),
        );
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Where [`detail_pane`] drew: its text and its footer, how far the text scrolls, and how
/// many of its lines show at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Drawn {
    pub body: Rect,
    pub footer: Rect,
    pub max_scroll: usize,
    pub page: u16,
}

/// A bordered pane of `text`, scrolled `scroll` lines, the screen's buttons for the picked
/// item on its first rows -- the actions were a run of text at the bottom of the pane --
/// and `footer` pinned to its bottom rows.
#[allow(clippy::too_many_arguments)]
pub fn detail_pane(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    title: &str,
    buttons: &[Button],
    text: Text<'_>,
    scroll: usize,
    footer: &[String],
) -> Drawn {
    if area.width < 2 || area.height < 2 {
        return Drawn::default();
    }
    let block: Block = crate::render::pane_block(model, title, section(model) == Section::Detail);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    hits.register(HitTarget::ScreenDetail, area);
    if !buttons.is_empty() {
        let focused = button_focus(model);
        let used = widgets::action_bar(frame, inner, model, hits, buttons, focused);
        // A blank row under the buttons, when there is room for it.
        let gap = u16::from(inner.height > used + 3);
        inner = Rect::new(
            inner.x,
            inner.y + used + gap,
            inner.width,
            inner.height.saturating_sub(used + gap),
        );
    }
    let footer_rows = (footer.len() as u16).min(inner.height);
    let body = Rect::new(inner.x, inner.y, inner.width, inner.height - footer_rows);
    let max_scroll = text.lines.len().saturating_sub(usize::from(body.height));
    hits.set_scroll_limit(crate::mouse::ScrollArea::ScreenDetail, max_scroll);
    hits.set_page(crate::mouse::ScrollArea::ScreenDetail, body.height);
    let top = scroll.min(max_scroll);
    frame.render_widget(
        Paragraph::new(text).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        body,
    );
    let footer_area = Rect::new(inner.x, body.bottom(), inner.width, footer_rows);
    frame.render_widget(Paragraph::new(footer.join("\n")), footer_area);
    Drawn {
        body,
        footer: footer_area,
        max_scroll,
        page: body.height,
    }
}

/// A screen's views on the left of its toolbar's row, and the toolbar -- search, filters,
/// the buttons that act on all of it -- in the rest. Returns what is under the row.
#[allow(clippy::too_many_arguments)]
pub fn views_and_toolbar(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    hits: &mut HitMap,
    views: &[(String, bool)],
    search: Option<&widgets::Search>,
    chips: &[widgets::Chip],
    buttons: &[Button],
) -> Rect {
    let views_width = views
        .iter()
        .map(|(label, _)| label.chars().count() as u16 + 5)
        .sum::<u16>()
        .min(area.width);
    views_bar(
        frame,
        Rect::new(area.x, area.y, views_width, area.height),
        model,
        hits,
        views,
    );
    // Too little left beside the views for a search box: the keys say the rest.
    if area.width - views_width >= 16 {
        widgets::toolbar(
            frame,
            Rect::new(
                area.x + views_width,
                area.y,
                area.width - views_width,
                area.height,
            ),
            model,
            hits,
            search,
            chips,
            buttons,
        );
    }
    let row = 1.min(area.height);
    Rect::new(area.x, area.y + row, area.width, area.height - row)
}

/// Sentences shown in the middle of an empty screen.
pub fn empty_state(frame: &mut Frame, area: Rect, model: &Model, lines: &[String]) {
    let width = usize::from(area.width.saturating_sub(4)).max(8);
    let wrapped: Vec<String> = lines
        .iter()
        .flat_map(|line| crate::model::wrap_words(line, width))
        .collect();
    let top = area.y + area.height.saturating_sub(wrapped.len() as u16) / 3;
    let muted = model.theme.style(Role::Muted, model.capabilities);
    frame.render_widget(
        Paragraph::new(wrapped.join("\n"))
            .alignment(Alignment::Center)
            .style(muted),
        Rect::new(area.x, top, area.width, area.bottom().saturating_sub(top)),
    );
}
