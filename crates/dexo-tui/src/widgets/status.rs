use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::accessibility::environment_marker;
use crate::model::Model;
use crate::theme::Role;
use dexo_driver_api::TransactionState;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // The sidebar already shows a connected session with a dot, so the name carries a
    // prefix only when something is wrong.
    let conn = if model.connection.ready && model.connections.is_temporary(&model.connection.name) {
        // Nothing saved behind it: closing Dexo forgets it, unless Save Connection… keeps it.
        format!("{} (temporary)", model.connection.name)
    } else if model.connection.ready {
        model.connection.name.clone()
    } else if model.connection.name.is_empty() {
        "disconnected".into()
    } else {
        format!("offline:{}", model.connection.name)
    };
    // Idle is the null state; a marker shown always marks nothing.
    let tx = match model.transaction {
        TransactionState::Idle => "",
        TransactionState::Active => "tx:active",
        TransactionState::Failed => "tx:failed",
        TransactionState::Unknown => "tx:unknown",
    };
    let env = environment_marker(&model.connection.environment, model.capabilities.unicode);
    let env_style = model.theme.style(
        crate::accessibility::environment_role(&model.connection.environment)
            .unwrap_or(Role::Muted),
        model.capabilities,
    );
    let err_style = model.theme.style(Role::Error, model.capabilities);
    let tx_style = if matches!(
        model.transaction,
        TransactionState::Failed | TransactionState::Unknown
    ) {
        err_style
    } else {
        Style::default()
    };
    // Vim's `:` and `/` are typed on this line, as in Vim, with the cursor in them.
    if crate::screens::vim::active(model)
        && let Some(prompt) = &model.vim.prompt
    {
        // Selected, the text shows in reverse: typing replaces it.
        let style = if prompt.input.is_selected() {
            Style::default().add_modifier(ratatui::style::Modifier::REVERSED)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(prompt.kind.to_string()),
                Span::styled(prompt.input.as_str().to_string(), style),
            ])),
            area,
        );
        let before: String = prompt
            .input
            .as_str()
            .chars()
            .take(prompt.input.cursor())
            .collect();
        let x = area.x + 1 + unicode_width::UnicodeWidthStr::width(before.as_str()) as u16;
        if x < area.x + area.width {
            frame.set_cursor_position(ratatui::layout::Position::new(x, area.y));
        }
        return;
    }
    let mut spans = Vec::new();
    if crate::screens::vim::active(model) && model.effective_focus() == crate::model::Focus::Editor
    {
        spans.push(Span::styled(
            format!("-- {} -- ", crate::screens::vim::mode(model).label()),
            model
                .theme
                .style(Role::Focus, model.capabilities)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
        let pending = crate::screens::vim::pending(model);
        if !pending.is_empty() {
            spans.push(Span::raw(format!("{pending}  ")));
        }
    }
    if !model.mouse {
        spans.push(Span::styled(
            "MOUSE OFF · Ctrl+P settings.mouse  ",
            err_style,
        ));
    }
    if matches!(model.layout_mode, crate::layout::LayoutMode::Compact) {
        spans.push(Span::raw(format!("{conn}  ctrl+p  F1")));
        if let Some(notice) = &model.update_notice {
            let arrow = if model.capabilities.unicode {
                "↑"
            } else {
                "update"
            };
            spans.push(Span::styled(
                format!("  {arrow} {}", notice.version),
                model.theme.style(Role::Warning, model.capabilities),
            ));
        }
        if let Some(hint) = footer_hint(model) {
            spans.push(Span::raw(format!("  {hint}")));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }
    if !env.is_empty() {
        spans.push(Span::styled(format!("{env} "), env_style));
    }
    spans.push(Span::raw(format!("{conn}  ")));
    if !tx.is_empty() {
        spans.push(Span::styled(format!("{tx}  "), tx_style));
    }
    // The focused pane already carries an accent border and the cursor; the layout
    // preset and the row count are both printed where they apply. None of them
    // belong here.
    //
    // Truncation order, narrowest last to survive: environment, connection, the two
    // doors, then the hint. The doors outlive the hint because F1 is how you get the
    // hint back.
    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let doors = doors(model);
    let doors = doors.as_str();
    let room = (area.width as usize).saturating_sub(used);
    // A newer release outranks the key hint: the hint comes back with F1, the command
    // to update does not come back at all.
    let notice = update_notice(model, room.saturating_sub(doors.chars().count() + 2));
    let reserved = doors.chars().count() + 2 + notice.as_ref().map_or(0, |n| n.chars().count() + 2);
    // With the cursor on something the editor underlined, its message says what.
    let wrong = (model.effective_focus() == crate::model::Focus::Editor)
        .then(|| crate::screens::editor::diagnostic_at_cursor(model))
        .flatten();
    if let Some(message) = wrong
        && room > reserved
    {
        spans.push(Span::styled(fit_hint(message, room - reserved), err_style));
    } else if let Some(hint) = footer_hint(model)
        && room > reserved
    {
        spans.push(Span::raw(fit_hint(&hint, room - reserved)));
    }
    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let right = notice.as_ref().map_or(0, |n| n.chars().count() + 2) + doors.chars().count();
    let gap = (area.width as usize).saturating_sub(used + right);
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        if let Some(notice) = notice {
            spans.push(Span::styled(
                notice,
                model.theme.style(Role::Warning, model.capabilities),
            ));
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            doors.to_string(),
            model.theme.style(Role::Muted, model.capabilities),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The version and the command that installs it, or only the version when the
/// command does not fit in `budget`.
fn update_notice(model: &Model, budget: usize) -> Option<String> {
    let notice = model.update_notice.as_ref()?;
    let arrow = if model.capabilities.unicode {
        "↑"
    } else {
        "update"
    };
    let full = format!("{arrow} {}: {}", notice.version, notice.command);
    if full.chars().count() <= budget {
        return Some(full);
    }
    let short = format!("{arrow} {}", notice.version);
    (short.chars().count() <= budget).then_some(short)
}

/// Drops whole hints rather than cutting a chord in half -- "Ctrl+W" teaches nothing.
fn fit_hint(hint: &str, budget: usize) -> String {
    if hint.chars().count() <= budget {
        return hint.to_string();
    }
    let mut out = String::new();
    for part in hint.split("  ") {
        let candidate = if out.is_empty() {
            part.to_string()
        } else {
            format!("{out}  {part}")
        };
        if candidate.chars().count() > budget {
            break;
        }
        out = candidate;
    }
    out
}

/// `key what` for each command the active keymap binds, two spaces apart: Vim and
/// Emacs page, close and run with other keys, or none, and a fixed hint said keys that
/// did nothing there.
fn keyed_hint(model: &Model, parts: &[(&str, &str)]) -> String {
    parts
        .iter()
        .filter_map(|(id, what)| {
            let key = crate::palette::shortcut_for(model, id, None)?;
            Some(format!("{key} {what}"))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// The palette's and Help's keys, which bring the hint back.
fn doors(model: &Model) -> String {
    keyed_hint(model, &[("palette.open", ""), ("help.open", "")])
        .split("  ")
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("  ")
}

fn footer_hint(model: &Model) -> Option<String> {
    if matches!(model.layout_mode, crate::layout::LayoutMode::Compact)
        && matches!(
            model.effective_focus(),
            crate::model::Focus::Editor | crate::model::Focus::Palette
        )
    {
        return Some(keyed_hint(
            model,
            &[
                ("focus.explorer", "connections"),
                ("palette.open", "commands"),
            ],
        ));
    }
    // A table document has no editor on screen, so the editor's hint would be a lie.
    match model.effective_focus() {
        crate::model::Focus::Explorer => model
            .explorer
            .selected_node()
            .map(|node| {
                if crate::screens::explorer::is_connection_node(node) {
                    if model.connections.session_for(&node.label).is_some() {
                        "Enter expand  a actions  n new  e edit"
                    } else {
                        "Enter connect  a actions  n new  e edit"
                    }
                } else if crate::screens::explorer::opens_table_data(&node.kind) {
                    "Enter expand  o open  a actions"
                } else {
                    "Enter expand  a actions"
                }
            })
            .or(Some("Enter connect/expand  a actions  n new"))
            .map(str::to_string),
        crate::model::Focus::DocumentTabs => Some(format!(
            "←/→ tabs  Enter open/new  {}  Esc editor",
            keyed_hint(model, &[("document.close", "close")])
        )),
        crate::model::Focus::Editor => Some(keyed_hint(
            model,
            &[
                ("query.execute_statement", "run"),
                ("document.new", "new sql"),
                ("document.close", "close"),
            ],
        )),
        crate::model::Focus::Results => {
            let page = match (
                crate::palette::shortcut_for(model, "data.page_next", None),
                crate::palette::shortcut_for(model, "data.page_prev", None),
            ) {
                (Some(next), Some(previous)) => format!("{next}/{previous} page  "),
                _ => String::new(),
            };
            Some(format!(
                "Enter actions  {}  {page}{}",
                keyed_hint(model, &[("results.cycle_view", "view")]),
                keyed_hint(model, &[("document.close", "close")])
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::footer_hint;
    use crate::model::{Focus, Model};
    use crate::screens::explorer::connection_id;

    #[test]
    fn sidebar_footer_shows_connect_or_expand_hint() {
        let mut model = Model {
            focus: Focus::Explorer,
            ..Model::default()
        };
        model.connections.load_profiles(vec![profile("prod")]);
        model
            .explorer
            .sync_connection_roots(&model.connections.profiles, "");
        model.explorer.select(connection_id("prod"));
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Enter connect  a actions  n new  e edit")
        );

        model
            .connections
            .upsert_session(crate::screens::connections::SessionRow {
                id: crate::runtime::SessionId(uuid::Uuid::new_v4()),
                connection: "prod".into(),
                transaction: dexo_driver_api::TransactionState::Idle,
                generation: 1,
                environment: "local".into(),
                read_only: false,
                driver: "postgres".into(),
            });
        model
            .explorer
            .sync_connection_roots(&model.connections.profiles, "prod");
        model.explorer.select(connection_id("prod"));
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Enter expand  a actions  n new  e edit")
        );
    }

    fn profile(name: &str) -> dexo_app::ConnectionProfile {
        dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::nil()),
            None,
            name,
            "postgres",
            "local",
            serde_json::json!({}),
            dexo_app::SecretRef::new("ref".into()),
        )
    }

    #[test]
    fn compact_editor_footer_hints_sidebar_access() {
        use crate::layout::LayoutMode;

        let model = Model {
            focus: Focus::Editor,
            layout_mode: LayoutMode::Compact,
            ..Model::default()
        };
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Alt+1 connections  Ctrl+P commands")
        );
    }

    #[test]
    /// A table document has no editor on screen, so the editor's hint was a lie there:
    /// `Ctrl+Enter run` runs nothing on a table.
    fn the_footer_hint_follows_the_pane_a_table_document_actually_shows() {
        let mut model = Model {
            focus: Focus::Editor,
            ..Model::default()
        };
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                dexo_app::parse_qualified("public.orders"),
                None,
            ));
        model.set_active_document(1);
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Enter actions  v view  n/p page  Ctrl+W close")
        );

        model.set_active_document(0);
        model.keys_disambiguated = true;
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Ctrl+Enter run  Ctrl+N new sql  Ctrl+W close")
        );
        // Where the terminal sends Ctrl+Enter as Enter, the key that runs is named.
        model.keys_disambiguated = false;
        assert_eq!(
            footer_hint(&model).as_deref(),
            Some("Ctrl+J run  Ctrl+N new sql  Ctrl+W close")
        );
    }

    #[test]
    fn messages_surface_as_a_toast_and_leave_the_footer_alone() {
        use crate::action::Action;
        use crate::render::render_to_string;
        use crate::update::update;

        let mut model = Model::default();
        model
            .messages
            .warn("Save the untitled document before closing it.".into());

        let view = render_to_string(&model, 120, 40);
        let footer = view.lines().last().unwrap();
        assert!(
            !footer.contains("Save the untitled"),
            "the message is back in the status bar: {footer}"
        );
        assert!(view.contains("Save the untitled"), "the toast never showed");

        // a warning ages out on its own
        for _ in 0..crate::model::Severity::Warn.ticks() {
            update(&mut model, Action::ToastTick);
        }
        assert!(model.messages.toast.is_none());
        assert!(
            !render_to_string(&model, 120, 40).contains("Save the untitled"),
            "the toast outlived its ticks"
        );
        // and the log keeps it
        assert_eq!(model.messages.iter().count(), 1);

        // Esc clears it without stealing the key from whatever is open.
        model.messages.warn("second".into());
        model.help.open = true;
        update(
            &mut model,
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        assert!(model.messages.toast.is_none(), "Esc left the toast up");
        assert!(!model.help.open, "the toast ate the overlay's Esc");
    }

    /// Nothing else in the app records a message, so an error that blinks out is lost.
    #[test]
    fn an_error_toast_stays_until_it_is_dismissed() {
        use crate::action::Action;
        use crate::render::render_to_string;
        use crate::update::update;

        let mut model = Model::default();
        model
            .messages
            .error("relation \"orders\" does not exist".into());
        for _ in 0..50 {
            update(&mut model, Action::ToastTick);
        }
        let view = render_to_string(&model, 120, 40);
        assert!(view.contains("does not exist"), "the error aged out");
        assert!(
            view.contains("error"),
            "the toast never said it was an error"
        );
        assert!(
            !model.messages.expires(),
            "a sticky toast is running the clock"
        );

        update(&mut model, Action::DismissToast);
        assert!(model.messages.toast.is_none());
    }

    #[test]
    fn narrow_status_drops_whole_hints_and_keeps_the_doors() {
        use crate::render::render_to_string;

        let model = Model::default();
        let footer = |width: u16| {
            render_to_string(&model, width, 40)
                .lines()
                .last()
                .unwrap()
                .to_string()
        };

        let wide = footer(160);
        assert!(wide.contains("Ctrl+W close"));

        let narrow = footer(60);
        // the way back to everything outlives the contextual hint
        assert!(narrow.contains("Ctrl+P"), "{narrow}");
        assert!(!narrow.contains("Ctrl+W close"), "{narrow}");
        // and a chord is never cut in half
        assert!(!narrow.contains("Ctrl+W"), "{narrow}");
    }

    #[test]
    fn disabled_mouse_status_shows_the_keyboard_recovery_command() {
        use crate::render::render_to_string;

        for (width, height) in [(160, 50), (60, 20)] {
            let mut model = Model {
                mouse: false,
                ..Model::default()
            };
            model.apply_size(width, height);
            let view = render_to_string(&model, width, height);
            assert!(
                view.contains("MOUSE OFF · Ctrl+P settings.mouse"),
                "missing mouse recovery command at {width}x{height}"
            );
        }
    }

    /// The hints say the keys of the keymap in use: Vim has no page keys, and Emacs
    /// opens the palette with Alt+X.
    #[test]
    fn hints_follow_the_keymap() {
        let mut model = Model {
            focus: Focus::Results,
            keymap: crate::keymap::Keymap::vim_profile(),
            ..Model::default()
        };
        let hint = footer_hint(&model).unwrap();
        assert!(!hint.contains("page"), "{hint}");
        assert!(hint.contains("Ctrl+W close"), "{hint}");
        model.keymap = crate::keymap::Keymap::emacs_profile();
        assert!(
            super::doors(&model).starts_with("Alt+X"),
            "{}",
            super::doors(&model)
        );
        model.keymap = crate::keymap::Keymap::default_profile();
        assert!(footer_hint(&model).unwrap().contains("n/p page"));
    }
}
