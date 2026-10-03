use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::layout::LayoutPlan;
use crate::model::{Focus, Model, Severity};
use crate::mouse::{
    HitButton, HitMap, HitTarget, PaneEdge, overlay_blocks_workbench, popup_inner, register_label,
    register_line, register_overlay,
};
use crate::palette::{filter_entries, palette_entries, scroll_to_selection};
use crate::theme::Role;

pub fn render(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    draw_workbench(frame, model, hits);
    if !model.capabilities.unicode {
        asciify(frame);
    }
}

/// With Unicode glyphs off, the marks that are not box drawing fall back to ASCII: the
/// pointer, the folder arrows, the ellipsis, the separators and the close mark were
/// drawn whatever the setting said, in every place that spells one.
fn asciify(frame: &mut Frame) {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            let ascii = match cell.symbol() {
                "▸" | "▶" => ">",
                "▾" | "▼" => "v",
                "…" => ".",
                "·" | "—" | "–" => "-",
                "×" => "x",
                "↑" => "^",
                "█" => "#",
                "‹" => "<",
                "›" => ">",
                _ => continue,
            };
            cell.set_symbol(ascii);
        }
    }
}

fn draw_workbench(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    hits.clear();
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, model.theme.base(model.capabilities));
    let plan =
        LayoutPlan::for_area_with_document_tabs(frame.area(), Some(&model.effective_panes()), true);
    render_bar(frame, plan.context, context_line(model));
    match plan.mode {
        crate::layout::LayoutMode::Compact => {
            crate::widgets::document_tabs::render(frame, plan.document_tabs, model, hits);
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
                model.effective_focus() == Focus::Explorer,
                explorer_body(model, plan.explorer),
            );
            crate::widgets::document_tabs::render(frame, plan.document_tabs, model, hits);
            if model.active_document().kind.is_table() {
                let grid_pane = plan.grid_pane(true);
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Grid, grid_pane);
                }
                crate::widgets::grid::render(frame, grid_pane, model, hits);
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Console, plan.results);
                }
                render_console_log(
                    frame,
                    plan.results,
                    model,
                    model.effective_focus() == Focus::Console,
                );
            } else {
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Editor, plan.content);
                }
                render_editor_content(frame, plan.content, model, hits);
                let grid_pane = plan.grid_pane(false);
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Grid, grid_pane);
                }
                crate::widgets::grid::render(frame, grid_pane, model, hits);
            }
        }
    }
    if !overlay_blocks_workbench(model) && plan.mode != crate::layout::LayoutMode::Compact {
        register_pane_dividers(hits, plan);
    }
    if plan.mode == crate::layout::LayoutMode::Compact {
        render_pane_switcher(frame, plan.context, model, hits);
    }
    crate::widgets::status::render(frame, plan.status, model);
    if model.onboarding.open {
        render_onboarding(frame, model, hits);
        return;
    }
    if model.palette.open {
        render_palette(frame, model, hits);
    }
    if model.help.open {
        render_help(frame, model, hits);
    }
    if model.results_menu.open {
        render_results_menu(frame, model, hits);
    }
    if model.node_menu.open {
        render_node_menu(frame, model, hits);
    }
    if let Some(review) = &model.data.review {
        render_review(frame, model, review, hits);
    }
    if model.data.insert_form.open {
        render_insert_row_form(frame, model, hits);
    }
    if model.data.cell_edit.is_some() {
        render_cell_edit(frame, model, hits);
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
    // Above the panel that asked for it, which drew over it and left a few columns of it.
    if let Some(preview) = &model.schema_editor.preview {
        render_ddl_preview(frame, model, preview, hits);
    }
    if model.admin.open {
        render_admin(frame, model, hits);
    }
    if model.mcp_profiles.open {
        render_mcp_profiles(frame, model, hits);
    }
    // A dialog opened over another is drawn alone: the one under it, taller, showed its
    // bottom border as a second box edge under the new one.
    if model.connections.open && !model.connection_form.open && !model.secret_prompt.open {
        render_connections(frame, model, hits);
    }
    if model.projects.open {
        render_projects(frame, model, hits);
    }
    if model.config_transfer.open && !model.file_picker.open {
        render_config_transfer(frame, model, hits);
    }
    if model.secret_prompt.open {
        render_secret(frame, model, hits);
    }
    if model.transaction_prompt.open {
        render_transaction_prompt(frame, model, hits);
    }
    if model.document_name_prompt.open {
        render_document_name_prompt(frame, model, hits);
    }
    if model.saved_queries.open {
        render_saved_queries(frame, model, hits);
    }
    if let Some(prompt) = &model.try_index {
        let popup = centered(frame.area(), 72, 7);
        let lines = prompt.lines(popup_inner(popup).width as usize);
        paint_popup(
            frame,
            model,
            popup,
            overlay_block(model, "Try an index"),
            lines.join("\n"),
        );
        register_overlay(hits, popup);
        for_popup_lines(popup, &lines, |_, line, rect| {
            if line.starts_with("index:") {
                let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
                paint_selection(frame, rect, "index: ", &prompt.input, focused);
                hits.register(HitTarget::FormField(0), rect);
            }
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Try");
            }
        });
    }
    if let Some(prompt) = &model.save_query_prompt {
        let popup = centered(frame.area(), 64, 9);
        let lines = prompt.lines(popup_inner(popup).width as usize);
        paint_popup(
            frame,
            model,
            popup,
            overlay_block(model, "Save query"),
            lines.join("\n"),
        );
        register_overlay(hits, popup);
        for_popup_lines(popup, &lines, |_, line, rect| {
            if line.starts_with("name:") {
                let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
                paint_selection(frame, rect, "name: ", &prompt.name, focused);
                hits.register(HitTarget::FormField(0), rect);
            }
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Save");
            }
        });
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
    if model.inspector.open {
        render_object_overlay(frame, model, hits);
    }
    if model.schema_editor.open {
        render_schema_form(frame, model, hits);
    }
    if model.data.viewer.is_some() {
        render_value_viewer(frame, model, hits);
    }
    render_toast(frame, model, hits);
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
    if let Some(picker) = &model.data.related_picker {
        render_related_picker(frame, model, picker, hits);
    }
    if model.connections.delete_target.is_some() {
        render_delete_connection(frame, model, hits);
    }
    if let Some(focus) = model.explain_prompt {
        render_explain_prompt(frame, model, focus, hits);
    }
    if let Some(focus) = model.quit_prompt {
        render_quit_prompt(frame, model, focus, hits);
    }
    if let Some(prompt) = &model.run_prompt {
        render_run_prompt(frame, model, prompt, hits);
    }
    if let Some(prompt) = &model.production_prompt {
        render_production_prompt(frame, model, prompt, hits);
    }
    if let Some(prompt) = &model.close_prompt {
        render_close_prompt(frame, model, prompt, hits);
    }
}

/// "Save, don't save, or cancel" for a document closed with unsaved changes. The
/// focused button is bracketed with a marker, not only styled, so the choice still
/// reads on a terminal without colour.
fn render_close_prompt(
    frame: &mut Frame,
    model: &Model,
    prompt: &crate::model::ClosePrompt,
    hits: &mut HitMap,
) {
    use crate::model::CloseChoice;
    let area = frame.area();
    if area.width < 20 || area.height < 7 {
        return;
    }
    let name = if prompt.title.is_empty() {
        "This document".to_string()
    } else {
        prompt.title.clone()
    };
    let buttons = [
        (CloseChoice::Save, "[Save]", HitButton::Confirm),
        (CloseChoice::Discard, "[Don't save]", HitButton::Discard),
        (CloseChoice::Cancel, "[Cancel]", HitButton::Cancel),
    ];
    let footer: String = buttons
        .iter()
        .map(|(choice, label, _)| {
            let marker = if *choice == prompt.choice { ">" } else { " " };
            format!("{marker}{label}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    // Short enough for a narrow terminal: the sentence was cut mid-word at 40 columns.
    let narrow = area.width < 48;
    let lines = [
        if narrow {
            format!("{name} is not saved.")
        } else {
            format!("{name} has changes that are not saved.")
        },
        if narrow {
            "Closing loses the changes.".to_string()
        } else {
            "Closing it without saving loses them.".to_string()
        },
        String::new(),
        footer.clone(),
    ];
    let content_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let width = (content_width as u16 + 4).min(area.width);
    let popup = centered(area, width, lines.len() as u16 + 2);
    let danger = model.theme.style(Role::Warning, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if index == 1 {
                Line::styled(text.clone(), danger)
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "Unsaved changes")),
        popup,
    );
    register_overlay(hits, popup);
    let footer_row = crate::mouse::line_rect(popup_inner(popup), lines.len() - 1);
    for (_, label, button) in buttons {
        register_label(hits, footer_row, &footer, label, HitTarget::Button(button));
    }
}

fn render_run_prompt(
    frame: &mut Frame,
    model: &Model,
    prompt: &crate::screens::run_prompt::RunPrompt,
    hits: &mut HitMap,
) {
    let width = 72.min(frame.area().width);
    let lines = prompt.lines(width.saturating_sub(2) as usize);
    let popup = centered(frame.area(), width, lines.len() as u16 + 2);
    // The reason each statement is flagged is the warning, in the warning colour, under a
    // border like every other dialog's.
    let warning = model.theme.style(Role::Warning, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .map(|line| {
            if line.starts_with("   ") && !line.trim().is_empty() {
                Line::styled(line.clone(), warning)
            } else {
                Line::raw(line.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, prompt.title())),
        popup,
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
            paint_selection(frame, rect, "name: ", &prompt.typed, focused);
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Run");
        }
    });
}

fn render_quit_prompt(
    frame: &mut Frame,
    model: &Model,
    focus: crate::widgets::form::FooterFocus,
    hits: &mut HitMap,
) {
    let mut lines = crate::update::quit_losses(model);
    lines.push(String::new());
    lines.push(crate::widgets::form::footer_line("Quit", focus));
    let width = lines
        .iter()
        .map(|line| line.chars().count() as u16 + 2)
        .max()
        .unwrap_or(0)
        .clamp(30, 76)
        .min(frame.area().width);
    let popup = centered(frame.area(), width, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title("Quit Dexo?"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let footer = crate::mouse::line_rect(popup_inner(popup), lines.len() - 1);
    crate::widgets::form::register_footer(hits, footer, &lines[lines.len() - 1], "Quit");
}

fn render_production_prompt(
    frame: &mut Frame,
    model: &Model,
    prompt: &crate::screens::production_prompt::ProductionPrompt,
    hits: &mut HitMap,
) {
    let width = 76.min(frame.area().width);
    let lines = prompt.lines(width.saturating_sub(2) as usize);
    let popup = centered(frame.area(), width, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title("Write on production"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
            paint_selection(frame, rect, "name: ", &prompt.typed, focused);
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Confirm");
        }
    });
}

fn render_explain_prompt(
    frame: &mut Frame,
    model: &Model,
    focus: crate::widgets::form::FooterFocus,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 20 || area.height < 8 {
        return;
    }
    let document = model.active_document();
    let text = document.text();
    let cursor = text
        .chars()
        .take(document.cursor())
        .map(char::len_utf8)
        .sum();
    let statement =
        dexo_sql::statement_at_in(&text, cursor, crate::screens::editor::editor_dialect(model))
            .map(|span| text[span.byte_range].trim().to_string())
            .unwrap_or_default();
    let width = 64.min(area.width);
    let preview_width = width.saturating_sub(6) as usize;
    let first_line = statement.lines().next().unwrap_or_default();
    let mut preview: String = first_line.chars().take(preview_width).collect();
    if first_line.chars().count() > preview_width || statement.lines().nth(1).is_some() {
        preview.pop();
        preview.push('…');
    }
    let production = dexo_app::Environment::parse_strict(&model.connection.environment)
        == dexo_app::Environment::Production;
    let footer = crate::widgets::form::footer_line("Run", focus);
    let mut lines = vec![
        "EXPLAIN ANALYZE runs this statement to time it,".to_string(),
        "then rolls back what it changed; a sequence or".to_string(),
        "an auto-increment counter keeps its advance.".to_string(),
    ];
    let warning_line = production.then_some(lines.len());
    if production {
        lines.push("This is a production connection.".to_string());
    }
    lines.extend([
        String::new(),
        format!("  {preview}"),
        String::new(),
        footer.clone(),
    ]);
    let popup = centered(area, width, lines.len() as u16 + 2);
    let warning = model.theme.style(Role::Warning, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if warning_line == Some(index) {
                Line::styled(text.clone(), warning)
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "Explain Analyze")),
        popup,
    );
    register_overlay(hits, popup);
    let footer_row = crate::mouse::line_rect(popup_inner(popup), lines.len() - 1);
    crate::widgets::form::register_footer(hits, footer_row, &footer, "Run");
}

fn register_pane_dividers(hits: &mut HitMap, plan: LayoutPlan) {
    if plan.explorer.width > 0 {
        hits.register(
            HitTarget::PaneDivider(PaneEdge::Explorer),
            // Both border cells of the divider, the explorer's and the editor's.
            Rect::new(
                plan.content.x.saturating_sub(1),
                plan.explorer.y,
                2,
                plan.explorer.height,
            ),
        );
    }
    if plan.results.height > 0 {
        hits.register(
            HitTarget::PaneDivider(PaneEdge::Results),
            Rect::new(
                plan.results.x,
                plan.results.y.saturating_sub(1),
                plan.results.width,
                2,
            ),
        );
    }
}

fn render_onboarding(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, model.theme.base(model.capabilities));
    register_overlay(hits, area);

    let compact = area.width < 60 || area.height < 18;
    let frame_idx = model
        .onboarding
        .logo_frame
        .min(model.onboarding.logo_frames.len().saturating_sub(1));
    let logo = model
        .onboarding
        .logo_frames
        .get(frame_idx)
        .cloned()
        .unwrap_or_else(crate::entrance::static_logo_frame);

    let run = crate::palette::shortcut_for(model, "query.execute_statement", Some("Ctrl+Enter"))
        .unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    if compact {
        // Eight rows fit inside the popup at 40x12. With the connection hint, the blank
        // that sat under the name would push Get started out of sight.
        lines.push("DEXO".into());
        lines.push("Welcome".into());
        lines.push("Ctrl+P  palette".into());
        lines.push(format!("{run}  run"));
        lines.push("F1  help".into());
        lines.push("n  new connection".into());
        lines.push(String::new());
        lines.push("[Get started]".into());
    } else {
        lines.push("Welcome".into());
        lines.push(String::new());
        for row in &logo.rows {
            lines.push(
                row.iter()
                    .map(|cell| cell.symbol.as_str())
                    .collect::<String>(),
            );
        }
        lines.push(String::new());
        lines.push("Ctrl+P opens the command palette.".into());
        lines.push(format!("{run} runs the SQL under the cursor."));
        lines.push("F1 opens help.".into());
        lines.push("n adds a connection (or Ctrl+P, New Connection).".into());
        lines.push(String::new());
        lines.push("[Get started]".into());
    }

    let popup = centered(
        area,
        if compact {
            area.width.saturating_sub(2).max(20)
        } else {
            72
        },
        if compact {
            area.height.saturating_sub(2).max(10)
        } else {
            (lines.len() as u16).saturating_add(2).min(area.height)
        },
    );
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "DEXO"),
        lines.join("\n"),
    );
    let mut logo_rows = Vec::new();
    for_popup_lines(popup, &lines, |index, line, rect| {
        if line.contains("Get started") {
            hits.register(HitTarget::Button(HitButton::GetStarted), rect);
        }
        // The logo sits two lines down, under "Welcome" and a blank.
        if !compact && (2..2 + logo.rows.len()).contains(&index) {
            logo_rows.push((index - 2, rect));
        }
    });
    paint_logo(frame, model, &logo, &logo_rows);
}

/// The logo's colours, cell by cell: the animation's own, or across a still logo the
/// theme's accent fading to its text colour. Painted over the popup's text, which is
/// plain -- joining the cells into lines used to drop their colours, and the logo came
/// out in the text colour. A terminal without true colour gets the accent alone.
fn paint_logo(
    frame: &mut Frame,
    model: &Model,
    logo: &crate::entrance::LogoFrame,
    rows: &[(usize, Rect)],
) {
    use crate::capabilities::ColorDepth;
    use crate::theme::Role;
    let caps = model.capabilities;
    let ends = model
        .theme
        .rgb(Role::Focus)
        .zip(model.theme.rgb(Role::Foreground));
    let width = logo.rows.first().map_or(0, Vec::len).max(1);
    let buffer = frame.buffer_mut();
    for &(row, rect) in rows {
        for (column, cell) in logo.rows[row].iter().enumerate() {
            let Ok(offset) = u16::try_from(column) else {
                break;
            };
            if offset >= rect.width || cell.symbol.trim().is_empty() {
                continue;
            }
            let color = match (caps.color_depth, cell.foreground, ends) {
                (ColorDepth::None, ..) => None,
                (ColorDepth::TrueColor, Some((r, g, b)), _) => Some(Color::Rgb(r, g, b)),
                (ColorDepth::TrueColor, None, Some((from, to))) => {
                    Some(blend(from, to, column as f32 / width as f32))
                }
                _ => model.theme.color(Role::Focus, caps),
            };
            if let (Some(color), Some(target)) = (color, buffer.cell_mut((rect.x + offset, rect.y)))
            {
                target.set_fg(color);
            }
        }
    }
}

fn blend(from: (u8, u8, u8), to: (u8, u8, u8), at: f32) -> Color {
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * at).round() as u8;
    Color::Rgb(mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2))
}

fn render_compact(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let interactive = !overlay_blocks_workbench(model);
    match model.effective_focus() {
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
        // The strip is its own row above the content; focusing it leaves the pane below
        // showing whatever the active document shows.
        Focus::DocumentTabs | Focus::Editor | Focus::Palette
            if model.active_document().kind.is_table() =>
        {
            if interactive {
                hits.register(HitTarget::Grid, area);
            }
            crate::widgets::grid::render(frame, area, model, hits);
        }
        Focus::DocumentTabs | Focus::Editor | Focus::Palette => {
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
        Focus::Console => render_console_log(frame, area, model, true),
    }
}

/// Compact mode draws one pane at a time, and only Alt+1..3 changed it: nothing on
/// screen said so, and the mouse could not. The header row carries the three panes, the
/// one on screen in brackets, each a click.
fn render_pane_switcher(frame: &mut Frame, row: Rect, model: &Model, hits: &mut HitMap) {
    let focus = model.effective_focus();
    let panes = [
        ("1 Sidebar", "1", Focus::Explorer, HitTarget::Explorer),
        ("2 SQL", "2", Focus::Editor, HitTarget::Editor),
        ("3 Results", "3", Focus::Results, HitTarget::Grid),
    ];
    let current = match focus {
        Focus::Explorer => 0,
        Focus::Results | Focus::Console => 2,
        _ => 1,
    };
    // The names when the row has room beside the connection, the digits when not.
    let spelled: usize = panes.iter().map(|pane| pane.0.len() + 3).sum();
    let named = usize::from(row.width) >= context_line(model).chars().count() + spelled + 2;
    let mut x = row.x + row.width;
    let mut cells: Vec<(Rect, String, bool, HitTarget)> = Vec::new();
    for (index, (name, digit, _, target)) in panes.iter().enumerate().rev() {
        let label = if named { *name } else { *digit };
        let text = format!("[{label}]");
        let width = text.chars().count() as u16 + 1;
        if x < row.x + width {
            break;
        }
        x -= width;
        cells.push((
            Rect::new(x, row.y, width, 1),
            text,
            index == current,
            *target,
        ));
    }
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let on = model.theme.style(Role::Focus, model.capabilities);
    for (rect, text, active, target) in cells {
        // The brackets stand for the pane in use even on a terminal with no colour.
        let shown = if active {
            text
        } else {
            text.replace(['[', ']'], " ")
        };
        frame.render_widget(
            Paragraph::new(shown).style(if active { on } else { muted }),
            rect,
        );
        if !overlay_blocks_workbench(model) {
            hits.register(target, rect);
        }
    }
}

fn context_line(model: &Model) -> String {
    format!(
        "{}  {}{}  {}",
        model.project,
        if model.connection.name.is_empty() {
            "no connection"
        } else {
            &model.connection.name
        },
        if model.connection.read_only && !model.connection.name.is_empty() {
            " (read-only)"
        } else {
            ""
        },
        if model.schema.is_empty() {
            "—"
        } else {
            &model.schema
        }
    )
}

fn render_console_log(frame: &mut Frame, area: Rect, model: &Model, focused: bool) {
    let log = &model.active_document().console_log;
    let rows = area.height.saturating_sub(2) as usize;
    let scroll = log.len().saturating_sub(rows.max(1)) as u16;
    render_panel_scrolled(
        frame,
        area,
        model,
        "Console",
        focused,
        log.join("\n"),
        scroll,
    );
}

fn render_editor_content(frame: &mut Frame, area: Rect, model: &Model, _hits: &mut HitMap) {
    crate::widgets::editor::render(frame, area, model);
}

/// The name of an object the inspector lists, from the tree; one the tree has not loaded
/// is said by its kind. The catalog's own ids (`pg:table:17104`) mean nothing to a person.
fn object_name(model: &Model, id: &dexo_driver_api::ObjectId) -> String {
    fn find(nodes: &[crate::screens::explorer::ExplorerNode], id: &str) -> Option<String> {
        nodes.iter().find_map(|node| {
            if node.id.as_str() == id {
                Some(if node.qualified.is_empty() {
                    node.label.clone()
                } else {
                    node.qualified.clone()
                })
            } else {
                find(&node.children, id)
            }
        })
    }
    find(&model.explorer.roots, id.as_str()).unwrap_or_else(|| {
        let kind = id.as_str().split(':').nth(1).unwrap_or("object");
        format!("a {kind}")
    })
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
        if let Some(type_name) = object.attributes.get("type").and_then(|v| v.as_str()) {
            lines.push(format!("type: {type_name}"));
        }
        // Each driver names these its own way (`driver.sqlite.not_null`, `driver.mysql.nullable`).
        for (key, value) in &object.attributes {
            let Some(rest) = key.strip_prefix("driver.") else {
                continue;
            };
            match (rest.split_once('.').map(|(_, name)| name), value.as_bool()) {
                (Some("not_null"), Some(not_null)) => {
                    lines.push(format!("nullable: {}", if not_null { "no" } else { "yes" }))
                }
                (Some("nullable"), Some(nullable)) => {
                    lines.push(format!("nullable: {}", if nullable { "yes" } else { "no" }))
                }
                (Some("default"), _) => {
                    if let Some(default) = value.as_str() {
                        lines.push(format!("default: {default}"));
                    }
                }
                _ => {}
            }
        }
        match model.inspector.shown_note() {
            Some(note) => lines.push(format!("note: {note}")),
            None => lines.push("note: none yet; n writes one".into()),
        }
    }
    // One related object to a line, by what it is: a list of ids ran past the border.
    for (title, ids) in [
        ("depends on", &model.inspector.dependencies),
        ("depended on by", &model.inspector.dependents),
    ] {
        if ids.is_empty() {
            continue;
        }
        lines.push(format!("{title}:"));
        for id in ids {
            let name = model
                .inspector
                .names
                .get(id)
                .cloned()
                .unwrap_or_else(|| object_name(model, id));
            lines.push(format!("  {name}"));
        }
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

fn explorer_body(model: &Model, area: Rect) -> Vec<Line<'static>> {
    let rows = area.height.saturating_sub(2).max(1) as usize;
    let lines = crate::widgets::object_tree::render_sidebar(
        &model.explorer,
        &model.connections.profiles,
        &model.connection.name,
        model.capabilities.unicode,
        rows,
        area.width.saturating_sub(2) as usize,
    );
    // The cursor sits in the first column and the name can be a long way to its right,
    // so the row it points at carries the colour too. Found through the same layout the
    // hit map reads, not by looking for the marker, so the two cannot disagree.
    let current = crate::widgets::object_tree::sidebar_layout(
        &model.explorer,
        model.connections.profiles.len(),
        &model.connection.name,
        rows,
    );
    let current = model
        .explorer
        .selected_index()
        .checked_sub(current.offset)
        .filter(|index| *index < current.nodes.len())
        .map(|index| current.node_row(index));
    // The accent means "the pane you are in". Out of focus the row is still where the
    // cursor is, so it stays bold, but it gives the colour back to the focused pane.
    let style = if model.effective_focus() == Focus::Explorer {
        model.theme.header(model.capabilities)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    lines
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            if Some(index) == current {
                Line::styled(text, style)
            } else {
                Line::raw(text)
            }
        })
        .collect()
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
    body: impl Into<ratatui::text::Text<'static>>,
) {
    render_panel_scrolled(frame, area, model, title, focused, body, 0);
}

fn render_panel_scrolled(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    title: &str,
    focused: bool,
    body: impl Into<ratatui::text::Text<'static>>,
    scroll: u16,
) {
    let body = body.into();
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
        .style(model.theme.base(model.capabilities))
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
    // The same header the sidebar drew, so a click lands on the word under the cursor
    // at every width rather than on where the word sits when the pane is wide.
    let header = crate::widgets::object_tree::sidebar_header(inner.width as usize);
    for (needle, target) in [
        ("[n]ew", HitButton::New),
        ("[e]dit", HitButton::Edit),
        ("[a]ctions", HitButton::Actions),
    ] {
        register_label(
            hits,
            crate::mouse::line_rect(inner, 0),
            header,
            needle,
            HitTarget::Button(target),
        );
    }
    let layout = crate::widgets::object_tree::sidebar_layout(
        &model.explorer,
        model.connections.profiles.len(),
        &model.connection.name,
        (inner.height as usize).max(1),
    );
    for (index, _) in layout.nodes.iter().enumerate() {
        register_line(
            hits,
            inner,
            layout.node_row(index),
            HitTarget::ExplorerNode(layout.offset.saturating_add(index)),
        );
    }
    // The arrow is its own target, over the row's: it toggles what a plain click only
    // selects.
    let lines = crate::widgets::object_tree::render_sidebar(
        &model.explorer,
        &model.connections.profiles,
        &model.connection.name,
        model.capabilities.unicode,
        (inner.height as usize).max(1),
        inner.width as usize,
    );
    for (index, _) in layout.nodes.iter().enumerate() {
        let row = layout.node_row(index);
        let Some(line) = lines.get(row) else { continue };
        if let Some(column) = line.chars().position(|ch| ch == '▸' || ch == '▾') {
            let line = crate::mouse::line_rect(inner, row);
            hits.register(
                HitTarget::ExplorerTwistie(layout.offset.saturating_add(index)),
                Rect::new(line.x.saturating_add(column as u16), line.y, 1, 1),
            );
        }
    }
}

/// `Clear` leaves the cells in the terminal's own colours, and the terminal's background
/// is the theme's, so an unstyled popup draws the terminal's foreground on the theme's
/// background: invisible when a dark theme meets a terminal with dark text.
fn paint_popup(frame: &mut Frame, model: &Model, popup: Rect, block: Block<'static>, body: String) {
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(block.style(model.theme.base(model.capabilities))),
        popup,
    );
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

/// A selected input's value in reverse video. The popups draw their lines as plain
/// text, so the selection is painted over the line: `before` is what precedes the value.
fn paint_selection(
    frame: &mut Frame,
    line: Rect,
    before: &str,
    input: &crate::widgets::text_input::TextInput,
    focused: bool,
) {
    use unicode_width::UnicodeWidthStr;
    if focused && input.is_selected() {
        paint_reversed(frame, line, before.width(), input.as_str().width());
    }
}

/// A field drawn as plain text, with no cursor of its own: the terminal's cursor goes
/// where the input's is, and a selection shows in reverse. `before` is what precedes the
/// value on `line`; a `masked` value is drawn as one mark per character.
fn show_input(
    frame: &mut Frame,
    line: Rect,
    before: &str,
    input: &crate::widgets::text_input::TextInput,
    masked: bool,
) {
    use unicode_width::UnicodeWidthStr;
    let head: String = input.as_str().chars().take(input.cursor()).collect();
    let (value, cursor) = if masked {
        (input.len(), input.cursor())
    } else {
        (input.as_str().width(), head.width())
    };
    let start = before.width();
    if input.is_selected() {
        paint_reversed(frame, line, start, value);
    }
    let x = start + cursor;
    if x < usize::from(line.width) {
        frame.set_cursor_position(ratatui::layout::Position::new(line.x + x as u16, line.y));
    }
}

/// [`show_input`] for a value drawn as the stretch of it around the cursor
/// ([`TextInput::window`]), so a value wider than `line` scrolls with the cursor.
fn show_windowed_input(
    frame: &mut Frame,
    line: Rect,
    before: &str,
    input: &crate::widgets::text_input::TextInput,
) {
    use unicode_width::UnicodeWidthStr;
    let start = before.width();
    let room = usize::from(line.width).saturating_sub(start).max(1);
    let (shown, at) = input.window(room);
    if input.is_selected() {
        paint_reversed(frame, line, start, shown.trim_end().width());
    }
    let x = start + at;
    if x < usize::from(line.width) {
        frame.set_cursor_position(ratatui::layout::Position::new(line.x + x as u16, line.y));
    }
}

/// The focused field of a form, drawn `> label: value` on `line`.
fn show_form_field(
    frame: &mut Frame,
    line: Rect,
    field: &crate::screens::schema_editor::FormField,
) {
    let before = format!("> {}: ", field.label);
    show_input(frame, line, &before, &field.value, field.secret);
}

/// Reverses `columns` cells of `line` from its column `from`, as far as the line goes.
fn paint_reversed(frame: &mut Frame, line: Rect, from: usize, columns: usize) {
    let cells = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
    let start = line.x.saturating_add(cells(from));
    let end = start.saturating_add(cells(columns)).min(line.right());
    let buffer = frame.buffer_mut();
    for x in start..end {
        if let Some(cell) = buffer.cell_mut((x, line.y)) {
            cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }
}

/// Width of the palette's category gutter, sized to the longest label.
const CATEGORY_WIDTH: usize = 12;

/// Widest the sidebar context menu gets. Its longest title is "Duplicate Connection".
const NODE_MENU_WIDTH: u16 = 38;

fn render_palette(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let entries = palette_entries(model);
    let visible = filter_entries(&entries, model.palette.query.as_str());
    let width = area.width.clamp(10, crate::palette::POPUP_MAX_WIDTH);
    let height = crate::palette::popup_height(area.height, visible.len());
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    let popup = Rect::new(x, y, width, height);
    let query_fits = model.palette.query.len() + 4 <= usize::from(width.saturating_sub(2));
    let mut lines = vec![if query_fits {
        format!("> {}", model.palette.query.as_str())
    } else {
        // Longer than the box: the end the user is typing stays in view.
        model
            .palette
            .query
            .inline_line_within("> ", true, usize::from(width.saturating_sub(2)))
    }];
    let rows = crate::palette::popup_list_rows(area.height, visible.len());
    let offset = scroll_to_selection(
        model.palette.selected,
        model.palette.offset,
        visible.len(),
        rows,
    );
    let browsing = model.palette.query.is_empty();
    let inner_width = popup.width.saturating_sub(2) as usize;
    let window: Vec<&&crate::palette::PaletteEntry> =
        visible.iter().skip(offset).take(rows).collect();
    // Searching, the categories line up in a column of their own; ragged labels glued to
    // each title read as noise rather than as the qualifier they are.
    let title_width = window
        .iter()
        .map(|entry| entry.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(inner_width.saturating_sub(CATEGORY_WIDTH + 4));
    for (row, entry) in window.iter().enumerate() {
        let index = offset + row;
        let marker = if index == model.palette.selected {
            ">"
        } else {
            " "
        };
        let label = crate::palette::category_label(entry.id);
        // Browsing, the category is a heading: printed once per group and again on the
        // first visible row so scrolling never loses it. Searching, the order is
        // relevance, not category, so a leading gutter would indent every title behind
        // dead space -- the label trails the title there instead.
        let (category, title) = if browsing {
            let repeats = row > 0 && crate::palette::category_label(window[row - 1].id) == label;
            let gutter = if repeats {
                " ".repeat(CATEGORY_WIDTH)
            } else {
                format!("{label:<CATEGORY_WIDTH$}")
            };
            (gutter, entry.title.to_string())
        } else {
            (
                String::new(),
                format!("{:<title_width$}  {label}", entry.title),
            )
        };
        // The shortcut sits against the right edge so the keys read as one column
        // instead of trailing each title at a different offset.
        let used = 2 + category.chars().count() + title.chars().count();
        // A key that does not fit whole is left off: `Ctrl+Sh` teaches nothing.
        let shortcut = entry
            .shortcut
            .as_deref()
            .filter(|key| used + key.chars().count() < inner_width)
            .unwrap_or_default();
        let gap = inner_width
            .saturating_sub(used + shortcut.chars().count())
            .max(1);
        lines.push(format!(
            "{marker} {category}{title}{}{shortcut}",
            " ".repeat(gap)
        ));
    }
    if visible.is_empty() {
        lines.push(format!(
            "  No command matches `{}`.",
            model.palette.query.as_str()
        ));
    }
    // One footer line, for the selected command only. The reason used to trail every
    // disabled row, which repeated "connect a session first" down the list and pushed
    // the shortcuts off the popup.
    let footer = visible
        .get(model.palette.selected)
        .and_then(|entry| entry.disabled_reason.clone())
        .unwrap_or_default();
    let list_lines = lines.len();
    lines.push(footer);
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(i, text)| {
            // Unusable commands and the footer are dim; they also sort to the bottom, so
            // the state survives a monochrome terminal.
            let dim = i == list_lines
                || (i > 0
                    && visible
                        .get(offset + i - 1)
                        .is_some_and(|entry| entry.disabled_reason.is_some()));
            if dim {
                Line::styled(text.clone(), muted)
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "Command Palette")),
        popup,
    );
    if query_fits {
        show_input(
            frame,
            crate::mouse::line_rect(popup_inner(popup), 0),
            "> ",
            &model.palette.query,
            false,
        );
    }
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, _, rect| {
        if i == 0 || i > list_lines || visible.is_empty() {
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
    let query = model.help.query.as_str();
    let mut lines = Vec::new();
    let mut any_match = false;
    for (section, rows) in model.keymap.help_sections() {
        let mut section_lines = Vec::new();
        for (chord, command) in rows {
            let title = crate::palette::command_spec(command.as_str())
                .map(|spec| spec.title)
                .unwrap_or(command.as_str());
            if !crate::palette::matches_any(&[&chord, title, &command], query) {
                continue;
            }
            // Spelled as the palette and the status bar spell keys.
            let keys = chord
                .split(" / ")
                .map(crate::palette::pretty_chord)
                .collect::<Vec<_>>()
                .join(" / ");
            section_lines.push(format!("  {keys:<16} {title}"));
        }
        if section_lines.is_empty() {
            continue;
        }
        any_match = true;
        lines.push(format!("[{section}]"));
        lines.extend(section_lines);
        lines.push(String::new());
    }
    if !any_match {
        lines.push(if query.is_empty() {
            "no bindings".into()
        } else {
            format!("no matches for '{query}'")
        });
    }
    // The search stays on the top line, a blank under it, and only the list scrolls:
    // it used to scroll away with the list on the first PageDown.
    let block = overlay_block(model, "Keybindings  Esc close");
    let [search, _, list] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(block.inner(popup));
    let max_scroll = lines.len().saturating_sub(usize::from(list.height).max(1));
    hits.set_scroll_limit(crate::mouse::ScrollArea::Help, max_scroll);
    let scroll = (model.help.scroll as usize).min(max_scroll) as u16;
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(format!("Search: {query}")), search);
    show_input(frame, search, "Search: ", &model.help.query, false);
    frame.render_widget(Paragraph::new(lines.join("\n")).scroll((scroll, 0)), list);
    register_overlay(hits, popup);
    // Over the title's own "Esc close": on the first inner row it lay over the search.
    register_label(
        hits,
        Rect::new(popup.x + 1, popup.y, popup.width.saturating_sub(2), 1),
        "Keybindings  Esc close",
        "Esc close",
        HitTarget::Button(HitButton::Close),
    );
}

fn results_menu_popup(area: Rect) -> Rect {
    let max_width = area.width.saturating_sub(4).clamp(60, 92);
    let max_height = area
        .height
        .saturating_sub(4)
        .clamp(14, area.height.saturating_sub(2).max(14));
    centered(area, max_width, max_height)
}

pub(crate) struct ResultsMenuLayout {
    pub popup: Rect,
    pub detail: Rect,
    pub actions: Rect,
}

pub(crate) fn results_menu_layout(area: Rect) -> ResultsMenuLayout {
    let popup = results_menu_popup(area);
    let outer_inner = Block::bordered().inner(popup);
    let [detail_area, actions_area] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Min(26)]).areas(outer_inner);
    ResultsMenuLayout {
        popup,
        detail: Block::bordered().title("Record").inner(detail_area),
        actions: Block::bordered().title("Actions").inner(actions_area),
    }
}

/// Screen row of the selected sidebar node, so the menu can sit against the object it
/// acts on. `None` when the sidebar is not on screen or the node scrolled out of it.
fn selected_node_row(model: &Model, area: Rect) -> Option<u16> {
    let plan = LayoutPlan::for_area_with_document_tabs(area, Some(&model.effective_panes()), true);
    let pane = plan.explorer;
    if pane.width == 0 || pane.height == 0 {
        return None;
    }
    let inner = popup_inner(pane);
    let layout = crate::widgets::object_tree::sidebar_layout(
        &model.explorer,
        model.connections.profiles.len(),
        &model.connection.name,
        (inner.height as usize).max(1),
    );
    let index = model
        .explorer
        .selected_index()
        .checked_sub(layout.offset)
        .filter(|index| *index < layout.nodes.len())?;
    let row = u16::try_from(layout.node_row(index)).ok()?;
    (row < inner.height).then(|| inner.y.saturating_add(row))
}

fn render_node_menu(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let Some(kind) = crate::update::node_menu_kind(model) else {
        return;
    };
    let entries = crate::palette::node_menu_entries(model, kind);
    if area.width < 24 || area.height < 6 || entries.is_empty() {
        return;
    }
    let width = area.width.clamp(24, NODE_MENU_WIDTH);
    let height = crate::palette::menu_height(area.height, entries.len());
    // Against the node when the sidebar shows it, centred when it does not -- the same
    // fallback the palette takes when there is no room to be clever.
    let anchor = selected_node_row(model, area);
    let x = match anchor {
        Some(_) => area.x + 2,
        None => area.x + area.width.saturating_sub(width) / 2,
    };
    let x = x.min(area.x + area.width.saturating_sub(width));
    let y = match anchor {
        // Below the row, flipped above it when that would run off the bottom.
        Some(row) if row.saturating_add(1 + height) <= area.y + area.height => row + 1,
        Some(row) if row >= area.y + height => row - height,
        _ => area.y + area.height.saturating_sub(height) / 3,
    };
    let popup = Rect::new(x, y, width, height);

    let rows = crate::palette::menu_list_rows(area.height, entries.len());
    let offset = scroll_to_selection(
        model.node_menu.selected,
        model.node_menu.offset,
        entries.len(),
        rows,
    );
    let inner_width = popup.width.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    for (index, entry) in entries.iter().enumerate().skip(offset).take(rows) {
        let marker = if index == model.node_menu.selected {
            ">"
        } else {
            " "
        };
        let shortcut = entry.shortcut.as_deref().unwrap_or_default();
        let used = 2 + entry.title.chars().count();
        let gap = inner_width
            .saturating_sub(used + shortcut.chars().count())
            .max(1);
        lines.push(format!(
            "{marker} {}{}{shortcut}",
            entry.title,
            " ".repeat(gap)
        ));
    }
    let list_lines = lines.len();
    let hidden = entries.len().saturating_sub(list_lines);
    lines.push(
        entries
            .get(model.node_menu.selected)
            .and_then(|entry| entry.disabled_reason.clone())
            .unwrap_or_else(|| {
                if hidden > 0 {
                    format!("Enter run  Esc close  +{hidden} more")
                } else {
                    "Enter run  Esc close".into()
                }
            }),
    );
    let muted = model.theme.style(Role::Muted, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let dim = i == list_lines
                || entries
                    .get(offset + i)
                    .is_some_and(|entry| entry.disabled_reason.is_some());
            if dim {
                Line::styled(text.clone(), muted)
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    let title = model
        .explorer
        .selected_node()
        .map(|node| node.label.clone())
        .unwrap_or_else(|| "Actions".into());
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, &title)),
        popup,
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, _, rect| {
        if i >= list_lines {
            return;
        }
        hits.register(HitTarget::ListRow(offset.saturating_add(i)), rect);
    });
}

fn render_results_menu(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let layout = results_menu_layout(area);
    let row = model.results.cursor_row().unwrap_or(0);
    let wrap_width = layout.detail.width.max(1) as usize;
    let detail_fields = crate::widgets::row_detail::row_detail_fields(&model.results, row);
    let detail_lines = crate::widgets::row_detail::row_detail_lines(
        &detail_fields,
        wrap_width,
        &model.theme,
        model.capabilities,
    );
    let detail_rows = layout.detail.height.max(1) as usize;
    let max_detail_offset = detail_lines.len().saturating_sub(detail_rows);
    let detail_offset = model.results_menu.offset.min(max_detail_offset);
    let visible_detail = detail_lines
        .into_iter()
        .skip(detail_offset)
        .take(detail_rows)
        .collect::<Vec<_>>();

    let items = crate::palette::results_menu_items();
    let action_rows = layout.actions.height.max(1) as usize;
    let action_offset =
        scroll_to_selection(model.results_menu.selected, 0, items.len(), action_rows);
    let mut action_lines = Vec::new();
    // Each action with its key, as the palette shows it.
    let width = layout.actions.width.saturating_sub(2) as usize;
    for (index, (id, title)) in items
        .iter()
        .enumerate()
        .skip(action_offset)
        .take(action_rows)
    {
        let marker = if index == model.results_menu.selected {
            ">"
        } else {
            " "
        };
        let label = format!("{marker} {title}");
        let key = crate::palette::shortcut_for(model, id, None).unwrap_or_default();
        let room = width.saturating_sub(unicode_width::UnicodeWidthStr::width(key.as_str()) + 1);
        action_lines.push(
            format!("{} {key}", crate::model::fit_cell(&label, room))
                .trim_end()
                .to_string(),
        );
    }
    if action_lines.is_empty() {
        action_lines.push("(empty)".into());
    }

    let title = format!("Row {}  Esc close", row + 1);
    let block = overlay_block(model, &title);
    let outer_inner = block.inner(layout.popup);
    let [detail_area, actions_area] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Min(26)]).areas(outer_inner);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(block, layout.popup);
    frame.render_widget(
        Paragraph::new(visible_detail).block(Block::bordered().title("Record")),
        detail_area,
    );
    frame.render_widget(
        Paragraph::new(action_lines.join("\n")).block(Block::bordered().title("Actions")),
        actions_area,
    );

    register_overlay(hits, layout.popup);
    for (index, _) in action_lines.iter().enumerate() {
        if action_lines[index] == "(empty)" {
            continue;
        }
        register_line(
            hits,
            layout.actions,
            index,
            HitTarget::ListRow(action_offset.saturating_add(index)),
        );
    }
}

fn render_insert_row_form(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let form = &model.data.insert_form;
    let area = frame.area();
    let lines = form.lines();
    let popup = centered(area, 60, (lines.len() as u16 + 2).max(6));
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "New row"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |index, line, rect| {
        if index < form.fields.len() {
            if index == form.focus {
                show_input(
                    frame,
                    rect,
                    &form.prefix(index),
                    &form.fields[index].value,
                    false,
                );
            }
            hits.register(HitTarget::FormField(index), rect);
        } else if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Insert");
        }
    });
}

fn render_cell_edit(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let Some(form) = &model.data.cell_edit else {
        return;
    };
    let area = frame.area();
    let lines = form.lines();
    let popup = centered(area, 72, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Edit cell"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |index, line, rect| match index {
        1 => {
            if form.focus == crate::screens::data::CellFocus::Value {
                show_input(frame, rect, "> value: ", &form.value, false);
            }
            hits.register(HitTarget::FormField(0), rect);
        }
        3 => {
            register_label(hits, rect, line, "[Save]", HitTarget::FooterSubmit);
            register_label(hits, rect, line, "[Cancel]", HitTarget::FooterCancel);
            register_label(
                hits,
                rect,
                line,
                "[NULL]",
                HitTarget::Button(HitButton::SetNull),
            );
            register_label(
                hits,
                rect,
                line,
                "[Editor]",
                HitTarget::Button(HitButton::OpenEditor),
            );
        }
        _ => {}
    });
}

fn render_review(
    frame: &mut Frame,
    model: &Model,
    review: &crate::screens::data::ReviewModal,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let width = area.width.clamp(10, 96);
    let view = crate::screens::data::review_view(review, usize::from(width.saturating_sub(2)));
    // The header, a blank line, the statements, then a hint and the buttons: only the
    // statements scroll.
    let chrome = view.header.len() + 3;
    let wanted = chrome + view.body.len() + 2;
    let height = u16::try_from(wanted.min(usize::from(area.height.saturating_sub(2)).max(8)))
        .unwrap_or(u16::MAX);
    let popup = centered(area, width, height);
    let body_rows = usize::from(popup_inner(popup).height)
        .saturating_sub(chrome)
        .max(1);
    let max_scroll = view.body.len().saturating_sub(body_rows);
    hits.set_scroll_limit(crate::mouse::ScrollArea::Review, max_scroll);
    let top = usize::from(review.scroll).min(max_scroll);
    let mut lines = view.header.clone();
    lines.push(String::new());
    let mut shown: Vec<String> = view
        .body
        .iter()
        .skip(top)
        .take(body_rows)
        .cloned()
        .collect();
    shown.resize(body_rows, String::new());
    lines.extend(shown);
    lines.push(if max_scroll == 0 {
        "d discard all changes  Esc cancel: they stay pending".to_string()
    } else {
        format!(
            "PageUp/PageDown scroll ({}-{} of {})  d discard all  Esc cancel",
            top + 1,
            (top + body_rows).min(view.body.len()),
            view.body.len()
        )
    });
    lines.push(crate::widgets::form::footer_line("Apply", review.footer));
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Review changes"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let last = lines.len() - 1;
    for_popup_lines(popup, &lines, |index, line, rect| {
        if index == last {
            crate::widgets::form::register_footer(hits, rect, line, "Apply");
        } else if index + 1 == last {
            register_label(
                hits,
                rect,
                line,
                "d discard all",
                HitTarget::Button(HitButton::Discard),
            );
        }
    });
}

fn render_ddl_preview(
    frame: &mut Frame,
    model: &Model,
    preview: &crate::screens::schema_editor::DdlPreviewState,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let frame_popup = centered(area, 72, area.height.saturating_sub(2).min(20));
    let inner = popup_inner(frame_popup);
    let (lines, max_scroll) =
        crate::modals::preview_lines(preview, usize::from(inner.height), usize::from(inner.width));
    hits.set_scroll_limit(crate::mouse::ScrollArea::DdlPreview, max_scroll);
    let popup = centered(area, 72, lines.len() as u16 + 2);
    let title = if model.connection.name.is_empty() {
        "DDL preview".to_string()
    } else {
        format!("DDL preview · {}", model.connection.name)
    };
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, &title),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let typing = preview.footer == crate::widgets::form::FooterFocus::Input;
    for_popup_lines(popup, &lines, |_, line, rect| {
        if preview.needs_typing() && line.starts_with("name: ") {
            paint_selection(frame, rect, "name: ", &preview.typed, typing);
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Apply");
        }
    });
}

fn centered(area: Rect, max_width: u16, max_height: u16) -> Rect {
    // At least 10x6 when the screen has it, and never more than the screen. `clamp`
    // panicked on a popup asking for fewer rows than the floor.
    let width = area.width.min(max_width.max(10));
    let height = area.height.min(max_height.max(6));
    let x = area.x + area.width.saturating_sub(width) / 2;
    // Every dialog starts on the same row, whatever it holds: they sat at rows 1 to 11
    // depending on their height. A tall one still moves up to fit.
    let y = area.y + (area.height / 6).min(area.height.saturating_sub(height));
    Rect::new(x, y, width, height)
}

fn render_schema_diff(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let diff = &model.schema_diff;
    let popup = centered(area, 88, area.height.saturating_sub(2).min(24));
    let inner = popup_inner(popup);
    let (body, selected_line, entries_from) = diff.body(usize::from(inner.width));
    // Two rows keep the bottom: the buttons and what the keys do.
    let room = usize::from(inner.height).saturating_sub(2).max(1);
    let offset = scroll_to_selection(selected_line.unwrap_or(0), 0, body.len(), room);
    let mut lines: Vec<String> = body.iter().skip(offset).take(room).cloned().collect();
    let footer_row = lines.len();
    let footer = crate::widgets::form::footer_line(diff.submit_label(), diff.footer);
    lines.push(footer.clone());
    lines.push(crate::model::truncate_cell(
        if diff.source_prompt {
            "  Left/Right change a source  Enter compare  Esc cancel"
        } else {
            "  a/r/c filter  Enter open the script  Esc close"
        },
        usize::from(inner.width),
    ));
    let title = if diff.source_prompt {
        "Compare Schema".to_string()
    } else {
        format!("Compare Schema \u{b7} {}", diff.from_label)
    };
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, &title),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let shown = filter_count(diff);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == footer_row {
            crate::widgets::form::register_footer(hits, rect, &footer, diff.submit_label());
            return;
        }
        let at = offset + i;
        if diff.source_prompt {
            if (2..=4).contains(&at) {
                hits.register(HitTarget::FormField(at - 2), rect);
                if at == 4 && diff.uses_file() && diff.row == 2 {
                    paint_selection(frame, rect, "> File: ", &diff.file, true);
                }
            }
            return;
        }
        if line.starts_with("Show:") {
            for (needle, button) in [
                ("[x] added", HitButton::ToggleAdded),
                ("[ ] added", HitButton::ToggleAdded),
                ("[x] removed", HitButton::ToggleRemoved),
                ("[ ] removed", HitButton::ToggleRemoved),
                ("[x] changed", HitButton::ToggleChanged),
                ("[ ] changed", HitButton::ToggleChanged),
            ] {
                register_label(hits, rect, line, needle, HitTarget::Button(button));
            }
        } else if at >= entries_from && at < entries_from + shown {
            hits.register(HitTarget::ListRow(at - entries_from), rect);
        }
    });
}

/// How many differences the filters let through.
fn filter_count(diff: &crate::screens::schema_diff::SchemaDiffScreen) -> usize {
    diff.filtered().len()
}

fn render_transfer(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    use crate::screens::transfer::{BROWSE, FIELD_HEAD, TransferField, TransferLineKind};
    let transfer = &model.transfer;
    let inner_width = popup_inner(centered(area, 72, 16)).width as usize;
    let lines = transfer.layout(inner_width);
    let caret = transfer.caret(inner_width);
    // As tall as what it says, up to the most a terminal can spare.
    let popup = centered(area, 72, (lines.len() as u16 + 2).min(16));
    let (body, footer) = lines.split_at(lines.len().saturating_sub(1));
    let body_rows = popup_inner(popup).height.saturating_sub(1) as usize;
    let offset = transfer.scroll.min(body.len().saturating_sub(body_rows));
    let mut visible = body
        .iter()
        .skip(offset)
        .take(body_rows)
        .cloned()
        .collect::<Vec<_>>();
    visible.extend(footer.iter().cloned());
    let texts = visible
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(transfer.title()),
        texts.join("\n"),
    );
    register_overlay(hits, popup);
    let fields = transfer.fields();
    for_popup_lines(popup, &texts, |i, line, rect| match visible[i].kind {
        TransferLineKind::Field(index) => {
            hits.register(HitTarget::FormField(index), rect);
            let Some(field) = fields.get(index).copied() else {
                return;
            };
            if field == TransferField::File {
                register_label(
                    hits,
                    rect,
                    line,
                    BROWSE.trim_start(),
                    HitTarget::Button(HitButton::Browse),
                );
            }
            // The terminal's cursor is where the focused field's text has it, and a
            // selection shows in reverse, as in every input.
            if let Some(caret) = caret.filter(|caret| caret.field == index) {
                if caret.selected > 0 {
                    paint_reversed(frame, rect, FIELD_HEAD, caret.selected);
                }
                if caret.column < usize::from(rect.width) {
                    frame.set_cursor_position(ratatui::layout::Position::new(
                        rect.x + caret.column as u16,
                        rect.y,
                    ));
                }
            }
        }
        TransferLineKind::Footer => {
            crate::widgets::form::register_footer(hits, rect, line, transfer.submit_label());
        }
        TransferLineKind::Text => {}
    });
}

fn render_security(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let width = 84.min(area.width.saturating_sub(2));
    let popup = centered(area, width, 14.min(area.height.saturating_sub(2)));
    let target = model.data.target.display_unquoted();
    let lines = model
        .security
        .lines(popup_inner(popup).width as usize, &target);
    let rows = popup_inner(popup).height as usize;
    // The hint stays at the bottom, whatever the roles and grants take.
    let (hint, list) = lines.split_at(lines.len().saturating_sub(1));
    let room = rows.saturating_sub(1);
    let offset = scroll_to_selection(
        model.security.selected,
        0,
        model.security.principals.len(),
        room,
    );
    let mut visible = list
        .iter()
        .skip(offset)
        .take(room)
        .cloned()
        .collect::<Vec<_>>();
    visible.resize(room, String::new());
    visible.extend(hint.iter().cloned());
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(format!("Security on {}", model.connection.name)),
        visible.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &visible, |i, _, rect| {
        let source_index = offset + i;
        if i < room && source_index < model.security.principals.len() {
            hits.register(HitTarget::ListRow(source_index), rect);
        }
    });
}

fn render_admin(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let width = 110.min(area.width.saturating_sub(2));
    let rows = model.admin.visible_rows(area.height);
    let lines = model.admin.lines(width.saturating_sub(2) as usize, rows);
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let popup = centered(area, width, height);
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(format!(
            "Sessions on {} (every database on the server)",
            model.connection.name
        )),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    // Row 0 is the header; each session's row below it picks that session, from the
    // first one the list has scrolled to.
    let start = model.admin.window_start(rows);
    let shown = model.admin.sessions.len().saturating_sub(start).min(rows);
    for_popup_lines(popup, &lines, |index, _, rect| {
        if index >= 1 && index <= shown {
            hits.register(HitTarget::ListRow(start + index - 1), rect);
            if start + index - 1 == model.admin.selected {
                paint_reversed(frame, rect, 0, rect.width as usize);
            }
        }
    });
    if let Some(prompt) = &model.admin.terminate {
        let width = 72.min(area.width);
        let lines = prompt.lines(width.saturating_sub(2) as usize);
        let popup = centered(area, width, lines.len() as u16 + 2);
        paint_popup(
            frame,
            model,
            popup,
            Block::bordered().title("Terminate session"),
            lines.join("\n"),
        );
        register_overlay(hits, popup);
        for_popup_lines(popup, &lines, |_, line, rect| {
            if line.starts_with("id:") {
                let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
                paint_selection(frame, rect, "id: ", &prompt.typed, focused);
                hits.register(HitTarget::FormField(0), rect);
            }
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Terminate");
            }
        });
    }
}

fn render_mcp_profiles(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let screen = &model.mcp_profiles;
    if let Some(form) = &screen.grant_form {
        render_grant_form(frame, model, form, hits);
        return;
    }
    if let Some(confirm) = &screen.confirm {
        let lines = confirm.lines(&screen.connections);
        let popup = centered(area, 84, lines.len() as u16 + 2);
        paint_popup(
            frame,
            model,
            popup,
            overlay_block(model, confirm.title()),
            lines.join("\n"),
        );
        register_overlay(hits, popup);
        for_popup_lines(popup, &lines, |_, line, rect| {
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, confirm.submit_label());
            }
        });
        return;
    }
    let width = area.width.min(100);
    let height = area.height.saturating_sub(2).clamp(8, 26);
    let popup = centered(area, width, height);
    let inner = popup_inner(popup);
    let view = screen.view(inner.width as usize, inner.height as usize);
    // The picked row in reverse video, so it reads at a glance and without colour.
    let body: Vec<Line> = view
        .lines
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if view.picked == Some(index) {
                Line::styled(
                    text.clone(),
                    Style::default().add_modifier(Modifier::REVERSED),
                )
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "MCP profiles")),
        popup,
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &view.lines, |i, _, rect| {
        if let Some((_, profile)) = view.rows.iter().find(|(line, _)| *line == i) {
            hits.register(HitTarget::ListRow(*profile), rect);
        }
    });
}

/// New MCP Grant, over MCP profiles.
fn render_grant_form(
    frame: &mut Frame,
    model: &Model,
    form: &crate::screens::mcp_profiles::GrantForm,
    hits: &mut HitMap,
) {
    let area = frame.area();
    let lines = form.lines();
    let popup = centered(area, 92, (lines.len() as u16 + 2).min(area.height));
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "New MCP grant"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    // The rows drawn are the fields that are shown, from the second line.
    let mut shown: Vec<usize> = (0..form.fields.len())
        .filter(|index| *index != crate::screens::mcp_profiles::GRANT_ASK_SECS || form.ask)
        .collect();
    shown.truncate(form.fields.len());
    for_popup_lines(popup, &lines, |index, line, rect| {
        if let Some(field) = index.checked_sub(1).and_then(|row| shown.get(row)).copied()
            && index <= shown.len()
        {
            if field == form.focus
                && !form.is_choice(field)
                && field != crate::screens::mcp_profiles::GRANT_ASK
            {
                show_form_field(frame, rect, &form.fields[field]);
            }
            hits.register(HitTarget::FormField(field), rect);
            if form.is_choice(field) {
                for (needle, step) in [("< ", -1), (" >", 1)] {
                    crate::mouse::register_label(
                        hits,
                        rect,
                        line,
                        needle,
                        HitTarget::FormChoice { index: field, step },
                    );
                }
            }
        } else if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Create");
        }
    });
}

/// The saved connections, sized to them: the list scrolls to keep the selection in
/// view once there are more than fit, and the hints stay on screen underneath.
fn render_connections(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let screen = &model.connections;
    let listed = screen.rows(model.active_session);
    // The popup is 72 wide at most, 70 inside its borders.
    let footer = screen.footer_lines((area.width.min(72) as usize).saturating_sub(2));
    // Borders, the blank line above the hints, and a row of air above and below.
    let chrome = 2 + 1 + footer.len();
    let room = (area.height as usize).saturating_sub(chrome + 2).max(1);
    let rows = listed.len().clamp(1, room);
    // Scrolled by line, so the Docker heading scrolls with its rows.
    let selected_line = listed
        .iter()
        .position(|(row, _)| *row == Some(screen.selected_profile))
        .unwrap_or(0);
    let offset = scroll_to_selection(selected_line, 0, listed.len(), rows);
    let shown: Vec<(Option<usize>, String)> = listed.into_iter().skip(offset).take(rows).collect();
    let mut lines: Vec<String> = shown.iter().map(|(_, line)| line.clone()).collect();
    lines.push(String::new());
    lines.extend(footer);
    let popup = centered(area, 72, (lines.len() + 2) as u16);
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Connections"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let buttons = [
        HitButton::Connect,
        HitButton::New,
        HitButton::Edit,
        HitButton::Duplicate,
        HitButton::Test,
        HitButton::Delete,
        HitButton::CloseSession,
        HitButton::Docker,
    ];
    for_popup_lines(popup, &lines, |i, line, rect| {
        if let Some((Some(row), _)) = shown.get(i).filter(|_| i < rows) {
            hits.register(HitTarget::ListRow(*row), rect);
        }
        if i > rows {
            for (label, button) in crate::screens::connections::HINTS.iter().zip(buttons) {
                register_label(hits, rect, line, label, HitTarget::Button(button));
            }
        }
    });
}

/// "Delete connection", drawn like "Unsaved changes": sized to its text, the focused
/// button marked with `>` so it reads without colour, Cancel focused first.
fn render_delete_connection(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::screens::connections::DeleteChoice;
    let area = frame.area();
    let Some(target) = &model.connections.delete_target else {
        return;
    };
    if area.width < 20 || area.height < 7 {
        return;
    }
    let mut name: String = target.name.chars().take(40).collect();
    if target.name.chars().count() > 40 {
        name.push('…');
    }
    let buttons = [
        (DeleteChoice::Delete, "[Delete]", HitButton::ConfirmDelete),
        (DeleteChoice::Cancel, "[Cancel]", HitButton::Cancel),
    ];
    let footer: String = buttons
        .iter()
        .map(|(choice, label, _)| {
            let marker = if *choice == model.connections.delete_choice {
                ">"
            } else {
                " "
            };
            format!("{marker}{label}")
        })
        .collect::<Vec<_>>()
        .join("  ");
    let mut notes = vec![
        "Its saved password is removed as well.".to_string(),
        "So are its saved queries and notes.".to_string(),
    ];
    if model.connections.session_for(&target.name).is_some() {
        notes.push("Its open session is closed.".to_string());
    }
    // Its documents stay open, and run on whatever connection is active afterwards.
    let id = target.id.0.to_string();
    let open = model
        .documents
        .iter()
        .filter(|document| document.connection_id.as_deref() == Some(id.as_str()))
        .count();
    if open > 0 {
        notes.push(format!(
            "{open} open document{} lose{} it and run on the active connection.",
            if open == 1 { "" } else { "s" },
            if open == 1 { "s" } else { "" }
        ));
    }
    let mut lines = vec![format!("Delete \"{name}\"? This cannot be undone.")];
    lines.extend(notes.iter().cloned());
    lines.push(String::new());
    lines.push(footer.clone());
    let content_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let width = (content_width as u16 + 4).min(area.width);
    let popup = centered(area, width, lines.len() as u16 + 2);
    let warning = model.theme.style(Role::Warning, model.capabilities);
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if (1..=notes.len()).contains(&index) {
                Line::styled(text.clone(), warning)
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "Delete connection")),
        popup,
    );
    register_overlay(hits, popup);
    let footer_row = crate::mouse::line_rect(popup_inner(popup), lines.len() - 1);
    for (_, label, button) in buttons {
        register_label(hits, footer_row, &footer, label, HitTarget::Button(button));
    }
}

fn render_projects(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::screens::projects::ProjectsMode;
    let area = frame.area();
    let projects = &model.projects;
    let unsaved = crate::update::unsaved_titles(model);
    // The list gives way before the buttons and hints under it do.
    let room = (area.height as usize).saturating_sub(8).clamp(1, 14);
    let lines = projects.lines(&model.project, &unsaved, room);
    let popup = centered(
        area,
        72,
        (lines.len() + 2).min(area.height as usize).max(6) as u16,
    );
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, projects.title()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let unsaved_question = projects.asking_about_unsaved();
    let deleting = projects.delete.is_some();
    let browsing = !unsaved_question
        && !deleting
        && !matches!(projects.mode, ProjectsMode::Create | ProjectsMode::Rename);
    let offset = projects.list_offset(room);
    let shown = projects.list.len().saturating_sub(offset).min(room.max(1));
    for_popup_lines(popup, &lines, |i, line, rect| {
        if unsaved_question {
            for (needle, button) in [
                ("[Save]", HitButton::Confirm),
                ("[Don't save]", HitButton::Discard),
                ("[Cancel]", HitButton::Cancel),
            ] {
                register_label(hits, rect, line, needle, HitTarget::Button(button));
            }
            return;
        }
        if browsing && i < shown {
            hits.register(HitTarget::ListRow(offset + i), rect);
        }
        if let Some(name) = line
            .strip_prefix("> name: ")
            .or_else(|| line.strip_prefix("  name: "))
        {
            let _ = name;
            if projects.footer == crate::widgets::form::FooterFocus::Input {
                let input = match &projects.delete {
                    Some(delete) => &delete.typed,
                    None => &projects.name_input,
                };
                show_input(frame, rect, "> name: ", input, false);
            }
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            let submit = if deleting { "Delete" } else { "Submit" };
            crate::widgets::form::register_footer(hits, rect, line, submit);
        }
        if deleting && line.contains("Alt+C") {
            hits.register(HitTarget::Button(HitButton::ToggleConnections), rect);
        }
    });
}

fn render_config_transfer(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::screens::config_transfer::ConfigStage;
    let area = frame.area();
    let screen = &model.config_transfer;
    let width = area.width.min(78);
    let cap = area.height.saturating_sub(2).clamp(6, 26);
    let inner_width = width.saturating_sub(2) as usize;
    let view = screen.view(inner_width, cap.saturating_sub(2) as usize);
    // The preview scrolls and takes the room; the other steps are as tall as they are.
    let height = if screen.stage() == ConfigStage::Preview {
        cap
    } else {
        (view.lines.len() as u16 + 2).min(cap)
    };
    let popup = centered(area, width, height);
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Config transfer"),
        view.lines.join("\n"),
    );
    register_overlay(hits, popup);
    // The buttons are the second line from the bottom of every step.
    let button_line = view.lines.len().saturating_sub(2);
    for_popup_lines(popup, &view.lines, |i, line, rect| {
        if let Some((_, conflict)) = view.conflict_rows.iter().find(|(row, _)| *row == i) {
            hits.register(HitTarget::ListRow(*conflict), rect);
        }
        if i != button_line {
            return;
        }
        for label in screen.buttons() {
            let button = match *label {
                "Export" => HitButton::Export,
                "Import" => HitButton::Apply,
                "Close" => HitButton::Close,
                _ => HitButton::Cancel,
            };
            register_label(
                hits,
                rect,
                line,
                &format!("[{label}]"),
                HitTarget::Button(button),
            );
        }
    });
}

fn render_secret(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let lines = model.secret_prompt.lines();
    let popup = centered(frame.area(), 70, (lines.len() as u16 + 2).max(8));
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title("Secret"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let prompt = &model.secret_prompt;
    for_popup_lines(popup, &lines, |i, line, rect| {
        // The second line is the secret's, its marks after the label's `: `.
        if i == 1 && prompt.footer == crate::widgets::form::FooterFocus::Input {
            let before = line.find(": ").map_or(line, |at| &line[..at + 2]);
            show_input(frame, rect, before, prompt.buffer.input(), true);
        }
        register_label(
            hits,
            rect,
            line,
            "save to the keychain",
            HitTarget::Button(HitButton::Keychain),
        );
        crate::widgets::form::register_footer(hits, rect, line, "Submit");
    });
}

fn render_transaction_prompt(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let prompt = &model.transaction_prompt;
    let lines = prompt.lines();
    // As tall as what it says: the form used to leave rows of empty box under its buttons.
    let popup = centered(frame.area(), 56, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(prompt.title()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            if prompt.footer == crate::widgets::form::FooterFocus::Input {
                show_input(frame, rect, "name: ", &prompt.name, false);
            }
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, prompt.submit_label());
        }
    });
}

/// Open Saved Query: the search on top, the queries on the left with the highlighted
/// one's SQL beside them, and what the keys do at the bottom.
fn render_saved_queries(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::widgets::form::footer_line;
    let picker = &model.saved_queries;
    let area = frame.area();
    let popup = centered(area, 100, area.height.saturating_sub(2).min(22));
    let inner = popup_inner(popup);
    let width = inner.width as usize;
    let list_width = (width * 2 / 5).clamp(16, 40);
    let rows = (inner.height as usize).saturating_sub(3).max(1);
    let filtered = picker.filtered();
    let offset = scroll_to_selection(picker.selected, 0, filtered.len(), rows);
    // The connection a save from here would use, so the owner marks agree with Save.
    let current_connection = crate::update::query_connection(model);
    let preview: Vec<&str> = picker
        .current()
        .map(|query| query.sql.lines().collect())
        .unwrap_or_default();
    let mut lines =
        vec![
            picker
                .search
                .inline_line_within("search: ", picker.renaming.is_none(), width),
        ];
    // Nothing to list: the message gets the whole width, not the list's column.
    let note = if picker.items.is_none() {
        Some("  Reading the saved queries…".to_string())
    } else if picker.items.as_ref().is_some_and(Vec::is_empty) {
        Some(
            match crate::palette::shortcut_for(model, "editor.save_query", None) {
                Some(key) => format!("  No saved queries yet; {key} saves one from the editor."),
                None => "  No saved queries yet; Save Query As saves one.".into(),
            },
        )
    } else if filtered.is_empty() {
        Some("  No saved query matches.".into())
    } else {
        None
    };
    for row in 0..rows {
        let left = match filtered.get(offset + row) {
            Some(query) => {
                let index = offset + row;
                let marker = if index == picker.selected { ">" } else { " " };
                let name = match (&picker.renaming, index == picker.selected) {
                    (Some(input), true) => {
                        input.inline_line_within("", true, list_width.saturating_sub(2))
                    }
                    _ => query.name.clone(),
                };
                // Another connection's query says whose it is.
                let owner = (current_connection.as_deref() != Some(query.connection_id.as_str()))
                    .then(|| {
                        model
                            .connections
                            .profiles
                            .iter()
                            .find(|row| row.profile.id.0.to_string() == query.connection_id)
                            .map_or("another connection".to_string(), |row| {
                                row.profile.name.clone()
                            })
                    });
                match owner {
                    Some(owner) => format!("{marker} {name} · {owner}"),
                    None => format!("{marker} {name}"),
                }
            }
            None => {
                if row == 0
                    && let Some(note) = &note
                {
                    lines.push(crate::model::truncate_cell(note, width));
                    continue;
                }
                String::new()
            }
        };
        let right = preview.get(row).copied().unwrap_or("");
        lines.push(format!(
            "{} │ {}",
            crate::model::fit_cell(&left, list_width),
            crate::model::truncate_cell(right, width.saturating_sub(list_width + 3)),
        ));
    }
    let footer = match (&picker.deleting, &picker.error) {
        (Some(focus), _) => {
            let name = picker.current().map_or("", |query| query.name.as_str());
            lines.push(format!("Delete {name}?"));
            footer_line("Delete", *focus)
        }
        (None, Some(error)) => error.clone(),
        (None, None) if picker.renaming.is_some() => "Enter rename  Esc keep the name".into(),
        (None, None) => "Enter open  F2 rename  Delete delete  Esc close".into(),
    };
    lines.push(footer);
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Open saved query"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == 0 {
            paint_selection(
                frame,
                rect,
                "search: ",
                &picker.search,
                picker.renaming.is_none(),
            );
        }
        if (1..=rows).contains(&i) && offset + i - 1 < filtered.len() {
            let list = Rect {
                width: (list_width as u16).min(rect.width),
                ..rect
            };
            hits.register(HitTarget::ListRow(offset + i - 1), list);
            if let Some(input) = &picker.renaming
                && offset + i - 1 == picker.selected
            {
                paint_selection(frame, list, "> ", input, true);
            }
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Delete");
        }
    });
}

fn render_document_name_prompt(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let width = 56.min(area.width);
    // A long name scrolls inside the field; the box is as tall as what it holds.
    let lines = model
        .document_name_prompt
        .lines_within(usize::from(width.saturating_sub(2)));
    let popup = centered(area, width, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, model.document_name_prompt.title()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let prompt = &model.document_name_prompt;
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            let focused = prompt.footer == crate::widgets::form::FooterFocus::Input;
            paint_selection(frame, rect, "name: ", &prompt.name, focused);
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, prompt.submit_label());
        }
    });
}

fn render_connection_form(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    // As tall as its fields, up to what the screen has: a short form was a short list in
    // a tall box.
    let wanted = model.connection_form.content_rows() as u16 + 2;
    let popup = centered(area, 72, area.height.saturating_sub(2).min(22).min(wanted));
    let rows = popup.height.saturating_sub(2).max(4) as usize;
    let visible = model
        .connection_form
        .visible_rows(rows, popup_inner(popup).width as usize);
    let lines: Vec<String> = visible.iter().map(|(_, line)| line.clone()).collect();
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, model.connection_form.title()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    let footer = lines.len().saturating_sub(1);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == footer {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
            crate::mouse::register_label(
                hits,
                rect,
                line,
                "[Test]",
                HitTarget::Button(HitButton::Test),
            );
            return;
        }
        // The status rows above the buttons are text, not fields: a click there is
        // nothing, where it used to focus whichever field sat at that row's index.
        let Some(index) = visible.get(i).and_then(|(field, _)| *field) else {
            return;
        };
        if line.contains("Advanced options") {
            hits.register(HitTarget::Button(HitButton::ToggleAdvanced), rect);
            return;
        }
        let form = &model.connection_form;
        if index == form.focus
            && !form.is_choice_at(index)
            && let Some(field) = form.fields.get(index)
        {
            show_form_field(frame, rect, field);
        }
        hits.register(HitTarget::FormField(index), rect);
        if form.is_choice_at(index) {
            for (needle, step) in [("< ", -1), (" >", 1)] {
                crate::mouse::register_label(
                    hits,
                    rect,
                    line,
                    needle,
                    HitTarget::FormChoice { index, step },
                );
            }
        }
    });
}

fn render_settings(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    // The eight rows, a blank line, the reset button and the hint, inside the borders.
    let popup = centered(frame.area(), 64, 13);
    let wide = popup_inner(popup).width >= crate::screens::settings::WIDE_MIN_WIDTH;
    let lines = model.settings.lines(wide);
    let body = if wide {
        settings_option_lines(model)
    } else {
        lines.iter().map(|line| Line::from(line.clone())).collect()
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(body).block(overlay_block(model, "Settings")),
        popup,
    );
    register_overlay(hits, popup);
    let fields = model.settings.options();
    for_popup_lines(popup, &lines, |index, line, rect| {
        if index < crate::screens::settings::FIELD_COUNT {
            hits.register(HitTarget::ListRow(index), rect);
            // Side by side, each value is a click of its own: the row used to step to the
            // next value wherever it was clicked, so Rose could not be picked.
            if wide
                && let Some(field) = fields.get(index)
                && field.values.len() > 1
            {
                // The marker and the label come first, as `settings_option_lines` lays out.
                let mut x = rect.x + 2 + 11;
                for (choice, value) in field.values.iter().enumerate() {
                    let width = value.chars().count() as u16 + 3;
                    let cell = Rect::new(x, rect.y, width, 1).intersection(rect);
                    hits.register(
                        HitTarget::SettingsChoice {
                            row: index,
                            index: choice,
                        },
                        cell,
                    );
                    x += width;
                }
            }
        } else if line.contains('[') {
            hits.register(HitTarget::Button(HitButton::Reset), rect);
        }
    });
}

/// Every choice stays on screen. The active one carries brackets, weight and color at
/// once, so it still reads when the terminal has no color to give.
fn settings_option_lines(model: &Model) -> Vec<Line<'static>> {
    let caps = model.capabilities;
    let muted = model.theme.style(Role::Muted, caps);
    let focus = model.theme.style(Role::Focus, caps);
    let settings = &model.settings;
    let mut body: Vec<Line> = Vec::new();

    for (row, field) in settings.options().into_iter().enumerate() {
        let focused = row == settings.focus;
        let marker = if focused { "> " } else { "  " };
        let mut spans = vec![
            Span::styled(marker.to_string(), focus),
            Span::styled(
                format!("{:<11}", field.label),
                if focused { focus } else { muted },
            ),
        ];
        for (index, value) in field.values.iter().enumerate() {
            let active = index == field.active;
            let tint = field
                .tint
                .then(|| crate::theme::accent_color(crate::theme::ACCENTS[index].0, caps))
                .flatten();
            let style = match (active, tint) {
                (true, Some(color)) => Style::default().fg(color).add_modifier(Modifier::BOLD),
                (true, None) => focus.add_modifier(Modifier::BOLD),
                (false, Some(color)) => Style::default().fg(color).add_modifier(Modifier::DIM),
                (false, None) => muted,
            };
            // Same width either way, so the row does not shift as the value changes.
            let text = if active {
                format!("[{value}] ")
            } else {
                format!(" {value}  ")
            };
            spans.push(Span::styled(text, style));
        }
        body.push(Line::from(spans));
    }

    body.push(Line::default());
    let reset_focused = settings.focus == crate::screens::settings::RESET_FOCUS;
    body.push(Line::from(Span::styled(
        settings.footer_line(),
        if reset_focused { focus } else { muted },
    )));
    body.push(Line::from(Span::styled(
        crate::screens::settings::SettingsScreen::hint(true).to_string(),
        muted,
    )));
    body
}

/// Object metadata belongs to the tree selection, not to the open document, so it is an
/// overlay rather than a workbench tab -- the shape dbx uses for `DdlViewDialog`.
fn render_object_overlay(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::screens::object_inspector::InspectorFacet;

    let area = frame.area();
    let popup = centered(area, 84, area.height.saturating_sub(2).min(24));
    let (title, body) = match model.inspector.facet {
        InspectorFacet::Ddl => ("DDL", ddl_overlay_body(model)),
        InspectorFacet::Properties => ("Properties", properties_tab_body(model)),
    };
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    lines.push(String::new());
    match &model.inspector.editing_note {
        Some((input, focus)) => {
            lines.push(
                input.inline_line("note: ", *focus == crate::widgets::form::FooterFocus::Input),
            );
            lines.push(crate::widgets::form::footer_line("Save", *focus));
        }
        None if model.inspector.object.is_some() => {
            lines.push("  Up/Down scroll  n note  Esc close".into())
        }
        None => lines.push("  Up/Down scroll  Esc close".into()),
    }
    let max_scroll = lines
        .len()
        .saturating_sub((popup.height.saturating_sub(2) as usize).max(1));
    hits.set_scroll_limit(crate::mouse::ScrollArea::Inspector, max_scroll);
    // The note editor is the last two lines: while it is open the view stays at the end,
    // where it is. Under a long DDL it opened below the popup and the note was typed blind.
    let scroll = if model.inspector.editing_note.is_some() {
        max_scroll
    } else {
        (model.inspector.scroll as usize).min(max_scroll)
    } as u16;
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .scroll((scroll, 0))
            .block(overlay_block(model, title)),
        popup,
    );
    register_overlay(hits, popup);
    // [Save] and [Cancel] answer a click like every other dialog's.
    if let Some((input, focus)) = &model.inspector.editing_note {
        let inner = crate::mouse::popup_inner(popup);
        let focused = *focus == crate::widgets::form::FooterFocus::Input;
        if let Some(row) = (lines.len() - 2).checked_sub(scroll as usize)
            && row < inner.height as usize
        {
            let line = crate::mouse::line_rect(inner, row);
            paint_selection(frame, line, "note: ", input, focused);
        }
        let footer = lines.len() - 1;
        if let Some(row) = footer.checked_sub(scroll as usize)
            && row < inner.height as usize
        {
            crate::widgets::form::register_footer(
                hits,
                crate::mouse::line_rect(inner, row),
                &lines[footer],
                "Save",
            );
        }
    }
}

fn ddl_overlay_body(model: &Model) -> String {
    match &model.inspector.ddl {
        Some(ddl) if model.inspector.qualified_name.is_empty() => ddl.clone(),
        Some(ddl) => format!("{}\n\n{ddl}", model.inspector.qualified_name),
        None => "DDL is not available for this object.".into(),
    }
}

fn render_schema_form(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let popup = centered(area, 76, area.height.saturating_sub(2).min(20));
    let editor = &model.schema_editor;
    // The buttons keep the bottom of the popup: raw SQL a page long would push them out.
    let inner = popup_inner(popup);
    let fields = editor.lines_within(usize::from(inner.width));
    let room = usize::from(inner.height.saturating_sub(2));
    let body: Vec<String> = fields
        .iter()
        .flat_map(|line| line.split('\n').map(str::to_string))
        .chain(std::iter::once(String::new()))
        .collect();
    // The body scrolls to keep the focused field in view, one line per field under the
    // heading. On a short screen the focus went to fields cut off below the buttons, and
    // what was typed landed where nobody could see it.
    let focus_line = (editor.footer == crate::widgets::form::FooterFocus::Input
        && !editor.is_raw())
    .then_some(editor.focus + 1);
    let offset = focus_line.map_or(0, |line| scroll_to_selection(line, 0, body.len(), room));
    let mut lines: Vec<String> = body.into_iter().skip(offset).take(room).collect();
    let submit = editor.submit_label();
    let footer = crate::widgets::form::footer_line(submit, editor.footer);
    let footer_index = lines.len();
    lines.push(footer.clone());
    lines.push(format!(
        "  Tab/arrows move  Enter {}  Esc cancel",
        submit.to_lowercase()
    ));
    let title = match (editor.is_raw(), model.connection.name.as_str()) {
        (true, "") => "Apply Raw DDL".to_string(),
        (true, name) => format!("Apply Raw DDL · {name}"),
        (false, "") => "Schema".to_string(),
        (false, name) => format!("Schema · {name}"),
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(overlay_block(model, &title)),
        popup,
    );
    register_overlay(hits, popup);
    // A click picks a field, on the rows the fields are drawn on and no others: the
    // footer and the hint are not fields cut off underneath them.
    let shown = offset..offset + footer_index;
    if !editor.is_raw() {
        for line in shown
            .clone()
            .filter(|line| (1..=editor.fields.len()).contains(line))
        {
            register_line(hits, inner, line - offset, HitTarget::FormField(line - 1));
        }
    }
    if let Some(line) = focus_line.filter(|line| shown.contains(line))
        && let Some(field) = editor.fields.get(editor.focus)
    {
        show_windowed_input(
            frame,
            crate::mouse::line_rect(inner, line - offset),
            &format!("> {}: ", field.label),
            &field.value,
        );
    }
    let footer_row = crate::mouse::line_rect(inner, footer_index);
    crate::widgets::form::register_footer(hits, footer_row, &footer, submit);
}

/// Messages used to be appended to the status bar, which is the one place a user never
/// looks after acting. This lands where the eye already is, and gets out of the way: it
/// ages out, and Esc clears it sooner. The Messages view keeps every one in full.
fn render_toast(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let Some(toast) = &model.messages.toast else {
        return;
    };
    let area = frame.area();
    if area.width < 10 || area.height < 4 {
        return;
    }
    // A long sentence wraps onto a few lines inside the screen, and ends in an ellipsis
    // when even that is not enough: it ran off the right edge mid-word before.
    let room = usize::from(area.width.saturating_sub(6)).min(72);
    let lines = wrap_toast(&toast.message, room, 4);
    let text_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let width = (text_width as u16 + 4).min(area.width.saturating_sub(2));
    let height = lines.len() as u16 + 2;
    if height > area.height {
        return;
    }
    // The label carries the severity on its own, so the colour is reinforcement and
    // never the only signal.
    let role = match toast.severity {
        Severity::Info => Role::Muted,
        Severity::Warn => Role::Warning,
        Severity::Error => Role::Error,
    };
    let style = model.theme.style(role, model.capabilities);
    let block = Block::bordered()
        .style(model.theme.base(model.capabilities))
        .title(Span::styled(toast.severity.label(), style))
        .border_style(style);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width + 1),
        area.y + 1,
        width,
        height,
    );
    let body: Vec<Line> = lines
        .into_iter()
        .map(|line| Line::raw(format!(" {line} ")))
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(body).block(block), popup);
    register_overlay(hits, popup);
}

/// `message` broken at spaces into lines of at most `width` columns, `max_lines` of
/// them; what does not fit ends the last line with an ellipsis.
fn wrap_toast(message: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in message.split_whitespace() {
        let word_len = word.chars().count();
        let needed = if current.is_empty() {
            word_len
        } else {
            current.chars().count() + 1 + word_len
        };
        if needed <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        // A word longer than a line (a path, a long identifier) is cut where the line ends.
        let mut rest = word;
        while rest.chars().count() > width {
            let cut = rest
                .char_indices()
                .nth(width)
                .map_or(rest.len(), |(index, _)| index);
            lines.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }
        current.push_str(rest);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            while last.chars().count() >= width {
                last.pop();
            }
            last.push('…');
        }
    }
    lines
}

fn render_value_viewer(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let Some(view) = &model.data.viewer else {
        return;
    };
    let area = frame.area();
    let popup = centered(area, 80, area.height.saturating_sub(2).min(24));
    let inner = popup_inner(popup);
    // The value wraps to the popup, tabs as spaces (a tab has no width of its own), and
    // scrolls under a footer that stays: a long text is read here, not cut at the border.
    let width = (inner.width as usize).max(1);
    let wrapped: Vec<String> = crate::widgets::viewer::describe(view)
        .lines()
        .flat_map(|line| crate::model::wrap_display_text(&line.replace('\t', "    "), width))
        .collect();
    let body_rows = (inner.height as usize).saturating_sub(2).max(1);
    let max_scroll = wrapped.len().saturating_sub(body_rows);
    hits.set_scroll_limit(crate::mouse::ScrollArea::Value, max_scroll);
    let top = (model.data.viewer_scroll as usize).min(max_scroll);
    let mut lines: Vec<String> = wrapped.iter().skip(top).take(body_rows).cloned().collect();
    lines.resize(body_rows, String::new());
    lines.push(String::new());
    lines.push(if max_scroll == 0 {
        "Esc close".into()
    } else {
        format!(
            "Up/Down scroll  PageUp/PageDown  Esc close   lines {}-{} of {}",
            top + 1,
            (top + body_rows).min(wrapped.len()),
            wrapped.len()
        )
    });
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(
            model,
            &format!("Value: {}", crate::widgets::viewer::summary(view)),
        ),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
}

fn render_recovery(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 64, 12);
    let lines = model.recovery.lines();
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, "Session recovery"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.contains("[Keep]") {
            register_label(
                hits,
                rect,
                line,
                "[Keep]",
                HitTarget::Button(HitButton::Recover),
            );
            register_label(
                hits,
                rect,
                line,
                "[Discard]",
                HitTarget::Button(HitButton::Discard),
            );
        }
    });
}

fn render_diagnostics(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let popup = centered(frame.area(), 72, 16);
    let mut lines = model.diagnostics.lines();
    if !model.diagnostics.writing {
        lines.push("[Export]".into());
    }
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title("Diagnostics"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        register_label(
            hits,
            rect,
            line,
            "[Export]",
            HitTarget::Button(HitButton::Export),
        );
    });
}

/// The list scrolls and the confirmation does not: with many requests, a tall statement
/// or a short terminal, [Approve]/[Cancel] fell off the bottom of the popup.
fn render_mcp_audit(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let screen = &model.mcp_audit;
    let view = screen.view(area.width.min(100).saturating_sub(2) as usize);
    let wanted = view.body.len() + view.footer.len() + 2;
    let popup = centered(
        area,
        100,
        u16::try_from(wanted)
            .unwrap_or(u16::MAX)
            // Two rows short of the screen's, so it clears the tab bar and the status line.
            .min(area.height.saturating_sub(4)),
    );
    let inner = crate::mouse::popup_inner(popup);
    let footer_rows = (view.footer.len() as u16).min(inner.height);
    let body_rows = inner.height - footer_rows;
    let max_scroll = view.body.len().saturating_sub(body_rows as usize);
    // The picked request starts one line from the top, the line above it for context,
    // and the view scrolls on down through its statement and the recent calls.
    let base = view
        .picked
        .map_or(0, |(first, _)| first.saturating_sub(1))
        .min(max_scroll);
    hits.set_scroll_limit(crate::mouse::ScrollArea::McpAudit, max_scroll - base);
    let top = (base + screen.scroll as usize).min(max_scroll);
    let title = if top < max_scroll {
        "Agent activity · PgDn for more"
    } else {
        "Agent activity"
    };
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, title),
        String::new(),
    );
    let style = model.theme.base(model.capabilities);
    frame.render_widget(
        Paragraph::new(view.body.join("\n"))
            .style(style)
            .scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        Rect::new(inner.x, inner.y, inner.width, body_rows),
    );
    let footer = Rect::new(inner.x, inner.y + body_rows, inner.width, footer_rows);
    frame.render_widget(Paragraph::new(view.footer.join("\n")).style(style), footer);
    register_overlay(hits, popup);
    // Each waiting request answers a click, and the wheel reads the list.
    let body_area = Rect::new(inner.x, inner.y, inner.width, body_rows);
    for (line, index) in &view.rows {
        if *line >= top && line - top < body_rows as usize {
            hits.register(
                HitTarget::ListRow(*index),
                crate::mouse::line_rect(body_area, line - top),
            );
        }
    }
    let deciding = screen.deciding.as_ref();
    for (index, line) in view.footer.iter().enumerate().take(footer_rows as usize) {
        let rect = crate::mouse::line_rect(footer, index);
        if line.contains("[Cancel]") {
            let label = if deciding.is_some_and(|deciding| deciding.approve) {
                "Approve"
            } else {
                "Deny"
            };
            crate::widgets::form::register_footer(hits, rect, line, label);
        } else if line.contains("revoke all grants") {
            register_label(
                hits,
                rect,
                line,
                "R revoke all grants",
                HitTarget::Button(HitButton::Revoke),
            );
        }
    }
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
        Paragraph::new(lines.join("\n"))
            .block(Block::bordered().style(model.theme.base(model.capabilities))),
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
    // The same plan the frame is drawn with: without the tab row the editor sits one row
    // higher, and the popup's top border landed on the cursor's own line.
    let plan = LayoutPlan::for_area_with_document_tabs(area, Some(&model.effective_panes()), true);
    let inner = Block::bordered().inner(plan.content);
    let doc = model.active_document();
    let (line, col) = crate::screens::editor::line_col_of(&doc.text(), doc.cursor());
    let gutter = crate::widgets::editor::GUTTER;
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
    // The popup stays inside the editor pane: on a small terminal it ran over the pane's
    // border and into the results, and past the screen's right edge.
    let pane = plan.content;
    let width = width.min(pane.width.max(1));
    let height = (items.len().clamp(1, 8) as u16).saturating_add(2);
    let x = if cursor_x.saturating_add(width) > pane.right() {
        pane.right().saturating_sub(width).max(pane.x)
    } else {
        cursor_x
    };
    // Below the cursor if the list fits there, else above it, else where there is more
    // room, with fewer rows.
    let room_below = pane.bottom().saturating_sub(cursor_y.saturating_add(1));
    let room_above = cursor_y.saturating_sub(pane.y);
    let (y, height) = if room_below >= height {
        (cursor_y.saturating_add(1), height)
    } else if room_above >= height {
        (cursor_y - height, height)
    } else if room_below >= room_above {
        (cursor_y.saturating_add(1), room_below)
    } else {
        (cursor_y - room_above, room_above)
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
    let body = format!("{name} = {}", model.editor.parameter_draft.as_str());
    let footer = crate::widgets::form::footer_line("Submit", model.editor.parameter_footer);
    let lines = vec![body, footer];
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(format!(
            "Parameters ({} of {})",
            model.editor.parameter_index + 1,
            model.editor.parameters.len().max(1)
        )),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, line, rect| {
        if i == 0 {
            if model.editor.parameter_footer == crate::widgets::form::FooterFocus::Input {
                let before = format!("{name} = ");
                show_input(frame, rect, &before, &model.editor.parameter_draft, false);
            }
            hits.register(HitTarget::FormField(0), rect);
        } else {
            crate::widgets::form::register_footer(hits, rect, line, "Submit");
        }
    });
}

fn render_list_overlay(
    frame: &mut Frame,
    model: &Model,
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
        model,
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

/// The ways out of a row: as wide as the longest key needs (two long keys used to read
/// the same cut at 48 columns), with the keys that work under them.
fn render_related_picker(
    frame: &mut Frame,
    model: &Model,
    picker: &crate::screens::data::RelatedPicker,
    hits: &mut HitMap,
) {
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let labels: Vec<String> = match &picker.links {
        Some(links) => links.iter().map(|link| link.label.clone()).collect(),
        None => vec!["Looking for foreign keys…".into()],
    };
    let hint = "Enter open  Esc close";
    let widest = labels
        .iter()
        .map(|label| unicode_width::UnicodeWidthStr::width(label.as_str()) + 2)
        .chain([hint.len()])
        .max()
        .unwrap_or(0);
    let width = (widest as u16 + 4).max(48);
    let height = (labels.len() as u16 + 4).min(area.height);
    let popup = centered(area, width, height);
    let rows = (popup.height.saturating_sub(4) as usize).max(1);
    let offset = scroll_to_selection(picker.selected, 0, labels.len(), rows);
    let mut lines: Vec<String> = labels
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(index, label)| {
            let marker = if index == picker.selected { ">" } else { " " };
            format!("{marker} {label}")
        })
        .collect();
    let shown = lines.len();
    lines.push(String::new());
    lines.push(hint.into());
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title("Related rows"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    if picker.links.is_some() {
        for_popup_lines(popup, &lines, |i, _, rect| {
            if i < shown {
                hits.register(HitTarget::ListRow(offset + i), rect);
            }
        });
    }
}

fn render_history(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    use crate::widgets::form::footer_line;
    let editor = &model.editor;
    let area = frame.area();
    if area.width < 10 || area.height < 5 {
        return;
    }
    let name = model.connection.name.as_str();
    if editor.history_confirm_clear {
        let popup = centered(area, 60, 7);
        let question = if name.is_empty() {
            "Clear all of the history?".to_string()
        } else {
            format!("Clear the history of {name}?")
        };
        let lines = vec![
            question,
            "The statements it holds are removed for good.".into(),
            String::new(),
            footer_line("Clear", editor.history_footer),
        ];
        paint_popup(
            frame,
            model,
            popup,
            overlay_block(model, "Clear history"),
            lines.join("\n"),
        );
        register_overlay(hits, popup);
        for_popup_lines(popup, &lines, |_, line, rect| {
            if line.contains("[Cancel]") {
                crate::widgets::form::register_footer(hits, rect, line, "Clear");
            }
        });
        return;
    }
    let popup = centered(area, 100, area.height.saturating_sub(2).min(18));
    let inner = popup_inner(popup);
    let width = inner.width as usize;
    let rows = (inner.height as usize).saturating_sub(2).max(1);
    let matches = editor.history_matches();
    let offset = scroll_to_selection(editor.history_selected, 0, matches.len(), rows);
    let mut lines = vec![
        editor
            .history_search
            .inline_line_within("search: ", true, width),
    ];
    for row in 0..rows {
        let index = offset + row;
        let line = match matches.get(index) {
            Some(sql) => {
                let marker = if index == editor.history_selected {
                    ">"
                } else {
                    " "
                };
                // One line per statement; its layout is the editor's to show.
                let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                format!(
                    "{marker} {}",
                    crate::model::truncate_cell(&flat, width.saturating_sub(2))
                )
            }
            None if row == 0 && editor.history.is_empty() => {
                "  Nothing has run yet on this connection.".into()
            }
            None if row == 0 => "  No statement matches.".into(),
            None => String::new(),
        };
        lines.push(line);
    }
    lines.push("Enter open in a new document  Esc close".into());
    let title = if name.is_empty() {
        "History".to_string()
    } else {
        format!("History · {name}")
    };
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, &title),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |i, _, rect| {
        if i == 0 {
            paint_selection(frame, rect, "search: ", &editor.history_search, true);
        } else if i <= rows && offset + i - 1 < matches.len() {
            hits.register(HitTarget::ListRow(offset + i - 1), rect);
        }
    });
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
        model,
        "Snippets",
        &names,
        model.editor.snippet_selected,
        0,
        hits,
    );
}

fn render_file_picker(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let area = frame.area();
    let popup = centered(
        area,
        72,
        crate::screens::file_picker::popup_height(area.height),
    );
    let inner = popup_inner(popup);
    let layout = model.file_picker.layout(
        model.file_picker_mode,
        usize::from(inner.height),
        usize::from(inner.width),
    );
    paint_popup(
        frame,
        model,
        popup,
        overlay_block(model, model.file_picker_mode.title()),
        layout.lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &layout.lines, |i, line, rect| {
        match layout.kinds.get(i) {
            Some(crate::screens::file_picker::FilePickerLineKind::Cwd) => {
                hits.register(HitTarget::Button(HitButton::ParentDir), rect);
            }
            Some(crate::screens::file_picker::FilePickerLineKind::RecentItem(index)) => {
                hits.register(HitTarget::RecentSqlFile(*index), rect);
            }
            Some(crate::screens::file_picker::FilePickerLineKind::BrowserEntry(index)) => {
                hits.register(HitTarget::ListRow(*index), rect);
            }
            Some(crate::screens::file_picker::FilePickerLineKind::Name) => {
                let picker = &model.file_picker;
                let focused = picker.focus == crate::screens::file_picker::FilePickerFocus::Name;
                paint_selection(frame, rect, "> name: ", &picker.name, focused);
                hits.register(HitTarget::FormField(0), rect);
            }
            Some(crate::screens::file_picker::FilePickerLineKind::Footer)
                if line.contains("[Cancel]") =>
            {
                crate::widgets::form::register_footer(
                    hits,
                    rect,
                    line,
                    model.file_picker_mode.submit_label(),
                );
            }
            _ => {}
        }
    });
    if let Some(confirm) = &model.file_picker.confirm {
        let lines = confirm.lines();
        let width = lines
            .iter()
            .map(|line| line.chars().count() as u16 + 4)
            .max()
            .unwrap_or(0)
            .max(34);
        let ask = centered(area, width, lines.len() as u16 + 2);
        paint_popup(
            frame,
            model,
            ask,
            overlay_block(model, "Replace file?"),
            lines.join("\n"),
        );
        register_overlay(hits, ask);
        let footer = crate::mouse::line_rect(popup_inner(ask), lines.len() - 1);
        crate::widgets::form::register_footer(hits, footer, &lines[lines.len() - 1], "Replace");
    }
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

    /// The completion list stays inside the editor pane at 80x24 and 60x20: it ran over
    /// the pane's bottom border into the results and past the screen's right edge.
    #[test]
    fn the_completion_popup_stays_inside_the_editor_pane() {
        use ratatui::layout::Rect;
        for (width, height) in [(80u16, 24u16), (60, 20), (120, 36)] {
            let mut model = Model::default();
            model.apply_size(width, height);
            model.set_sql("select * from customers c where c.");
            let sql_len = model.active_document().text().chars().count();
            model.active_document_mut().sql.set_cursor(sql_len).unwrap();
            let items: Vec<String> = (0..12)
                .map(|n| format!("created_at_column_{n}  main.customers"))
                .collect();
            let area = Rect::new(0, 0, width, height);
            let plan = crate::layout::LayoutPlan::for_area_with_document_tabs(
                area,
                Some(&model.effective_panes()),
                true,
            );
            let popup = super::completion_popup_rect(area, &model, &items);
            assert!(popup.right() <= plan.content.right(), "{width}x{height}");
            assert!(popup.bottom() <= plan.content.bottom(), "{width}x{height}");
            assert!(popup.x >= plan.content.x && popup.y >= plan.content.y);
        }
    }

    /// On production the analyze dialog says so in the warning colour, on that line
    /// and no other.
    #[test]
    fn the_analyze_dialog_warns_on_its_production_line() {
        use crate::theme::Role;
        let mut model = Model::default();
        model.connection.environment = "production".into();
        model.explain_prompt = Some(crate::widgets::form::FooterFocus::Submit);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| super::render(frame, &model, &mut hits))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let warning = model.theme.style(Role::Warning, model.capabilities).fg;
        let fg_of = |text: &str| {
            let area = buffer.area();
            (area.y..area.bottom()).find_map(|y| {
                let row: String = (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                let x = row.find(text)?;
                let column = row[..x].chars().count() as u16;
                Some(buffer[(area.x + column, y)].fg)
            })
        };
        assert_eq!(fg_of("This is a production connection."), warning);
        assert_ne!(fg_of("an auto-increment counter"), warning);
    }

    /// The welcome logo used to come out in the text colour: its cells were joined into
    /// plain lines. It starts in the theme's accent and fades towards the text colour.
    #[test]
    fn the_welcome_logo_is_painted_in_the_theme_colours() {
        use crate::theme::Role;
        use ratatui::style::Color;
        let mut model = Model::default();
        model.onboarding.open = true;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| super::render(frame, &model, &mut hits))
            .unwrap();
        let painted: Vec<Color> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| matches!(cell.symbol(), "█" | "░"))
            .map(|cell| cell.fg)
            .collect();
        let (r, g, b) = model.theme.rgb(Role::Focus).unwrap();
        assert!(painted.contains(&Color::Rgb(r, g, b)));
        let text = model.theme.color(Role::Foreground, model.capabilities);
        assert!(painted.iter().filter(|fg| Some(**fg) == text).count() < painted.len() / 2);
    }

    #[test]
    fn help_search_shows_the_typed_query() {
        let mut model = Model::default();
        model.apply_size(100, 40);
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("disc");

        let view = render_to_string(&model, 100, 40);

        assert!(view.contains("Search: disc"));
    }

    /// The suggested name is selected, and shows it.
    #[test]
    fn a_selected_name_is_drawn_in_reverse() {
        use ratatui::style::Modifier;
        let model = Model {
            document_name_prompt:
                crate::screens::document_name_prompt::DocumentNamePrompt::open_create(
                    "query-1.sql".into(),
                    None,
                ),
            ..Model::default()
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| super::render(frame, &model, &mut hits))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let reversed: String = buffer
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
            .map(|cell| cell.symbol())
            .collect();
        assert!(reversed.contains("query-1.sql"), "{reversed:?}");
    }

    /// The cells a frame of `model` draws in reverse video, in order.
    fn reversed(model: &Model) -> String {
        use ratatui::style::Modifier;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| super::render(frame, model, &mut hits))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
            .map(|cell| cell.symbol())
            .collect()
    }

    /// Ctrl+A selected the saved-queries search, its rename and Vim's `:` line without
    /// showing it, and the next letter typed wiped the text.
    #[test]
    fn every_selected_input_is_drawn_in_reverse() {
        use crate::widgets::text_input::TextInput;
        let mut model = Model::default();
        model.saved_queries.open = true;
        model
            .saved_queries
            .set_items(vec![dexo_storage::SavedQuery {
                id: "1".into(),
                connection_id: String::new(),
                name: "monthly".into(),
                sql: "select 1".into(),
            }]);
        model.saved_queries.search = TextInput::new("mon");
        model.saved_queries.search.select_all();
        assert!(reversed(&model).contains("mon"));

        model.saved_queries.search.clear();
        let mut name = TextInput::new("monthly-sales");
        name.select_all();
        model.saved_queries.renaming = Some(name);
        assert!(reversed(&model).contains("monthly-sales"));

        let mut model = Model {
            keymap: crate::keymap::Keymap::vim_profile(),
            focus: crate::model::Focus::Editor,
            ..Model::default()
        };
        let mut input = TextInput::new("s/a/b/");
        input.select_all();
        model.vim.prompt = Some(crate::screens::vim::Prompt { kind: ':', input });
        assert!(reversed(&model).contains("s/a/b/"));
    }

    /// PageUp and PageDown moved one line, like the arrows.
    #[test]
    fn the_keybindings_scroll_a_page_at_a_time() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut model = Model::default();
        model.apply_size(100, 30);
        model.help.open = true;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| super::render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let key = |code| crate::Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        crate::update::update(&mut model, key(KeyCode::PageDown));
        // The list shows 24 of the popup's 26 rows, under the search and a blank.
        assert_eq!(model.help.scroll, 23);
        crate::update::update(&mut model, key(KeyCode::End));
        let bottom = model.help.scroll;
        assert!(bottom > 23);
        crate::update::update(&mut model, key(KeyCode::PageUp));
        assert_eq!(model.help.scroll, bottom - 23);
        crate::update::update(&mut model, key(KeyCode::Home));
        assert_eq!(model.help.scroll, 0);
    }

    /// The search line scrolled away with the list after a PageDown.
    #[test]
    fn the_keybindings_search_stays_above_the_list() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut model = Model::default();
        model.apply_size(100, 30);
        model.help.open = true;
        let mut hits = crate::mouse::HitMap::default();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| super::render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let first = render_to_string(&model, 100, 30);
        let key = |code| crate::Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        crate::update::update(&mut model, key(KeyCode::PageDown));
        assert!(model.help.scroll > 0);
        let paged = render_to_string(&model, 100, 30);
        let search_row = |view: &str| view.lines().position(|line| line.contains("Search: "));
        assert!(search_row(&first).is_some(), "{first}");
        assert_eq!(search_row(&paged), search_row(&first), "{paged}");
        assert_ne!(first, paged, "the list did not move");
    }

    #[test]
    fn help_search_filters_bindings_by_action_or_chord() {
        let mut model = Model::default();
        model.apply_size(100, 40);
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("disconnect");

        let view = render_to_string(&model, 100, 40);

        assert!(view.contains("Shift+D"));
        assert!(view.contains("Disconnect Connection"));
        assert!(!view.contains("New Connection"));
    }

    #[test]
    fn help_search_shows_a_message_when_nothing_matches() {
        let mut model = Model::default();
        model.apply_size(100, 40);
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("zzz-no-such-binding");

        let view = render_to_string(&model, 100, 40);

        assert!(view.contains("no matches"));
    }

    #[test]
    fn compact_workbench_footer_hints_sidebar_access() {
        let mut model = Model::default();
        model.apply_size(60, 20);
        model.focus = crate::model::Focus::Editor;

        let frame = render_to_string(&model, 60, 20);

        assert!(frame.contains("Alt+1 connections"));
        // One spelling of the keys, and Ctrl+P once: it read `ctrl+p  F1  Alt+1
        // connections  Ctrl+P commands`.
        let status = frame.lines().last().unwrap_or_default();
        assert!(status.contains("Ctrl+P  F1"), "{status}");
        assert!(!status.contains("ctrl+p"), "{status}");
        assert_eq!(status.matches("Ctrl+P").count(), 1, "{status}");
    }

    #[test]
    fn object_metadata_renders_as_overlays_not_tabs() {
        use crate::screens::object_inspector::InspectorFacet;

        let mut model = Model {
            width: 100,
            height: 40,
            ..Model::default()
        };
        model.inspector.open = true;
        model.inspector.facet = InspectorFacet::Properties;
        let props = render_to_string(&model, 100, 40);
        assert!(props.contains("Properties"), "{props}");
        assert!(props.contains("Select an object in Explorer"));

        model.inspector.facet = InspectorFacet::Ddl;
        let ddl = render_to_string(&model, 100, 40);
        assert!(ddl.contains("DDL is not available"), "{ddl}");

        model.inspector.open = false;
        model.schema_editor.open = true;
        let form = render_to_string(&model, 100, 40);
        assert!(
            form.contains("target:") || form.contains("schema table"),
            "{form}"
        );
    }

    /// A column says its type and nullability, and what it relates to by name, one to a line.
    #[test]
    fn properties_say_what_a_column_is() {
        let mut model = Model::default();
        let column = dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new("c"),
            dexo_driver_api::ObjectKind::Column,
            dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "name"),
            None,
        )
        .with_attribute("type", serde_json::json!("text"))
        .with_attribute("driver.postgres.not_null", serde_json::json!(true));
        let related = dexo_driver_api::ObjectId::new("pg:constraint:1");
        model.inspector.qualified_name = "public.customers.name".into();
        model.inspector.object = Some(column);
        model.inspector.dependents = vec![related.clone()];
        model
            .inspector
            .names
            .insert(related, "constraint customers_pkey".into());
        let body = super::properties_tab_body(&model);
        assert!(body.contains("type: text"), "{body}");
        assert!(body.contains("nullable: no"), "{body}");
        assert!(body.contains("  constraint customers_pkey"), "{body}");
        assert!(!body.contains("pg:constraint"), "{body}");
    }

    #[test]
    fn sql_workbench_renders_document_tabs_with_dirty_marker() {
        let mut model = Model::default();
        model.documents = vec![
            crate::model::EditorDocument::new_unique("console.sql", None, None),
            crate::model::EditorDocument::new_unique("q2.sql", None, None),
        ];
        model.documents[1].sql.insert(0, "select 1").unwrap();
        model.set_active_document(1);

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
