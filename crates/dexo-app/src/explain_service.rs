use dexo_driver_api::{ExplainPlan, ExplainProvider, ExplainRequest, PlanNode};

use crate::error::AppError;
use crate::query_service::map_driver_error;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub enum NodeDelta {
    Added {
        path: String,
        kind: String,
        relation: Option<String>,
    },
    Removed {
        path: String,
        kind: String,
        relation: Option<String>,
    },
    Changed {
        path: String,
        kind: String,
        relation: Option<String>,
        field: String,
    },
}

pub fn render_tree(plan: &ExplainPlan) -> String {
    let mut lines = vec![format!(
        "planning_ms={} execution_ms={}",
        fmt_opt(plan.planning_ms),
        fmt_opt(plan.execution_ms)
    )];
    walk_tree(&plan.root, 0, &mut lines);
    lines.join("\n")
}

pub fn render_table(plan: &ExplainPlan) -> String {
    let mut lines =
        vec!["path\tkind\trelation\tcost\trows_est\trows_act\ttime_ms\tloops\theuristic".into()];
    walk_table(&plan.root, "0", &mut lines);
    lines.join("\n")
}

pub fn render_summary(plan: &ExplainPlan) -> String {
    let mut nodes = 0_usize;
    let mut scans = 0_usize;
    summarize(&plan.root, &mut nodes, &mut scans);
    format!(
        "nodes={nodes} scans={scans} planning_ms={} execution_ms={} raw_bytes={}",
        fmt_opt(plan.planning_ms),
        fmt_opt(plan.execution_ms),
        plan.raw.len()
    )
}

pub fn compare_plans(before: &ExplainPlan, after: &ExplainPlan) -> Vec<NodeDelta> {
    let mut before_map = Vec::new();
    let mut after_map = Vec::new();
    index_nodes(&before.root, "0", &mut before_map);
    index_nodes(&after.root, "0", &mut after_map);
    let mut deltas = Vec::new();
    for (path, node) in &before_map {
        match after_map.iter().find(|(other, _)| other == path) {
            None => deltas.push(NodeDelta::Removed {
                path: path.clone(),
                kind: node.kind.clone(),
                relation: node.relation.clone(),
            }),
            Some((_, other)) => {
                if other.kind != node.kind || other.relation != node.relation {
                    deltas.push(NodeDelta::Changed {
                        path: path.clone(),
                        kind: other.kind.clone(),
                        relation: other.relation.clone(),
                        field: "kind/relation".into(),
                    });
                } else if other.estimates.rows != node.estimates.rows {
                    deltas.push(NodeDelta::Changed {
                        path: path.clone(),
                        kind: node.kind.clone(),
                        relation: node.relation.clone(),
                        field: "estimates.rows".into(),
                    });
                } else if other.actual.rows != node.actual.rows {
                    deltas.push(NodeDelta::Changed {
                        path: path.clone(),
                        kind: node.kind.clone(),
                        relation: node.relation.clone(),
                        field: "actual.rows".into(),
                    });
                }
            }
        }
    }
    for (path, node) in &after_map {
        if before_map.iter().all(|(other, _)| other != path) {
            deltas.push(NodeDelta::Added {
                path: path.clone(),
                kind: node.kind.clone(),
                relation: node.relation.clone(),
            });
        }
    }
    deltas
}

pub fn ratio_label(estimate: Option<f64>, actual: Option<f64>) -> Option<String> {
    let estimate = estimate?;
    let actual = actual?;
    if estimate <= 0.0 {
        return Some("actual vs estimate ratio skipped (heuristic)".into());
    }
    let ratio = actual / estimate;
    if ratio >= 10.0 {
        Some("actual rows >> estimate (heuristic)".into())
    } else if ratio <= 0.1 {
        Some("actual rows << estimate (heuristic)".into())
    } else {
        None
    }
}

pub struct ExplainService;

impl ExplainService {
    pub async fn explain(
        provider: &dyn ExplainProvider,
        request: ExplainRequest,
    ) -> Result<ExplainPlan, AppError> {
        provider.explain(request).await.map_err(map_driver_error)
    }
}

fn walk_tree(node: &PlanNode, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let heuristic = ratio_label(node.estimates.rows, node.actual.rows)
        .map(|label| format!(" {label}"))
        .unwrap_or_default();
    lines.push(format!(
        "{indent}{} rel={} cost={} rows={}/{} time_ms={} loops={}{heuristic}",
        node.kind,
        node.relation.as_deref().unwrap_or("-"),
        fmt_opt(node.estimates.cost),
        fmt_opt(node.estimates.rows),
        fmt_opt(node.actual.rows),
        fmt_opt(node.actual.time_ms),
        node.loops
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into()),
    ));
    for child in &node.children {
        walk_tree(child, depth + 1, lines);
    }
}

fn walk_table(node: &PlanNode, path: &str, lines: &mut Vec<String>) {
    let heuristic = ratio_label(node.estimates.rows, node.actual.rows).unwrap_or_default();
    lines.push(format!(
        "{path}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{heuristic}",
        node.kind,
        node.relation.as_deref().unwrap_or("-"),
        fmt_opt(node.estimates.cost),
        fmt_opt(node.estimates.rows),
        fmt_opt(node.actual.rows),
        fmt_opt(node.actual.time_ms),
        node.loops
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into()),
    ));
    for (index, child) in node.children.iter().enumerate() {
        walk_table(child, &format!("{path}.{index}"), lines);
    }
}

fn summarize(node: &PlanNode, nodes: &mut usize, scans: &mut usize) {
    *nodes += 1;
    if node.kind.to_ascii_lowercase().contains("scan") {
        *scans += 1;
    }
    for child in &node.children {
        summarize(child, nodes, scans);
    }
}

fn index_nodes<'a>(node: &'a PlanNode, path: &str, out: &mut Vec<(String, &'a PlanNode)>) {
    out.push((path.to_string(), node));
    for (index, child) in node.children.iter().enumerate() {
        index_nodes(child, &format!("{path}.{index}"), out);
    }
}

fn fmt_opt(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number}"))
        .unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::{NodeDelta, compare_plans, ratio_label, render_summary, render_table, render_tree};
    use dexo_driver_api::{ExplainPlan, PlanMetrics, PlanNode};

    fn node(
        kind: &str,
        relation: Option<&str>,
        est: f64,
        act: Option<f64>,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            kind: kind.into(),
            relation: relation.map(str::to_string),
            estimates: PlanMetrics {
                cost: Some(1.0),
                rows: Some(est),
                width: None,
                time_ms: None,
            },
            actual: PlanMetrics {
                cost: None,
                rows: act,
                width: None,
                time_ms: act.map(|_| 0.2),
            },
            loops: Some(1),
            children,
            native: serde_json::json!({"Node Type": kind}),
        }
    }

    fn plan(root: PlanNode) -> ExplainPlan {
        ExplainPlan {
            planning_ms: Some(0.1),
            execution_ms: Some(0.5),
            raw: "{\"Plan\":{}}".into(),
            root,
        }
    }

    #[test]
    fn tree_table_summary_goldens() {
        let plan = plan(node(
            "Hash Join",
            None,
            200.0,
            Some(200.0),
            vec![
                node("Seq Scan", Some("orders"), 1000.0, Some(1000.0), vec![]),
                node("Seq Scan", Some("users"), 100.0, Some(100.0), vec![]),
            ],
        ));
        let tree = render_tree(&plan);
        assert!(tree.contains("Hash Join"));
        assert!(tree.contains("rel=orders"));
        assert!(tree.contains("rel=users"));
        let table = render_table(&plan);
        assert!(table.contains("0.0\tSeq Scan\torders"));
        assert!(table.contains("0.1\tSeq Scan\tusers"));
        let summary = render_summary(&plan);
        assert!(summary.contains("nodes=3"));
        assert!(summary.contains("scans=2"));
        assert!(summary.contains("raw_bytes="));
    }

    #[test]
    fn compare_uses_path_kind_relation() {
        let before = plan(node("Seq Scan", Some("items"), 10.0, Some(10.0), vec![]));
        let after = plan(node(
            "Index Scan",
            Some("items"),
            10.0,
            Some(10.0),
            vec![node(
                "Index Scan",
                Some("idx_items"),
                1.0,
                Some(1.0),
                vec![],
            )],
        ));
        let deltas = compare_plans(&before, &after);
        assert!(deltas.iter().any(|delta| matches!(
            delta,
            NodeDelta::Changed { path, field, .. } if path == "0" && field == "kind/relation"
        )));
        assert!(deltas.iter().any(|delta| matches!(
            delta,
            NodeDelta::Added { path, kind, relation } if path == "0.0" && kind == "Index Scan" && relation.as_deref() == Some("idx_items")
        )));
    }

    #[test]
    fn heuristic_ratio_is_labeled_not_certain() {
        assert_eq!(
            ratio_label(Some(1.0), Some(50.0)).as_deref(),
            Some("actual rows >> estimate (heuristic)")
        );
        assert_eq!(
            ratio_label(Some(100.0), Some(1.0)).as_deref(),
            Some("actual rows << estimate (heuristic)")
        );
        assert!(ratio_label(Some(10.0), Some(12.0)).is_none());
        assert!(ratio_label(None, Some(1.0)).is_none());
    }
}
