use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::layout::LayoutPlan;
use crate::model::{Focus, Model};
use crate::mouse::{
    HitButton, HitMap, HitTarget, overlay_blocks_workbench, popup_inner, register_label,
    register_line, register_overlay,
};
use crate::palette::{filter_entries, palette_entries, scroll_to_selection};

pub fn render(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    hits.clear();
    let plan = LayoutPlan::for_area_with_document_tabs(
        frame.area(),
        Some(&model.panes),
        model.tabs.active == 0,
    );
    render_bar(frame, plan.context, context_line(model));
    match plan.mode {
        crate::layout::LayoutMode::Compact => {
            if model.tabs.active == 0 {
                crate::widgets::document_tabs::render(frame, plan.document_tabs, model, hits);
            }
            render_compact(frame, plan.content, model, hits);
        }
        _ => {
            if !overlay_blocks_workbench(model) {
                hits.register(HitTarget::Explorer, plan.explorer);
                register_explorer_nodes(hits, plan.explorer, model);
            }
            render_panel(
                frame,
                plan.explorer,
                model,
                "Sidebar",
                model.focus == Focus::Explorer,
                explorer_body(model, plan.explorer),
            );
            crate::widgets::tabs::render(frame, plan.tabs, model, hits);
            if model.tabs.active == 0 {
                crate::widgets::document_tabs::render(frame, plan.document_tabs, model, hits);
            }
            if !overlay_blocks_workbench(model) {
                hits.register(HitTarget::Editor, plan.content);
            }
            render_editor_content(frame, plan.content, model, hits);
            if !overlay_blocks_workbench(model) {
                hits.register(HitTarget::Grid, plan.results);
                hits.register(HitTarget::Inspector, plan.inspector);
            }
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
        render_palette(frame, model, hits);
    }
    if model.help.open {
        render_help(frame, model, hits);
    }
    if model.results_menu.open {
        render_results_menu(frame, model, hits);
    }
    if let Some(review) = &model.data.review {
        render_review(frame, review, hits);
    }
    if let Some(preview) = &model.schema_editor.preview {
        render_ddl_preview(frame, preview, hits);
    }
    if model.schema_diff.open {
        render_schema_diff(frame, model, hits);
    }
    if model.transfer.open {
        render_transfer(frame, model, hits);
    }
    if model.security.open {
        render_security(frame, model, hits);
    }
    if model.admin.open {
        render_admin(frame, model, hits);
    }
    if model.mcp_profiles.open {
        render_mcp_profiles(frame, model, hits);
    }
    if model.connections.open {
        render_connections(frame, model, hits);
    }
    if model.projects.open {
        render_projects(frame, model, hits);
    }
    if model.config_transfer.open {
        render_config_transfer(frame, model, hits);
    }
    if model.secret_prompt.open {
        render_secret(frame, model, hits);
    }
    if model.transaction_prompt.open {
        render_transaction_prompt(frame, model, hits);
    }
    if model.data.query_prompt.open {
        render_data_query(frame, model, hits);
    }
    if model.connection_form.open {
        render_connection_form(frame, model, hits);
    }
    if model.settings.open {
        render_settings(frame, model, hits);
    }
    if model.recovery.open {
        render_recovery(frame, model, hits);
    }
    if model.diagnostics.open {
        render_diagnostics(frame, model, hits);
    }
    if model.mcp_audit.open {
        render_mcp_audit(frame, model, hits);
    }
    if model.file_picker.open {
        render_file_picker(frame, model, hits);
    }
    if model.editor.completion_open {
        render_completion(frame, model, hits);
    }
    if model.editor.parameter_prompt {
        render_parameters(frame, model, hits);
    }
    if model.editor.history_open {
        render_history(frame, model, hits);
    }
    if model.editor.snippet_open {
        render_snippets(frame, model, hits);
    }
}

fn render_compact(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let interactive = !overlay_blocks_workbench(model);
    match model.focus {
        Focus::Explorer => {
            if interactive {
                hits.register(HitTarget::Explorer, area);
                register_explorer_nodes(hits, area, model);
            }
            render_panel(
                frame,
                area,
                model,
                "Sidebar",
                true,
                explorer_body(model, area),
            );
        }
        Focus::Editor | Focus::Palette => {
            if interactive {
                hits.register(HitTarget::Editor, area);
            }
            render_editor_content(frame, area, model, hits);
        }
        Focus::Results => {
            if interactive {
                hits.register(HitTarget::Grid, area);
            }
            crate::widgets::grid::render(frame, area, model, hits);
        }
        Focus::Inspector => {
            if interactive {
                hits.register(HitTarget::Inspector, area);
            }
            render_panel(
                frame,
                area,
                model,
                &inspector_title(model),
                true,
                inspector_body(model),
            );
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

fn render_editor_content(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if model.tabs.active == 0 {
        crate::widgets::editor::render(frame, area, model);
        return;
    }
    let (title, body) = editor_tab_view(model);
    if model.tabs.active == 2 && model.inspector.ddl.is_none() && !overlay_blocks_workbench(model) {
        register_form_fields(
            hits,
            area,
            &crate::widgets::form::render_lines(&model.schema_editor),
        );
    }
    render_panel_scrolled(
        frame,
        area,
        model,
        title,
        model.focus == Focus::Editor,
        body,
        model.tabs.scroll,
    );
}

fn editor_tab_view(model: &Model) -> (&'static str, String) {
    match model.tabs.active {
        1 => ("Data", data_tab_body(model)),
        2 => ("DDL", ddl_tab_body(model)),
        3 => ("Properties", properties_tab_body(model)),
        4 => ("Explain", model.explain.lines().join("\n")),
        _ => ("Data", data_tab_body(model)),
    }
}

fn data_tab_body(model: &Model) -> String {
    let mut lines = Vec::new();
    let target = model.data.target.display_unquoted();
    if !target.is_empty() && target != "tbl" {
        lines.push(format!("table: {target}"));
    }
    if let Some(filter) = &model.data.filter {
        lines.push(format!("filter: {filter:?}"));
    }
    lines.push(format!(
        "rows: {}  page: {}  limit: {}",
        model.results.row_count(),
        model.data.page_offset,
        model.data.page_limit
    ));
    if model.results.columns().is_empty() {
        lines.push("Open a table or run a query. Rows stay in Results.".into());
    } else {
        lines.push("columns:".into());
        for column in model.results.columns() {
            let null = if column.nullable { "null" } else { "not null" };
            lines.push(format!("  {} {} {null}", column.name, column.type_name));
        }
    }
    lines.join("\n")
}

fn ddl_tab_body(model: &Model) -> String {
    if let Some(ddl) = &model.inspector.ddl {
        let name = if model.inspector.qualified_name.is_empty() {
            "DDL"
        } else {
            model.inspector.qualified_name.as_str()
        };
        format!("{name}\n\n{ddl}")
    } else {
        crate::widgets::form::render_lines(&model.schema_editor).join("\n")
    }
}

fn properties_tab_body(model: &Model) -> String {
    if model.inspector.qualified_name.is_empty() && model.inspector.object.is_none() {
        return "Select an object in Explorer.".into();
    }
    let mut lines = Vec::new();
    if !model.inspector.qualified_name.is_empty() {
        lines.push(model.inspector.qualified_name.clone());
    }
    if let Some(error) = &model.inspector.error {
        lines.push(format!("error: {error}"));
    }
    for restriction in &model.inspector.restrictions {
        lines.push(format!("restricted: {restriction}"));
    }
    if let Some(object) = &model.inspector.object {
        lines.push(format!("kind: {}", object.kind.as_str()));
    }
    if !model.inspector.dependencies.is_empty() {
        lines.push(format!(
            "deps: {}",
            model
                .inspector
                .dependencies
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !model.inspector.dependents.is_empty() {
        lines.push(format!(
            "dependents: {}",
            model
                .inspector
                .dependents
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !model.inspector.effective_privileges.is_empty() {
        lines.push(format!(
            "privileges: {}",
            model.inspector.effective_privileges.join(", ")
        ));
    }
    if !model.results.columns().is_empty() {
        lines.push("columns:".into());
        for column in model.results.columns() {
            let null = if column.nullable { "null" } else { "not null" };
            lines.push(format!("  {} {} {null}", column.name, column.type_name));
        }
    }
    lines.join("\n")
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
    crate::widgets::object_tree::render_sidebar(
        &model.explorer,
        &model.connections.profiles,
        &model.connection.name,
        model.capabilities.unicode,
        rows.max(1),
    )
    .join("\n")
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
    render_panel_scrolled(frame, area, model, title, focused, body, 0);
}

fn render_panel_scrolled(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    title: &str,
    focused: bool,
    body: String,
    scroll: u16,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width < 2 || area.height < 2 {
        frame.render_widget(Paragraph::new(body).scroll((scroll, 0)), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(body)
            .scroll((scroll, 0))
            .block(pane_block(model, title, focused)),
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

fn register_explorer_nodes(hits: &mut HitMap, area: Rect, model: &Model) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let inner = Block::bordered().inner(area);
    let sidebar_rows = 2 + model.connections.profiles.len();
    for (index, _) in model.connections.profiles.iter().enumerate() {
        register_line(hits, inner, 1 + index, HitTarget::SidebarConnection(index));
    }
    let catalog_rows = inner.height.saturating_sub(sidebar_rows as u16).max(1) as usize;
    let chrome = crate::widgets::object_tree::chrome_count(&model.explorer);
    let (offset, ids) =
        crate::widgets::object_tree::windowed_ids(&model.explorer, catalog_rows);
    for (i, _) in ids.iter().enumerate() {
        let row = sidebar_rows + chrome + i;
        register_line(
            hits,
            inner,
            row,
            HitTarget::ExplorerNode(offset.saturating_add(i)),
        );
    }
}

fn register_form_fields(hits: &mut HitMap, area: Rect, lines: &[String]) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let inner = Block::bordered().inner(area);
    let mut field = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if line.contains(": ") && (line.starts_with('>') || line.starts_with(' ')) {
            register_line(hits, inner, i, HitTarget::FormField(field));
            field += 1;
        }
    }
}

fn paint_popup(frame: &mut Frame, popup: Rect, block: Block<'static>, body: String) {
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(body).block(block), popup);
}

fn for_popup_lines(popup: Rect, lines: &[String], mut map: impl FnMut(usize, &str, Rect)) {
    let inner = popup_inner(popup);
    for (i, line) in lines.iter().enumerate() {
        if (i as u16) >= inner.height {
            break;
        }
        map(i, line, crate::mouse::line_rect(inner, i));
    }
}

fn render_palette(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let width = area.width.clamp(10, 60);
    let height = area.height.clamp(5, 12);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    let popup = Rect::new(x, y, width, height);
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
        let shortcut = entry
            .shortcut
            .map(|value| format!(" [{value}]"))
            .unwrap_or_default();
        let disabled = entry
            .disabled_reason
            .as_deref()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default();
        lines.push(format!("{marker} {}{shortcut}{disabled}", entry.title));
    }
    paint_popup(
        frame,
        popup,
        overlay_block(model, "Command Palette"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, _, rect| {
        if i == 0 {
            return;
        }
        hits.register(HitTarget::ListRow(offset.saturating_add(i - 1)), rect);
    });
}

fn render_help(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
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
    register_overlay(hits, popup);
    register_label(
        hits,
        crate::mouse::line_rect(popup_inner(popup), 0),
        "Esc to close",
        "Esc to close",
        HitTarget::Button(HitButton::Close),
    );
}

fn render_results_menu(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
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
    paint_popup(
        frame,
        popup,
        overlay_block(model, "Row actions  Esc to close"),
        labels.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &labels, |i, _, rect| {
        hits.register(HitTarget::ListRow(i), rect);
    });
}

fn render_review(frame: &mut Frame, review: &crate::screens::data::ReviewModal, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 14);
    let lines = crate::screens::data::review_lines(review);
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Review changes"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.contains("confirm production") {
            hits.register(HitTarget::Button(HitButton::ConfirmProduction), rect);
        } else if line == "ready" {
            hits.register(HitTarget::Button(HitButton::Apply), rect);
        }
    });
}

fn render_ddl_preview(
    frame: &mut Frame,
    preview: &crate::screens::schema_editor::DdlPreviewState,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 14);
    let lines = crate::modals::preview_lines(preview);
    paint_popup(
        frame,
        popup,
        Block::bordered().title("DDL preview"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line == "ready" {
            hits.register(HitTarget::Button(HitButton::Apply), rect);
        } else if line.contains("confirm") {
            hits.register(HitTarget::Button(HitButton::Confirm), rect);
        }
    });
}

fn centered(area: Rect, max_width: u16, max_height: u16) -> Rect {
    let width = area.width.clamp(10, max_width);
    let height = area.height.clamp(6, max_height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    Rect::new(x, y, width, height)
}

fn render_schema_diff(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 80, 18);
    let lines = model.schema_diff.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Schema diff"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("filters ") {
            register_label(
                hits,
                rect,
                line,
                "added=",
                HitTarget::Button(HitButton::ToggleAdded),
            );
            register_label(
                hits,
                rect,
                line,
                "removed=",
                HitTarget::Button(HitButton::ToggleRemoved),
            );
            register_label(
                hits,
                rect,
                line,
                "changed=",
                HitTarget::Button(HitButton::ToggleChanged),
            );
        } else if line.starts_with("confirm=") {
            register_label(
                hits,
                rect,
                line,
                "confirm=",
                HitTarget::Button(HitButton::ConfirmDiff),
            );
            register_label(
                hits,
                rect,
                line,
                "apply=",
                HitTarget::Button(HitButton::ApplyDiff),
            );
        }
    });
}

fn render_transfer(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 16);
    let lines = model.transfer.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Transfer"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == 0 {
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
        }
        if line.contains("confirm restore") {
            hits.register(HitTarget::Button(HitButton::Confirm), rect);
        }
    });
}

fn render_security(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 40, 12);
    let lines = model.security.lines();
    render_panel(frame, popup, model, "Security", true, lines.join("\n"));
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if (line.starts_with('>') || line.starts_with("  ")) && i < model.security.principals.len()
        {
            hits.register(HitTarget::ListRow(i), rect);
        }
    });
}

fn render_admin(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 80, 16);
    let lines = model.admin.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Sessions"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.contains("paused=") {
            let button = if model.admin.paused {
                HitButton::Resume
            } else {
                HitButton::Pause
            };
            hits.register(HitTarget::Button(button), rect);
        }
        if line.contains("confirm-target=") || line.contains("confirmed=") {
            hits.register(HitTarget::Button(HitButton::Confirm), rect);
        }
    });
}

fn render_mcp_profiles(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 72, 14);
    let lines = model.mcp_profiles.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("MCP profiles"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if line.starts_with("mcp profile=") || i < model.mcp_profiles.profiles.len() {
            hits.register(HitTarget::ListRow(model.mcp_profiles.selected.min(i)), rect);
        }
        if line.contains("revoke") {
            hits.register(HitTarget::Button(HitButton::Revoke), rect);
        }
    });
}

fn render_connections(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 18);
    let lines = model.connections.lines(model.active_session);
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Connections"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i < model.connections.profiles.len() {
            hits.register(HitTarget::ListRow(i), rect);
        }
        if line.contains("keep secrets") {
            register_label(
                hits,
                rect,
                line,
                "k keep secrets",
                HitTarget::Button(HitButton::KeepSecrets),
            );
            register_label(
                hits,
                rect,
                line,
                "d delete secrets",
                HitTarget::Button(HitButton::DeleteSecrets),
            );
            register_label(
                hits,
                rect,
                line,
                "esc cancel",
                HitTarget::Button(HitButton::Cancel),
            );
        }
        if line.contains(" n new ") || line.contains("n new") {
            register_label(hits, rect, line, "n new", HitTarget::Button(HitButton::New));
            register_label(
                hits,
                rect,
                line,
                "e edit",
                HitTarget::Button(HitButton::Edit),
            );
            register_label(
                hits,
                rect,
                line,
                "d duplicate",
                HitTarget::Button(HitButton::Duplicate),
            );
            register_label(
                hits,
                rect,
                line,
                "t test",
                HitTarget::Button(HitButton::Test),
            );
            register_label(
                hits,
                rect,
                line,
                "x delete",
                HitTarget::Button(HitButton::Delete),
            );
            register_label(
                hits,
                rect,
                line,
                "c close",
                HitTarget::Button(HitButton::CloseSession),
            );
        }
    });
}

fn render_projects(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 18);
    let lines = model.projects.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Projects"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i < model.projects.list.len() {
            hits.register(HitTarget::ListRow(i), rect);
        }
        if line.starts_with("create:") || line.starts_with("rename:") {
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
        }
        if line.contains("connections:") {
            hits.register(HitTarget::Button(HitButton::ToggleConnections), rect);
        }
        if line.contains("delete ") && line.contains('?') {
            hits.register(HitTarget::Button(HitButton::ConfirmDelete), rect);
        }
        if line.contains("switch to ") {
            hits.register(HitTarget::Button(HitButton::ConfirmDirty), rect);
        }
    });
}

fn render_config_transfer(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 16);
    let lines = model.config_transfer.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Config transfer"),
        lines.join("\n"),
    );
    let conflict_names = model
        .config_transfer
        .preview
        .as_ref()
        .map(|preview| preview.conflicts.clone())
        .unwrap_or_default();
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if let Some(name) = line.trim_start().split(':').next()
            && let Some(index) = conflict_names.iter().position(|item| item == name)
        {
            hits.register(HitTarget::ListRow(index), rect);
        }
        if line.starts_with("conflicts:") || line.starts_with("path:") {
            hits.register(HitTarget::Button(HitButton::Apply), rect);
        }
    });
}

fn render_secret(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 56, 8);
    let lines = model.secret_prompt.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Secret"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        register_label(
            hits,
            rect,
            line,
            "s session only",
            HitTarget::Button(HitButton::Session),
        );
        register_label(
            hits,
            rect,
            line,
            "k save to keychain",
            HitTarget::Button(HitButton::Keychain),
        );
        register_label(
            hits,
            rect,
            line,
            "esc cancel",
            HitTarget::Button(HitButton::Cancel),
        );
    });
}

fn render_transaction_prompt(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 56, 8);
    let lines = model.transaction_prompt.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Savepoint"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
        }
    });
}

fn render_data_query(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 56, 8);
    let lines = model.data.query_prompt.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Query"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("column:") {
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.starts_with("value:") {
            hits.register(HitTarget::FormField(1), rect);
        }
        if line.starts_with("descending:") {
            hits.register(HitTarget::Button(HitButton::ToggleDescending), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
        }
    });
}

fn render_connection_form(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let popup = centered(area, 72, area.height.saturating_sub(2).min(22));
    let rows = popup.height.saturating_sub(2).max(4) as usize;
    let lines = model.connection_form.visible_lines(rows);
    paint_popup(
        frame,
        popup,
        overlay_block(model, model.connection_form.title()),
        lines.join("\n"),
    );
    let body_rows = rows.saturating_sub(1).max(1);
    let focus_line = model
        .connection_form
        .focus
        .min(model.connection_form.fields.len().saturating_sub(1));
    let offset = scroll_to_selection(focus_line, 0, model.connection_form.fields.len(), body_rows);
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
            return;
        }
        if i < body_rows {
            hits.register(HitTarget::FormField(offset.saturating_add(i)), rect);
        }
        if line.contains("driver:") {
            hits.register(HitTarget::Button(HitButton::CycleDriver), rect);
        }
    });
}

fn render_settings(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 64, 12);
    let lines = model.settings.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Settings"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("theme=") {
            hits.register(HitTarget::Button(HitButton::Theme), rect);
        } else if line.starts_with("keymap=") {
            hits.register(HitTarget::Button(HitButton::Keymap), rect);
        } else if line.starts_with("mouse=") {
            hits.register(HitTarget::Button(HitButton::Mouse), rect);
        } else if line.starts_with("confirm_reset=") {
            hits.register(HitTarget::Button(HitButton::Reset), rect);
        }
    });
}

fn render_recovery(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 64, 12);
    let lines = model.recovery.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Session recovery"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("recovery open=") {
            hits.register(HitTarget::Button(HitButton::Recover), rect);
        }
        if line.starts_with("confirm_discard=") {
            hits.register(HitTarget::Button(HitButton::Discard), rect);
        }
    });
}

fn render_diagnostics(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 16);
    let lines = model.diagnostics.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Diagnostics"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, _, rect| {
        hits.register(HitTarget::Button(HitButton::Export), rect);
    });
}

fn render_mcp_audit(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 12);
    let lines = model.mcp_audit.lines();
    paint_popup(
        frame,
        popup,
        Block::bordered().title("MCP audit"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.contains("revoke") {
            hits.register(HitTarget::Button(HitButton::Revoke), rect);
        }
    });
}

fn render_completion(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
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
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, _, rect| {
        if lines.first().map(String::as_str) == Some("(empty)") {
            return;
        }
        hits.register(HitTarget::ListRow(offset.saturating_add(i)), rect);
    });
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

fn render_parameters(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let name = model
        .editor
        .parameters
        .get(model.editor.parameter_index)
        .map(|parameter| parameter.name.as_str())
        .unwrap_or("param");
    let popup = centered(frame.area(), 48, 6);
    let body = format!("{name} = {}", model.editor.parameter_draft);
    paint_popup(
        frame,
        popup,
        Block::bordered().title("Parameters"),
        body.clone(),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &[body], |_, _, rect| {
        hits.register(HitTarget::FormField(0), rect);
        crate::widgets::form::register_footer(hits, rect, "[Submit]  [Cancel]", "Submit");
    });
}

fn render_list_overlay(
    frame: &mut Frame,
    title: &str,
    items: &[String],
    selected: usize,
    offset: usize,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let popup = centered(area, 48, 12);
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
    paint_popup(
        frame,
        popup,
        Block::bordered().title(title.to_string()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if line == "(empty)" {
            hits.register(HitTarget::Button(HitButton::Confirm), rect);
            return;
        }
        hits.register(HitTarget::ListRow(offset.saturating_add(i)), rect);
    });
}

fn render_history(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    if model.editor.history_confirm_clear {
        let target = if model.connection.name.is_empty() {
            "all"
        } else {
            model.connection.name.as_str()
        };
        render_list_overlay(
            frame,
            &format!("clear history for {target}?"),
            &[],
            0,
            0,
            hits,
        );
    } else {
        render_list_overlay(
            frame,
            "History",
            &model.editor.history,
            model.editor.history_selected,
            0,
            hits,
        );
    }
}

fn render_snippets(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let names: Vec<String> = model
        .editor
        .snippets
        .iter()
        .map(|snippet| snippet.name.clone())
        .collect();
    render_list_overlay(
        frame,
        "Snippets",
        &names,
        model.editor.snippet_selected,
        0,
        hits,
    );
}

fn render_file_picker(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let popup = centered(area, 72, area.height.saturating_sub(2).min(22));
    let list_rows = popup.height.saturating_sub(5).max(4) as usize;
    let lines = model.file_picker.lines(model.file_picker_mode, list_rows);
    paint_popup(
        frame,
        popup,
        overlay_block(model, model.file_picker_mode.title()),
        lines.join("\n"),
    );
    let offset = crate::palette::scroll_to_selection(
        model.file_picker.selected,
        model.file_picker.offset,
        model.file_picker.entries.len(),
        list_rows.max(1),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == 0 {
            hits.register(HitTarget::Button(HitButton::ParentDir), rect);
            return;
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(
                hits,
                rect,
                line,
                model.file_picker_mode.submit_label(),
            );
            return;
        }
        if line.contains("name:") {
            hits.register(HitTarget::FormField(0), rect);
            return;
        }
        let index = offset.saturating_add(i.saturating_sub(1));
        if index < model.file_picker.entries.len() {
            hits.register(HitTarget::ListRow(index), rect);
        }
    });
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
    fn editor_tabs_are_not_all_sql() {
        let mut model = Model {
            width: 100,
            height: 40,
            ..Model::default()
        };
        model.tabs.active = 1;
        let data = render_to_string(&model, 100, 40);
        assert!(data.contains("Open a table or run a query"));
        model.tabs.active = 3;
        let props = render_to_string(&model, 100, 40);
        assert!(props.contains("Select an object in Explorer"));
        model.tabs.active = 2;
        let ddl = render_to_string(&model, 100, 40);
        assert!(ddl.contains("schema table") || ddl.contains("target:"));
    }

    #[test]
    fn sql_workbench_renders_document_tabs_with_dirty_marker() {
        let mut model = Model::default();
        model.documents = vec![
            crate::model::EditorDocument::new_unique("console.sql", None, None),
            crate::model::EditorDocument::new_unique("q2.sql", None, None),
        ];
        model.documents[1].sql.insert(0, "select 1").unwrap();
        model.active_document = 1;

        let frame = render_to_string(&model, 120, 35);

        assert!(frame.contains("console.sql"));
        assert!(frame.contains("q2.sql*"));
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
