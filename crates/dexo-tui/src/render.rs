use ratatui::Frame;
use ratatui::layout::Rect;
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
                "Explorer",
                crate::widgets::object_tree::render_lines(&model.explorer).join("\n"),
            );
            crate::widgets::tabs::render(frame, plan.tabs, model);
            hits.register(HitTarget::Editor, plan.content);
            if model.tabs.active == 2 {
                render_panel(
                    frame,
                    plan.content,
                    "Schema",
                    crate::widgets::form::render_lines(&model.schema_editor).join("\n"),
                );
            } else if model.tabs.active == 4 {
                render_panel(
                    frame,
                    plan.content,
                    "Explain",
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
                &inspector_title(model),
                inspector_body(model),
            );
        }
    }
    crate::widgets::status::render(frame, plan.status, model);
    if model.palette.open {
        render_palette(frame, model);
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
            "Security",
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
        );
    }
    if model.editor.snippet_open {
        let names: Vec<String> = model
            .editor
            .snippets
            .iter()
            .map(|snippet| snippet.name.clone())
            .collect();
        render_list_overlay(frame, "Snippets", &names, model.editor.snippet_selected);
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
                "Explorer",
                crate::widgets::object_tree::render_lines(&model.explorer).join("\n"),
            );
        }
        Focus::Editor | Focus::Palette => {
            hits.register(HitTarget::Editor, area);
            if model.tabs.active == 2 {
                render_panel(
                    frame,
                    area,
                    "Schema",
                    crate::widgets::form::render_lines(&model.schema_editor).join("\n"),
                );
            } else if model.tabs.active == 4 {
                render_panel(frame, area, "Explain", model.explain.lines().join("\n"));
            } else {
                crate::widgets::editor::render(frame, area, model);
            }
        }
        Focus::Results => {
            hits.register(HitTarget::Grid, area);
            crate::widgets::grid::render(frame, area, model, hits);
        }
        Focus::Inspector => {
            render_panel(frame, area, &inspector_title(model), inspector_body(model))
        }
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
        return describe_object_inspector(&model.inspector);
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

fn render_bar(frame: &mut Frame, area: Rect, text: String) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Paragraph::new(text), area);
}

fn render_panel(frame: &mut Frame, area: Rect, title: &str, body: String) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(body), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(body).block(Block::bordered().title(title)),
        area,
    );
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
        Paragraph::new(lines.join("\n")).block(Block::bordered().title("Command Palette")),
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
        .map(|item| item.label.clone())
        .collect();
    render_list_overlay(
        frame,
        "Completions",
        &labels,
        model.editor.completion_selected,
    );
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

fn render_list_overlay(frame: &mut Frame, title: &str, items: &[String], selected: usize) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 48, 12);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    for (index, item) in items.iter().enumerate().take(10) {
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
}
