use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::layout::LayoutPlan;
use crate::model::{Focus, Model};
use crate::palette::{filter_entries, palette_entries, scroll_to_selection};

pub fn render(frame: &mut Frame, model: &Model) {
    let plan = LayoutPlan::for_area_with(frame.area(), Some(&model.panes));
    render_bar(frame, plan.context, context_line(model));
    match plan.mode {
        crate::layout::LayoutMode::Compact => render_compact(frame, plan.content, model),
        _ => {
            render_panel(
                frame,
                plan.explorer,
                "Explorer",
                crate::widgets::object_tree::render_lines(&model.explorer).join("\n"),
            );
            crate::widgets::tabs::render(frame, plan.tabs, model);
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
            crate::widgets::grid::render(frame, plan.results, model);
            render_panel(frame, plan.inspector, "Inspector", inspector_body(model));
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
}

fn render_compact(frame: &mut Frame, area: Rect, model: &Model) {
    match model.focus {
        Focus::Explorer => render_panel(
            frame,
            area,
            "Explorer",
            crate::widgets::object_tree::render_lines(&model.explorer).join("\n"),
        ),
        Focus::Editor | Focus::Palette => {
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
        Focus::Results => crate::widgets::grid::render(frame, area, model),
        Focus::Inspector => render_panel(frame, area, "Inspector", inspector_body(model)),
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

fn inspector_body(model: &Model) -> String {
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

pub fn render_to_string(model: &Model, width: u16, height: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("test backend");
    terminal.draw(|frame| render(frame, model)).expect("render");
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
