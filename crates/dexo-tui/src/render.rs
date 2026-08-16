use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::layout::LayoutPlan;
use crate::model::{Focus, Model};
use crate::mouse::{HitMap, HitTarget};
use crate::palette::{filter_entries, palette_entries, scroll_to_selection};

pub fn render(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    hits.clear();
    let plan = LayoutPlan::for_area_with(frame.area(), Some(&model.panes));
    render_bar(frame, plan.context, context_line(model));
    match plan.mode {
        crate::layout::LayoutMode::Compact => render_compact(frame, plan.content, model, hits),
        _ => {
            hits.register(HitTarget::Explorer, plan.explorer);
            render_panel(
                frame,
                plan.explorer,
                model,
                "Explorer",
                model.focus == Focus::Explorer,
                explorer_body(model, plan.explorer),
            );
            crate::widgets::tabs::render(frame, plan.tabs, model);
            hits.register(HitTarget::Editor, plan.content);
            if model.tabs.active == 2 {
                render_panel(
                    frame,
                    plan.content,
                    model,
                    "Schema",
                    model.focus == Focus::Editor,
                    crate::widgets::form::render_lines(&model.schema_editor).join("\n"),
                );
            } else if model.tabs.active == 4 {
                render_panel(
                    frame,
                    plan.content,
                    model,
                    "Explain",
                    model.focus == Focus::Editor,
                    model.explain.lines().join("\n"),
                );
            } else {
                crate::widgets::editor::render(frame, plan.content, model);
            }
            hits.register(HitTarget::Grid, plan.results);
            crate::widgets::grid::render(frame, plan.results, model, hits);
            render_panel(
                frame,
                plan.inspector,
                model,
                &inspector_title(model),
                model.focus == Focus::Inspector,
                inspector_body(model),
            );
        }
    }
    crate::widgets::status::render(frame, plan.status, model);
    if model.palette.open {
        render_palette(frame, model);
    }
    if model.help.open {
        render_help(frame, model);
    }
    if model.results_menu.open {
        render_results_menu(frame, model);
    }
    if let Some(review) = &model.data.review {
        render_review(frame, review);
    }
    if let Some(preview) = &model.schema_editor.preview {
        render_ddl_preview(frame, preview);
    }
    if model.schema_diff.open {
        let area = frame.area();
        if area.width >= 10 && area.height >= 5 {
            let popup = centered(area, 80, 18);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(model.schema_diff.lines().join("\n"))
                    .block(Block::bordered().title("Schema diff")),
                popup,
            );
        }
    }
    if model.transfer.open {
        let area = frame.area();
        if area.width >= 10 && area.height >= 5 {
            let popup = centered(area, 72, 16);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(model.transfer.lines().join("\n"))
                    .block(Block::bordered().title("Transfer")),
                popup,
            );
        }
    }
    if model.security.open {
        render_panel(
            frame,
            centered(frame.area(), 40, 12),
            model,
            "Security",
            true,
            model.security.lines().join("\n"),
        );
    }
    if model.admin.open {
        let area = frame.area();
        if area.width >= 10 && area.height >= 5 {
            let popup = centered(area, 80, 16);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(model.admin.lines().join("\n"))
                    .block(Block::bordered().title("Sessions")),
                popup,
            );
        }
    }
    if model.mcp_profiles.open {
        let area = frame.area();
        if area.width >= 10 && area.height >= 5 {
            let popup = centered(area, 72, 14);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(model.mcp_profiles.lines().join("\n"))
                    .block(Block::bordered().title("MCP profiles")),
                popup,
            );
        }
    }
    if model.connections.open {
        let popup = centered(frame.area(), 72, 18);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.connections.lines().join("\n"))
                .block(Block::bordered().title("Connections")),
            popup,
        );
    }
    if model.projects.open {
        let popup = centered(frame.area(), 72, 18);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.projects.lines().join("\n"))
                .block(Block::bordered().title("Projects")),
            popup,
        );
    }
    if model.config_transfer.open {
        let popup = centered(frame.area(), 72, 16);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.config_transfer.lines().join("\n"))
                .block(Block::bordered().title("Config transfer")),
            popup,
        );
    }
    if model.secret_prompt.open {
        let popup = centered(frame.area(), 56, 8);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.secret_prompt.lines().join("\n"))
                .block(Block::bordered().title("Secret")),
            popup,
        );
    }
    if model.connection_form.open {
        let popup = centered(frame.area(), 64, 16);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.connection_form.lines().join("\n"))
                .block(Block::bordered().title("Add connection")),
            popup,
        );
    }
    if model.settings.open {
        let popup = centered(frame.area(), 64, 12);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.settings.lines().join("\n"))
                .block(Block::bordered().title("Settings")),
            popup,
        );
    }
    if model.recovery.open {
        let popup = centered(frame.area(), 64, 12);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.recovery.lines().join("\n"))
                .block(Block::bordered().title("Session recovery")),
            popup,
        );
    }
    if model.mcp_audit.open {
        let popup = centered(frame.area(), 72, 12);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(model.mcp_audit.lines().join("\n"))
                .block(Block::bordered().title("MCP audit")),
            popup,
        );
    }
    if model.file_picker.open {
        render_file_picker(frame, model);
    }
    if model.editor.completion_open {
        render_completion(frame, model);
    }
    if model.editor.parameter_prompt {
        render_parameters(frame, model);
    }
    if model.editor.history_open {
        render_list_overlay(
            frame,
            "History",
            &model.editor.history,
            model.editor.history_selected,
            0,
        );
    }
    if model.editor.snippet_open {
        let names: Vec<String> = model
            .editor
            .snippets
            .iter()
            .map(|snippet| snippet.name.clone())
            .collect();
        render_list_overlay(frame, "Snippets", &names, model.editor.snippet_selected, 0);
    }
    if let Some(preview) = &model.diagnostic_preview {
        let popup = centered(frame.area(), 72, 16);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(preview.as_str()).block(Block::bordered().title("Diagnostics")),
            popup,
        );
    }
}

fn render_compact(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    match model.focus {
        Focus::Explorer => {
            hits.register(HitTarget::Explorer, area);
            render_panel(
                frame,
                area,
                model,
                "Explorer",
                true,
                explorer_body(model, area),
            );
        }
        Focus::Editor | Focus::Palette => {
            hits.register(HitTarget::Editor, area);
            if model.tabs.active == 2 {
                render_panel(
                    frame,
                    area,
                    model,
                    "Schema",
                    true,
                    crate::widgets::form::render_lines(&model.schema_editor).join("\n"),
                );
            } else if model.tabs.active == 4 {
                render_panel(
                    frame,
                    area,
                    model,
                    "Explain",
                    true,
                    model.explain.lines().join("\n"),
                );
            } else {
                crate::widgets::editor::render(frame, area, model);
            }
        }
        Focus::Results => {
            hits.register(HitTarget::Grid, area);
            crate::widgets::grid::render(frame, area, model, hits);
        }
        Focus::Inspector => render_panel(
            frame,
            area,
            model,
            &inspector_title(model),
            true,
            inspector_body(model),
        ),
    }
}

fn context_line(model: &Model) -> String {
    format!(
        "{}  {}  {}",
        model.project,
        if model.connection.name.is_empty() {
            "no connection"
        } else {
            &model.connection.name
        },
        if model.schema.is_empty() {
            "—"
        } else {
            &model.schema
        }
    )
}

fn inspector_title(model: &Model) -> String {
    format!("Inspector · {}", model.inspector.tab.label())
}

fn inspector_body(model: &Model) -> String {
    if model.inspector.open {
        let mut body = describe_object_inspector(&model.inspector);
        if !model.results.columns().is_empty() {
            body.push('\n');
            for column in model.results.columns() {
                let null = if column.nullable { "null" } else { "not null" };
                body.push_str(&format!("{} {} {null}\n", column.name, column.type_name));
            }
        }
        return body;
    }
    if let Some(view) = &model.data.viewer {
        return crate::widgets::viewer::describe(view);
    }
    if model.results.columns().is_empty() {
        "No selection".into()
    } else {
        model
            .results
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn describe_object_inspector(
    inspector: &crate::screens::object_inspector::ObjectInspector,
) -> String {
    let mut lines = Vec::new();
    if inspector.qualified_name.is_empty() {
        lines.push("loading…".into());
    } else {
        lines.push(inspector.qualified_name.clone());
    }
    if let Some(error) = &inspector.error {
        lines.push(format!("error: {error}"));
    }
    for restriction in &inspector.restrictions {
        lines.push(format!("restricted: {restriction}"));
    }
    if let Some(object) = &inspector.object {
        lines.push(format!("kind: {}", object.kind.as_str()));
    }
    match inspector.tab {
        crate::screens::object_inspector::InspectorTab::Ddl => {
            if let Some(ddl) = &inspector.ddl {
                lines.push(ddl.clone());
            }
        }
        crate::screens::object_inspector::InspectorTab::Dependencies => {
            if !inspector.dependencies.is_empty() {
                lines.push(format!(
                    "deps: {}",
                    inspector
                        .dependencies
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !inspector.dependents.is_empty() {
                lines.push(format!(
                    "dependents: {}",
                    inspector
                        .dependents
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        crate::screens::object_inspector::InspectorTab::Privileges => {
            if !inspector.effective_privileges.is_empty() {
                lines.push(format!(
                    "privileges: {}",
                    inspector.effective_privileges.join(", ")
                ));
            }
        }
        crate::screens::object_inspector::InspectorTab::Properties => {
            if let Some(ddl) = &inspector.ddl {
                lines.push(ddl.clone());
            }
            if !inspector.dependencies.is_empty() {
                lines.push(format!(
                    "deps: {}",
                    inspector
                        .dependencies
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !inspector.dependents.is_empty() {
                lines.push(format!(
                    "dependents: {}",
                    inspector
                        .dependents
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !inspector.effective_privileges.is_empty() {
                lines.push(format!(
                    "privileges: {}",
                    inspector.effective_privileges.join(", ")
                ));
            }
        }
    }
    lines.join("\n")
}

fn explorer_body(model: &Model, area: Rect) -> String {
    let rows = area.height.saturating_sub(2) as usize;
    crate::widgets::object_tree::render_visible(&model.explorer, Some(rows.max(1))).join("\n")
}

fn render_bar(frame: &mut Frame, area: Rect, text: String) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Paragraph::new(text), area);
}

fn render_panel(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    title: &str,
    focused: bool,
    body: String,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(body), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(body).block(pane_block(model, title, focused)),
        area,
    );
}

pub fn pane_title(title: &str, focused: bool, unicode: bool) -> String {
    let mark = if focused {
        if unicode { "▸ " } else { "> " }
    } else {
        "  "
    };
    format!("{mark}{title}")
}

pub fn pane_block(model: &Model, title: &str, focused: bool) -> Block<'static> {
    let caps = model.capabilities;
    Block::bordered()
        .title(Span::styled(
            pane_title(title, focused, caps.unicode),
            model.theme.pane_title(focused, caps),
        ))
        .border_style(model.theme.pane_border(focused, caps))
}

fn overlay_block(model: &Model, title: &str) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(
            title.to_string(),
            model.theme.overlay(model.capabilities),
        ))
        .border_style(model.theme.overlay(model.capabilities))
}

fn render_palette(frame: &mut Frame, model: &Model) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let width = area.width.clamp(10, 60);
    let height = area.height.clamp(5, 12);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    let popup = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup);
    let entries = palette_entries(model);
    let visible = filter_entries(&entries, &model.palette.query);
    let mut lines = vec![format!("> {}", model.palette.query)];
    let rows = height.saturating_sub(3) as usize;
    let offset = scroll_to_selection(
        model.palette.selected,
        model.palette.offset,
        visible.len(),
        rows,
    );
    for (index, entry) in visible.iter().enumerate().skip(offset).take(rows) {
        let marker = if index == model.palette.selected {
            ">"
        } else {
            " "
        };
        let disabled = entry
            .disabled_reason
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default();
        lines.push(format!("{marker} {}{disabled}", entry.title));
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(overlay_block(model, "Command Palette")),
        popup,
    );
}

fn render_help(frame: &mut Frame, model: &Model) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 76, area.height.saturating_sub(2).max(12));
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    for (section, rows) in model.keymap.help_sections() {
        lines.push(format!("[{section}]"));
        for (chord, command) in rows {
            let title = palette_entries(model)
                .iter()
                .find(|entry| entry.id == command)
                .map(|entry| entry.title)
                .unwrap_or(command.as_str());
            lines.push(format!("  {chord:<16} {title}"));
        }
        lines.push(String::new());
    }
    if lines.is_empty() {
        lines.push("no bindings".into());
    }
    let inner_h = popup.height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(inner_h.max(1));
    let scroll = (model.help.scroll as usize).min(max_scroll) as u16;
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .scroll((scroll, 0))
            .block(overlay_block(model, "Keybindings  Esc to close")),
        popup,
    );
}

fn render_results_menu(frame: &mut Frame, model: &Model) {
    let items = crate::palette::results_menu_items();
    let labels: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(index, (_, title))| {
            let marker = if index == model.results_menu.selected {
                ">"
            } else {
                " "
            };
            format!("{marker} {title}")
        })
        .collect();
    let popup = centered(frame.area(), 48, 14);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(labels.join("\n")).block(overlay_block(model, "Row actions  Esc to close")),
        popup,
    );
}

fn render_review(frame: &mut Frame, review: &crate::screens::data::ReviewModal) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 14);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(crate::screens::data::review_lines(review).join("\n"))
            .block(Block::bordered().title("Review changes")),
        popup,
    );
}

fn render_ddl_preview(frame: &mut Frame, preview: &crate::screens::schema_editor::DdlPreviewState) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 14);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(crate::modals::preview_lines(preview).join("\n"))
            .block(Block::bordered().title("DDL preview")),
        popup,
    );
}

fn centered(area: Rect, max_width: u16, max_height: u16) -> Rect {
    let width = area.width.clamp(10, max_width);
    let height = area.height.clamp(6, max_height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    Rect::new(x, y, width, height)
}

fn render_completion(frame: &mut Frame, model: &Model) {
    let labels: Vec<String> = model
        .editor
        .completions
        .iter()
        .map(|item| match &item.detail {
            Some(detail) => format!("{}  {detail}", item.label),
            None => item.label.clone(),
        })
        .collect();
    let popup = completion_popup_rect(frame.area(), model, &labels);
    if popup.width < 4 || popup.height < 2 {
        return;
    }
    frame.render_widget(Clear, popup);
    let rows = (popup.height.saturating_sub(2) as usize).max(1);
    let offset = scroll_to_selection(
        model.editor.completion_selected,
        model.editor.completion_offset,
        labels.len(),
        rows,
    );
    let mut lines = Vec::new();
    for (index, item) in labels.iter().enumerate().skip(offset).take(rows) {
        let marker = if index == model.editor.completion_selected {
            ">"
        } else {
            " "
        };
        lines.push(format!("{marker} {item}"));
    }
    if lines.is_empty() {
        lines.push("(empty)".into());
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::bordered()),
        popup,
    );
}

/// Vim/Neovim pum: align with the cursor, prefer below, flip above if it does not fit.
fn completion_popup_rect(area: Rect, model: &Model, items: &[String]) -> Rect {
    let plan = LayoutPlan::for_area_with(area, Some(&model.panes));
    let inner = Block::bordered().inner(plan.content);
    let doc = model.active_document();
    let (line, col) = crate::screens::editor::line_col_of(&doc.text(), doc.cursor());
    let gutter = 5u16;
    let cursor_x = inner
        .x
        .saturating_add(gutter)
        .saturating_add(col.saturating_sub(doc.viewport_column) as u16);
    let cursor_y = inner
        .y
        .saturating_add(line.saturating_sub(doc.viewport_line) as u16);
    let width = items
        .iter()
        .map(|item| item.chars().count().saturating_add(4))
        .max()
        .unwrap_or(16)
        .clamp(12, 42) as u16;
    let width = width.min(area.width.max(1));
    let height = (items.len().clamp(1, 8) as u16)
        .saturating_add(2)
        .min(area.height.max(1));
    let x = if cursor_x.saturating_add(width) > area.width {
        area.width.saturating_sub(width)
    } else {
        cursor_x
    };
    let below = area.height.saturating_sub(cursor_y.saturating_add(1));
    let y = if below >= height {
        cursor_y.saturating_add(1)
    } else {
        cursor_y.saturating_sub(height)
    };
    Rect::new(x, y, width, height)
}

fn render_parameters(frame: &mut Frame, model: &Model) {
    let name = model
        .editor
        .parameters
        .get(model.editor.parameter_index)
        .map(|parameter| parameter.name.as_str())
        .unwrap_or("param");
    let popup = centered(frame.area(), 48, 6);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!("{name} = {}", model.editor.parameter_draft))
            .block(Block::bordered().title("Parameters")),
        popup,
    );
}

fn render_list_overlay(
    frame: &mut Frame,
    title: &str,
    items: &[String],
    selected: usize,
    offset: usize,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 48, 12);
    frame.render_widget(Clear, popup);
    let rows = (popup.height.saturating_sub(2) as usize).max(1);
    let offset = scroll_to_selection(selected, offset, items.len(), rows);
    let mut lines = Vec::new();
    for (index, item) in items.iter().enumerate().skip(offset).take(rows) {
        let marker = if index == selected { ">" } else { " " };
        lines.push(format!("{marker} {item}"));
    }
    if lines.is_empty() {
        lines.push("(empty)".into());
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::bordered().title(title)),
        popup,
    );
}

fn render_file_picker(frame: &mut Frame, model: &Model) {
    let popup = centered(frame.area(), 64, 16);
    frame.render_widget(Clear, popup);
    let mut lines = vec![model.file_picker.cwd.display().to_string()];
    for (index, path) in model.file_picker.entries.iter().enumerate().take(12) {
        let marker = if index == model.file_picker.selected {
            ">"
        } else {
            " "
        };
        lines.push(format!("{marker} {}", path.display()));
    }
    if let Some(error) = &model.file_picker.error {
        lines.push(error.clone());
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::bordered().title("File")),
        popup,
    );
}

pub fn render_to_string(model: &Model, width: u16, height: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("test backend");
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| render(frame, model, &mut hits))
        .expect("render");
    buffer_view(terminal.backend().buffer())
}

fn buffer_view(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area();
    let mut out = String::new();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render_to_string;
    use crate::model::Model;

    #[test]
    fn compact_terminal_does_not_panic() {
        let _ = render_to_string(&Model::default(), 20, 8);
    }

    #[test]
    fn completion_popup_sits_under_cursor_not_centered() {
        let mut model = Model::default();
        model.set_sql("select ");
        model.width = 160;
        model.height = 50;
        let area = ratatui::layout::Rect::new(0, 0, 160, 50);
        let popup = super::completion_popup_rect(area, &model, &["select".into(), "from".into()]);
        let center_x = area.width / 2;
        assert!(
            popup.x < center_x.saturating_sub(10),
            "expected cursor-aligned popup, got {popup:?}"
        );
        assert!(
            popup.y > 2,
            "expected below the editor cursor, got {popup:?}"
        );
    }
}
