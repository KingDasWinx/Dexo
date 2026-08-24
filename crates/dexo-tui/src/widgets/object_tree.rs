use crate::palette::scroll_to_selection;
use crate::screens::explorer::{ExplorerNode, ExplorerState, NodeState};
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

pub fn render_visible(state: &ExplorerState, viewport_rows: Option<usize>) -> Vec<String> {
    let mut header = Vec::new();
    if state.offline {
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
        lines.push("No connection".into());
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
