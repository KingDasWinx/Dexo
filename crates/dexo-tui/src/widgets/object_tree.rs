use crate::screens::explorer::{ExplorerNode, ExplorerState, NodeState};

pub fn render_lines(state: &ExplorerState) -> Vec<String> {
    let mut lines = Vec::new();
    if state.offline {
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
    collect(&state.roots, state, 0, &mut lines);
    if let Some(node) = state.selected_node() {
        let actions = crate::screens::explorer::ExplorerAction::all()
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("actions: {actions}"));
        lines.push(format!("selected: {}", node.qualified));
    }
    if let Some(copied) = &state.copied {
        lines.push(format!("copied: {copied}"));
    }
    if lines.is_empty() {
        lines.push("No connection".into());
    }
    lines
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
            lines.push(format!(
                "{}{marker} {fav}{}{badge}",
                "  ".repeat(depth),
                node.label
            ));
        }
        if node.expanded {
            collect(&node.children, state, depth + 1, lines);
        }
    }
}
