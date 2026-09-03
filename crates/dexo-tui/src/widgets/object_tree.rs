use crate::palette::scroll_to_selection;
use crate::screens::connections::ConnectionRow;
use crate::screens::explorer::{ExplorerNode, ExplorerState, NodeState, is_connection_node};
use dexo_driver_api::ObjectId;

/// Row layout of the sidebar. `render_sidebar` and the mouse hit map both read
/// it, so a click always lands on the node drawn under the cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarLayout {
    /// Rows drawn above the tree body.
    pub header_rows: usize,
    /// Chrome rows the tree body draws before its first node.
    pub chrome_rows: usize,
    /// Scroll offset of the first tree node drawn.
    pub offset: usize,
    /// Tree nodes drawn, top to bottom.
    pub nodes: Vec<ObjectId>,
}

impl SidebarLayout {
    pub fn node_row(&self, index: usize) -> usize {
        self.header_rows + self.chrome_rows + index
    }
}

pub fn sidebar_layout(
    state: &ExplorerState,
    profiles: usize,
    _active_connection: &str,
    viewport_rows: usize,
) -> SidebarLayout {
    let header_rows = if profiles == 0 { 2 } else { 1 };
    let body_rows = viewport_rows.saturating_sub(header_rows).max(1);
    if profiles == 0 || state.roots.is_empty() {
        return SidebarLayout {
            header_rows,
            chrome_rows: 0,
            offset: 0,
            nodes: Vec::new(),
        };
    }
    let chrome_rows = chrome_lines(state, false).len();
    let ids = state.visible_ids();
    let tree_rows = body_rows.saturating_sub(chrome_rows).max(1);
    let offset = scroll_to_selection(state.selected_index(), state.offset, ids.len(), tree_rows);
    SidebarLayout {
        header_rows,
        chrome_rows,
        offset,
        nodes: ids.into_iter().skip(offset).take(tree_rows).collect(),
    }
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
    let layout = sidebar_layout(state, profiles.len(), active_connection, viewport_rows);
    let connected = if unicode { "●" } else { "*" };
    let offline = if unicode { "○" } else { "o" };
    let mut lines = vec!["Connections  [n]ew [e]dit".into()];
    if profiles.is_empty() {
        lines.push("No connections — press n".into());
        return lines;
    }
    debug_assert_eq!(lines.len(), layout.header_rows);
    let mut tree = Vec::new();
    collect(
        &state.roots,
        state,
        profiles,
        None,
        0,
        &mut tree,
        connected,
        offline,
    );
    let mut body = chrome_lines(state, false);
    body.extend(
        tree.into_iter()
            .skip(layout.offset)
            .take(layout.nodes.len()),
    );
    if body.is_empty() {
        body.push(if active_connection.is_empty() {
            "Select a connection".into()
        } else {
            "No objects".into()
        });
    }
    lines.extend(body);
    lines
}

pub fn render_visible(state: &ExplorerState, viewport_rows: Option<usize>) -> Vec<String> {
    render_visible_inner(state, viewport_rows, true)
}

fn chrome_lines(state: &ExplorerState, show_offline_chrome: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if show_offline_chrome && state.offline {
        lines.push("[offline]".into());
    }
    if !state.search.is_empty() {
        lines.push(format!("search:{}", state.search));
    }
    if !state.filter_name.is_empty() || state.filter_kind.is_some() || state.favorites_only {
        lines.push(format!(
            "filter:{} kind:{} fav:{}",
            state.filter_name,
            state.filter_kind.as_deref().unwrap_or("-"),
            state.favorites_only
        ));
    }
    lines
}

fn render_visible_inner(
    state: &ExplorerState,
    viewport_rows: Option<usize>,
    show_offline_chrome: bool,
) -> Vec<String> {
    let header = chrome_lines(state, show_offline_chrome);
    let mut tree = Vec::new();
    collect(&state.roots, state, &[], None, 0, &mut tree, "●", "○");
    let tree = window_tree(state, &tree, viewport_rows, header.len());
    let mut lines = header;
    lines.extend(tree);
    if lines.is_empty() {
        lines.push("No objects".into());
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

fn connection_sessions(profiles: &[ConnectionRow], name: &str) -> usize {
    profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| row.sessions)
        .unwrap_or(0)
}

fn collect(
    nodes: &[ExplorerNode],
    state: &ExplorerState,
    profiles: &[ConnectionRow],
    owner: Option<&str>,
    depth: usize,
    lines: &mut Vec<String>,
    connected: &str,
    offline: &str,
) {
    for node in nodes {
        let owner = crate::screens::explorer::connection_name(&node.id).or(owner);
        if state.matches(node) {
            let cursor = if state.is_selected(owner, &node.id) {
                ">"
            } else {
                " "
            };
            let twistie = if node.can_expand() {
                if node.expanded { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let badge = match node.state {
                NodeState::Loading(_) => " [loading]",
                NodeState::Restricted => " [restricted]",
                NodeState::Error { .. } => " [error]",
                NodeState::Stale => " [stale]",
                NodeState::Collapsed | NodeState::Expanded => {
                    if is_connection_node(node) && state.offline {
                        " [offline]"
                    } else {
                        ""
                    }
                }
            };
            let fav = if node.favorite { "*" } else { "" };
            let label = if is_connection_node(node) {
                let marker = if connection_sessions(profiles, &node.label) > 0 {
                    connected
                } else {
                    offline
                };
                format!("{marker} {label}{twistie}{fav}{badge}", label = node.label)
            } else {
                format!("{twistie}{fav}{}{badge}", node.label)
            };
            lines.push(format!("{cursor} {}{label}", "  ".repeat(depth)));
        }
        if node.expanded {
            collect(
                &node.children,
                state,
                profiles,
                owner,
                depth + 1,
                lines,
                connected,
                offline,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

    use crate::screens::explorer::{ExplorerState, connection_id};

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
        explorer.sync_connection_roots(&[connection_row("prod", 1)], "prod");
        explorer.replace_connection_catalog(
            "prod",
            CatalogList {
                objects: (0..12)
                    .map(|i| table(&format!("table:{i}"), &format!("t{i}")))
                    .collect(),
                restrictions: vec![],
            },
            false,
        );
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
    }

    use crate::screens::connections::ConnectionRow;
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
    fn connected_profile_renders_as_expandable_folder() {
        let mut explorer = ExplorerState::default();
        explorer.sync_connection_roots(&[connection_row("prod", 1)], "prod");
        explorer.replace_connection_catalog(
            "prod",
            CatalogList {
                objects: vec![table("table:orders", "orders")],
                restrictions: vec![],
            },
            false,
        );
        explorer.select(connection_id("prod"));
        let lines = super::render_sidebar(&explorer, &[connection_row("prod", 1)], "prod", true, 8);
        let text = lines.join("\n");
        assert!(text.contains("● prod"), "{text}");
        assert!(text.contains("▾"), "{text}");
        assert!(text.contains("orders"), "{text}");
        assert!(!text.contains("Catalog"), "{text}");
    }

    #[test]
    fn disconnected_profile_shows_offline_marker_without_catalog_header() {
        let mut explorer = ExplorerState::default();
        explorer.sync_connection_roots(&[connection_row("prod", 0)], "");
        explorer.select(connection_id("prod"));
        let lines = super::render_sidebar(&explorer, &[connection_row("prod", 0)], "", true, 8);
        let text = lines.join("\n");
        assert!(text.contains("○ prod"), "{text}");
        assert!(text.contains('▸'), "{text}");
        assert!(!text.contains("Catalog"), "{text}");
    }

    #[test]
    fn sidebar_ascii_offline_marker_is_visible() {
        let mut explorer = ExplorerState::default();
        explorer.sync_connection_roots(&[connection_row("prod", 0)], "");
        let lines = super::render_sidebar(&explorer, &[connection_row("prod", 0)], "", false, 8);
        assert!(
            lines.iter().any(|line| line.contains(" o prod")),
            "{lines:?}"
        );
    }

    #[test]
    fn leaves_have_no_twistie() {
        let mut explorer = ExplorerState::default();
        explorer.sync_connection_roots(&[connection_row("prod", 1)], "prod");
        explorer.replace_connection_catalog(
            "prod",
            CatalogList {
                objects: vec![CatalogObject::new(
                    ObjectId::new("col:id"),
                    ObjectKind::Column,
                    QualifiedName::new(Some("db"), Some("public"), "id"),
                    None,
                )],
                restrictions: vec![],
            },
            false,
        );
        explorer.sync_connection_roots(&[connection_row("prod", 1)], "prod");
        explorer.select(ObjectId::new("col:id"));
        let lines = super::render_lines(&explorer);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("id") && !line.contains('▸') && !line.contains('▾')),
            "{lines:?}"
        );
    }
}
