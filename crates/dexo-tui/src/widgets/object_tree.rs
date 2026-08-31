use crate::palette::scroll_to_selection;
use crate::screens::connections::ConnectionRow;
use crate::screens::explorer::{ExplorerNode, ExplorerState, NodeState, SidebarFocus};
use dexo_driver_api::ObjectId;

pub fn chrome_count(state: &ExplorerState) -> usize {
    let mut n = 0;
    if state.offline {
        n += 1;
    }
    if !state.search.is_empty() {
        n += 1;
    }
    if !state.filter_name.is_empty() || state.filter_kind.is_some() || state.favorites_only {
        n += 1;
    }
    n
}

pub fn windowed_ids(state: &ExplorerState, viewport_rows: usize) -> (usize, Vec<ObjectId>) {
    let ids = state.visible_ids();
    let chrome = chrome_count(state);
    let tree_rows = viewport_rows.saturating_sub(chrome).max(1);
    let offset = scroll_to_selection(state.selected_index(), state.offset, ids.len(), tree_rows);
    let window = ids.into_iter().skip(offset).take(tree_rows).collect();
    (offset, window)
}

pub fn render_lines(state: &ExplorerState) -> Vec<String> {
    render_visible(state, None)
}

pub fn render_sidebar(
    state: &ExplorerState,
    profiles: &[ConnectionRow],
    active_connection: &str,
    unicode: bool,
    viewport_rows: usize,
) -> Vec<String> {
    let connected = if unicode { "●" } else { "*" };
    let offline = if unicode { "○" } else { " " };
    let mut lines = vec!["Connections".into()];
    if profiles.is_empty() {
        lines.push("No connections — press n".into());
    } else {
        for (index, row) in profiles.iter().enumerate() {
            let cursor = if state.sidebar_focus == SidebarFocus::Connections
                && state.connection_cursor == index
            {
                ">"
            } else {
                " "
            };
            let marker = if row.sessions > 0 {
                connected
            } else {
                offline
            };
            lines.push(format!("{cursor} {marker} {}", row.profile.name));
        }
    }
    lines.push(if state.offline {
        "Catalog — offline".into()
    } else {
        "Catalog".into()
    });
    let catalog_rows = viewport_rows.saturating_sub(lines.len()).max(1);
    if active_connection.is_empty() {
        lines.push("Select a connection".into());
    } else if state.roots.is_empty() && state.offline {
        lines.push("No catalog".into());
    } else {
        lines.extend(render_visible_inner(state, Some(catalog_rows), false));
    }
    lines
}

pub fn render_visible(state: &ExplorerState, viewport_rows: Option<usize>) -> Vec<String> {
    render_visible_inner(state, viewport_rows, true)
}

fn render_visible_inner(
    state: &ExplorerState,
    viewport_rows: Option<usize>,
    show_offline_chrome: bool,
) -> Vec<String> {
    let mut header = Vec::new();
    if show_offline_chrome && state.offline {
        header.push("[offline]".into());
    }
    if !state.search.is_empty() {
        header.push(format!("search:{}", state.search));
    }
    if !state.filter_name.is_empty() || state.filter_kind.is_some() || state.favorites_only {
        header.push(format!(
            "filter:{} kind:{} fav:{}",
            state.filter_name,
            state.filter_kind.as_deref().unwrap_or("-"),
            state.favorites_only
        ));
    }
    let mut tree = Vec::new();
    collect(&state.roots, state, 0, &mut tree);
    let tree = window_tree(state, &tree, viewport_rows, header.len());
    let mut lines = header;
    lines.extend(tree);
    if lines.is_empty() {
        lines.push("Select a connection".into());
    }
    lines
}

fn window_tree(
    state: &ExplorerState,
    tree: &[String],
    viewport_rows: Option<usize>,
    chrome: usize,
) -> Vec<String> {
    let Some(rows) = viewport_rows else {
        return tree.to_vec();
    };
    let tree_rows = rows.saturating_sub(chrome).max(1);
    let offset = scroll_to_selection(state.selected_index(), state.offset, tree.len(), tree_rows);
    tree.iter().skip(offset).take(tree_rows).cloned().collect()
}

fn collect(nodes: &[ExplorerNode], state: &ExplorerState, depth: usize, lines: &mut Vec<String>) {
    for node in nodes {
        if state.matches(node) {
            let marker = if node.can_expand() {
                if node.expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let badge = match node.state {
                NodeState::Loading(_) => " [loading]",
                NodeState::Restricted => " [restricted]",
                NodeState::Error { .. } => " [error]",
                NodeState::Stale => " [stale]",
                NodeState::Collapsed | NodeState::Expanded => "",
            };
            let fav = if node.favorite { "*" } else { "" };
            let cursor = if state.selected.as_ref() == Some(&node.id) {
                ">"
            } else {
                " "
            };
            lines.push(format!(
                "{cursor}{}{marker}{fav}{}{badge}",
                "  ".repeat(depth),
                node.label
            ));
        }
        if node.expanded {
            collect(&node.children, state, depth + 1, lines);
        }
    }
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

    use crate::screens::explorer::ExplorerState;

    fn table(id: &str, name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(id),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn selected_row_has_palette_cursor_and_scrolls() {
        let mut explorer = ExplorerState::default();
        explorer.replace_roots(CatalogList {
            objects: (0..12)
                .map(|i| table(&format!("table:{i}"), &format!("t{i}")))
                .collect(),
            restrictions: vec![],
        });
        explorer.select(ObjectId::new("table:0"));
        let full = super::render_lines(&explorer);
        assert!(
            full.iter()
                .any(|line| line.starts_with('>') && line.contains("t0")),
            "{full:?}"
        );

        explorer.select(ObjectId::new("table:11"));
        explorer.sync_scroll(4);
        let window = super::render_visible(&explorer, Some(4));
        let tree: Vec<_> = window
            .iter()
            .filter(|line| line.contains('▸') || line.contains('▾'))
            .collect();
        assert!(
            tree.iter()
                .any(|line| line.contains('>') && line.contains("t11")),
            "{window:?}"
        );
        assert!(
            !tree.iter().any(|line| line.contains("t0")),
            "scroll should hide the top: {window:?}"
        );
    }

    use crate::screens::connections::ConnectionRow;
    use crate::screens::explorer::SidebarFocus;
    use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

    fn profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            name,
            "postgres",
            "local",
            serde_json::json!({}),
            SecretRef::new("ref".into()),
        )
    }

    fn connection_row(name: &str, sessions: usize) -> ConnectionRow {
        ConnectionRow {
            profile: profile(name),
            sessions,
        }
    }

    #[test]
    fn sidebar_empty_connections_shows_press_n_hint() {
        let explorer = ExplorerState::default();
        let lines = super::render_sidebar(&explorer, &[], "", true, 8);
        let text = lines.join("\n");
        assert!(text.contains("No connections — press n"), "{text}");
    }

    #[test]
    fn sidebar_catalog_without_session_prompts_select() {
        let explorer = ExplorerState::default();
        let lines = super::render_sidebar(
            &explorer,
            &[connection_row("prod", 0)],
            "",
            true,
            8,
        );
        let text = lines.join("\n");
        assert!(text.contains("Select a connection"), "{text}");
        assert!(!text.contains("No connection"), "{text}");
    }

    #[test]
    fn sidebar_offline_closed_session_shows_disconnected_marker() {
        let mut explorer = ExplorerState::default();
        explorer.offline = true;
        let lines = super::render_sidebar(
            &explorer,
            &[connection_row("prod", 0)],
            "prod",
            true,
            8,
        );
        let text = lines.join("\n");
        assert!(text.contains("○ prod"), "{text}");
        assert!(!text.contains("● prod"), "{text}");
        assert!(text.contains("Catalog — offline"), "{text}");
        assert!(!text.contains("[offline]"), "{text}");
        assert!(text.contains("No catalog"), "{text}");
    }

    #[test]
    fn sidebar_connected_row_uses_filled_marker() {
        let explorer = ExplorerState::default();
        let lines = super::render_sidebar(
            &explorer,
            &[connection_row("prod", 1)],
            "prod",
            true,
            8,
        );
        let text = lines.join("\n");
        assert!(text.contains("● prod"), "{text}");
    }

    #[test]
    fn sidebar_connection_cursor_only_on_focused_row() {
        let mut explorer = ExplorerState::default();
        explorer.sidebar_focus = SidebarFocus::Connections;
        explorer.connection_cursor = 1;
        let lines = super::render_sidebar(
            &explorer,
            &[connection_row("a", 0), connection_row("b", 0)],
            "",
            true,
            8,
        );
        assert!(lines.iter().any(|line| line.starts_with("> ○ b")));
        assert!(lines.iter().any(|line| line.starts_with("  ○ a")));
    }

    #[test]
    fn leaves_have_no_twistie() {
        let mut explorer = ExplorerState::default();
        explorer.replace_roots(CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("col:id"),
                ObjectKind::Column,
                QualifiedName::new(Some("db"), Some("public"), "id"),
                None,
            )],
            restrictions: vec![],
        });
        explorer.select(ObjectId::new("col:id"));
        let lines = super::render_lines(&explorer);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("id") && !line.contains('▸') && !line.contains('▾')),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("actions:")),
            "{lines:?}"
        );
    }
}
