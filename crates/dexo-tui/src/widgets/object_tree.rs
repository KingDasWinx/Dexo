use crate::palette::scroll_to_selection;
use crate::screens::explorer::{ExplorerNode, ExplorerState, NodeState};

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
    let mut footer = Vec::new();
    if let Some(node) = state.selected_node() {
        let actions = crate::screens::explorer::ExplorerAction::all()
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>()
            .join(" ");
        footer.push(format!("actions: {actions}"));
        footer.push(format!("selected: {}", node.qualified));
        if crate::screens::explorer::opens_table_data(&node.kind) {
            footer.push("Enter abre a table".into());
        }
    }
    if let Some(copied) = &state.copied {
        footer.push(format!("copied: {copied}"));
    }
    let tree = window_tree(state, &tree, viewport_rows, header.len() + footer.len());
    let mut lines = header;
    lines.extend(tree);
    lines.extend(footer);
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
            let marker = if node.expanded { "▾" } else { "▸" };
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
                "{cursor}{}{marker} {fav}{}{badge}",
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
}
