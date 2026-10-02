use dexo_driver_api::{ExplainPlan, ExplainProvider, ExplainRequest, PlanNode};

use crate::error::{AppError, ErrorCategory};
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

/// One node of a plan as it is drawn: where it sits in the tree, what it is, and its
/// figures. The TUI styles it and fits it to the pane; the CLI prints it as is.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanRow {
    /// Indentation and tree connectors, such as `   └─ `.
    pub prefix: String,
    /// `Seq Scan on customers c`.
    pub label: String,
    /// What the node works by -- condition, key, index -- or empty.
    pub detail: String,
    pub cost: Option<f64>,
    pub estimated_rows: Option<f64>,
    pub actual_rows: Option<f64>,
    /// The node's time over all of its loops; servers report it per loop.
    pub total_ms: Option<f64>,
    /// `total_ms` less its children's: the time spent in the node itself.
    pub own_ms: Option<f64>,
    /// The same for cost, which is how an estimated plan ranks its nodes.
    pub own_cost: Option<f64>,
    pub loops: Option<u64>,
    /// The node with the most time (or, estimated, cost) of its own.
    pub hottest: bool,
    /// How many times off the row estimate was, when it was off 10× or more.
    pub misestimate: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanView {
    pub analyzed: bool,
    /// `Analyzed · 4.86 ms execution · 10.2 ms planning`, or `Estimated · cost 350`.
    pub headline: String,
    /// Plan order, root first.
    pub rows: Vec<PlanRow>,
}

struct Glyphs {
    branch: &'static str,
    last: &'static str,
    pipe: &'static str,
    arrow: &'static str,
    hot: &'static str,
}

fn glyphs(unicode: bool) -> Glyphs {
    if unicode {
        Glyphs {
            branch: "├─ ",
            last: "└─ ",
            pipe: "│  ",
            arrow: "→",
            hot: "▲",
        }
    } else {
        Glyphs {
            branch: "|- ",
            last: "`- ",
            pipe: "|  ",
            arrow: "->",
            hot: "^",
        }
    }
}

pub fn plan_view(plan: &ExplainPlan, unicode: bool) -> PlanView {
    let analyzed = plan.execution_ms.is_some() || has_actuals(&plan.root);
    let glyphs = glyphs(unicode);
    let mut rows = Vec::new();
    flatten(
        &plan.root,
        String::new(),
        String::new(),
        false,
        &glyphs,
        &mut rows,
    );
    if rows.len() > 1 {
        let own = |row: &PlanRow| if analyzed { row.own_ms } else { row.own_cost };
        let hottest = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| own(row).map(|value| (index, value)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index);
        if let Some(index) = hottest {
            rows[index].hottest = true;
        }
    }
    PlanView {
        analyzed,
        headline: headline(plan, analyzed),
        rows,
    }
}

fn has_actuals(node: &PlanNode) -> bool {
    node.actual.rows.is_some() || node.children.iter().any(has_actuals)
}

fn headline(plan: &ExplainPlan, analyzed: bool) -> String {
    if analyzed {
        let mut parts = vec!["Analyzed".to_string()];
        if let Some(ms) = plan.execution_ms {
            parts.push(format!("{} execution", fmt_ms(ms)));
        }
        if let Some(ms) = plan.planning_ms {
            parts.push(format!("{} planning", fmt_ms(ms)));
        }
        parts.join(" · ")
    } else {
        match plan.root.estimates.cost {
            Some(cost) => format!("Estimated · cost {}", fmt_cost(cost)),
            None => "Estimated".into(),
        }
    }
}

/// Pushes `node` and its subtree in plan order and returns the node's total time and
/// cost, so its parent can take them off its own.
fn flatten(
    node: &PlanNode,
    prefix: String,
    indent: String,
    under_limit: bool,
    glyphs: &Glyphs,
    rows: &mut Vec<PlanRow>,
) -> (Option<f64>, Option<f64>) {
    let index = rows.len();
    let label = match &node.relation {
        Some(relation) => format!("{} on {relation}", node.kind),
        None => node.kind.clone(),
    };
    let loops = node.loops;
    let total_ms = node
        .actual
        .time_ms
        .map(|ms| ms * loops.unwrap_or(1).max(1) as f64);
    let misestimate = match (node.estimates.rows, node.actual.rows) {
        // A LIMIT stops pulling once it has its rows, so the node under it returns fewer
        // than it was estimated to make; that is the limit working, not a bad estimate.
        (Some(estimated), Some(actual)) if under_limit && actual < estimated => None,
        (Some(estimated), Some(actual)) => {
            let (estimated, actual) = (estimated.max(1.0), actual.max(1.0));
            let ratio = estimated.max(actual) / estimated.min(actual);
            (ratio >= 10.0).then_some(ratio)
        }
        _ => None,
    };
    rows.push(PlanRow {
        prefix,
        label,
        detail: node.detail.clone().unwrap_or_default(),
        cost: node.estimates.cost,
        estimated_rows: node.estimates.rows,
        actual_rows: node.actual.rows,
        total_ms,
        own_ms: None,
        own_cost: None,
        loops,
        hottest: false,
        misestimate,
    });
    let mut children_ms = 0.0;
    let mut children_cost = 0.0;
    let count = node.children.len();
    for (position, child) in node.children.iter().enumerate() {
        let last = position + 1 == count;
        let connector = if last { glyphs.last } else { glyphs.branch };
        let carry = if last { "   " } else { glyphs.pipe };
        let (ms, cost) = flatten(
            child,
            format!("{indent}{connector}"),
            format!("{indent}{carry}"),
            node.kind.starts_with("Limit"),
            glyphs,
            rows,
        );
        children_ms += ms.unwrap_or(0.0);
        children_cost += cost.unwrap_or(0.0);
    }
    // ponytail: a parallel plan sums its workers' loops, so children can outrun their
    // Gather and its own time clamps at zero; divide by workers if that ever misleads.
    rows[index].own_ms = total_ms.map(|ms| (ms - children_ms).max(0.0));
    rows[index].own_cost = node
        .estimates
        .cost
        .map(|cost| (cost - children_cost).max(0.0));
    (total_ms, node.estimates.cost)
}

/// The tags that follow a row's label: the hotspot, and an estimate that was far off.
pub fn row_tags(row: &PlanRow, analyzed: bool, unicode: bool) -> Vec<String> {
    let glyphs = glyphs(unicode);
    let mut tags = Vec::new();
    if row.hottest {
        let word = if analyzed { "slowest" } else { "costliest" };
        tags.push(format!("{} {word}", glyphs.hot));
    }
    if let Some(ratio) = row.misestimate {
        tags.push(format!("rows off x{}", fmt_ratio(ratio)));
    }
    tags
}

pub fn tree_columns(analyzed: bool) -> &'static [&'static str] {
    if analyzed {
        &["rows est → act", "time", "loops"]
    } else {
        &["cost", "rows"]
    }
}

pub fn tree_cells(row: &PlanRow, analyzed: bool, unicode: bool) -> Vec<String> {
    if analyzed {
        vec![
            format!(
                "{} {} {}",
                fmt_rows(row.estimated_rows),
                glyphs(unicode).arrow,
                fmt_rows(row.actual_rows)
            ),
            row.total_ms.map(fmt_ms).unwrap_or_else(|| "-".into()),
            row.loops
                .map(|loops| loops.to_string())
                .unwrap_or_else(|| "-".into()),
        ]
    } else {
        vec![
            row.cost.map(fmt_cost).unwrap_or_else(|| "-".into()),
            fmt_rows(row.estimated_rows),
        ]
    }
}

pub fn table_columns(analyzed: bool) -> &'static [&'static str] {
    if analyzed {
        &["self", "time", "rows est", "rows act", "loops"]
    } else {
        &["self cost", "cost", "rows"]
    }
}

pub fn table_cells(row: &PlanRow, analyzed: bool) -> Vec<String> {
    let dash = || "-".to_string();
    if analyzed {
        vec![
            row.own_ms.map(fmt_ms).unwrap_or_else(dash),
            row.total_ms.map(fmt_ms).unwrap_or_else(dash),
            fmt_rows(row.estimated_rows),
            fmt_rows(row.actual_rows),
            row.loops
                .map(|loops| loops.to_string())
                .unwrap_or_else(dash),
        ]
    } else {
        vec![
            row.own_cost.map(fmt_cost).unwrap_or_else(dash),
            row.cost.map(fmt_cost).unwrap_or_else(dash),
            fmt_rows(row.estimated_rows),
        ]
    }
}

/// The table view lists nodes by what they spent themselves, the most first: where the
/// time went, without walking the tree.
pub fn table_order(view: &PlanView) -> Vec<usize> {
    let mut order: Vec<usize> = (0..view.rows.len()).collect();
    let own = |index: &usize| {
        let row = &view.rows[*index];
        if view.analyzed {
            row.own_ms
        } else {
            row.own_cost
        }
        .unwrap_or(-1.0)
    };
    order.sort_by(|a, b| own(b).total_cmp(&own(a)));
    order
}

/// The Summary view: what the plan cost, where it spent it, which estimates were off,
/// and, when there is one, what changed since the last plan of the same statement.
pub fn summary_lines(plan: &ExplainPlan, deltas: &[NodeDelta], unicode: bool) -> Vec<String> {
    let view = plan_view(plan, unicode);
    let scans = view
        .rows
        .iter()
        .filter(|row| row.label.to_ascii_lowercase().contains("scan"))
        .count();
    let mut lines = vec![
        view.headline.clone(),
        format!(
            "{} {} · {scans} {}",
            view.rows.len(),
            if view.rows.len() == 1 {
                "node"
            } else {
                "nodes"
            },
            if scans == 1 { "scan" } else { "scans" }
        ),
    ];
    if let Some(row) = view.rows.iter().find(|row| row.hottest) {
        if view.analyzed {
            let whole = plan
                .execution_ms
                .or(view.rows.first().and_then(|root| root.total_ms));
            let own = row.own_ms.unwrap_or(0.0);
            let share = whole
                .filter(|whole| *whole > 0.0)
                .map(|whole| format!(", {:.0}% of the run", own / whole * 100.0))
                .unwrap_or_default();
            lines.push(format!(
                "Slowest: {} -- {} of its own{share}",
                row.label,
                fmt_ms(own)
            ));
        } else {
            lines.push(format!(
                "Costliest: {} -- cost {} of its own",
                row.label,
                fmt_cost(row.own_cost.unwrap_or(0.0))
            ));
        }
    }
    let mut off: Vec<&PlanRow> = view
        .rows
        .iter()
        .filter(|row| row.misestimate.is_some())
        .collect();
    off.sort_by(|a, b| {
        let ratio = |row: &PlanRow| row.misestimate.unwrap_or(0.0);
        ratio(b).total_cmp(&ratio(a))
    });
    for row in off.iter().take(5) {
        lines.push(format!(
            "Off estimate: {} -- estimated {} rows, got {}",
            row.label,
            fmt_rows(row.estimated_rows),
            fmt_rows(row.actual_rows)
        ));
    }
    if view.analyzed && off.is_empty() {
        lines.push("Row estimates held within 10x everywhere.".into());
    }
    if !deltas.is_empty() {
        lines.push(String::new());
        lines.push("Since the last plan of this statement:".into());
        lines.extend(
            deltas
                .iter()
                .map(|delta| format!("  {}", describe_delta(delta))),
        );
    }
    lines
}

pub fn describe_delta(delta: &NodeDelta) -> String {
    let named = |kind: &str, relation: &Option<String>| match relation {
        Some(relation) => format!("{kind} on {relation}"),
        None => kind.to_string(),
    };
    match delta {
        NodeDelta::Added { kind, relation, .. } => format!("added    {}", named(kind, relation)),
        NodeDelta::Removed { kind, relation, .. } => {
            format!("removed  {}", named(kind, relation))
        }
        NodeDelta::Changed {
            kind,
            relation,
            field,
            ..
        } => match field.as_str() {
            "kind/relation" => format!("now      {}", named(kind, relation)),
            "estimates.rows" => format!("estimate {}: row estimate changed", named(kind, relation)),
            _ => format!("rows     {}: actual rows changed", named(kind, relation)),
        },
    }
}

/// Plain text for the CLI, at the width its content needs.
pub fn render_tree(plan: &ExplainPlan) -> String {
    let view = plan_view(plan, true);
    let left: Vec<String> = view
        .rows
        .iter()
        .map(|row| {
            let mut text = format!("{}{}", row.prefix, row.label);
            for tag in row_tags(row, view.analyzed, true) {
                text.push_str("  ");
                text.push_str(&tag);
            }
            if !row.detail.is_empty() {
                text.push_str("  ");
                text.push_str(&row.detail);
            }
            text
        })
        .collect();
    let cells = view
        .rows
        .iter()
        .map(|row| tree_cells(row, view.analyzed, true))
        .collect::<Vec<_>>();
    let mut out = vec![view.headline.clone()];
    out.extend(columns_text(&left, &cells, tree_columns(view.analyzed)));
    out.join("\n")
}

pub fn render_table(plan: &ExplainPlan) -> String {
    let view = plan_view(plan, true);
    let order = table_order(&view);
    let left: Vec<String> = order
        .iter()
        .map(|index| view.rows[*index].label.clone())
        .collect();
    let cells: Vec<Vec<String>> = order
        .iter()
        .map(|index| table_cells(&view.rows[*index], view.analyzed))
        .collect();
    let mut out = vec![view.headline.clone()];
    out.extend(columns_text(&left, &cells, table_columns(view.analyzed)));
    out.join("\n")
}

pub fn render_summary(plan: &ExplainPlan) -> String {
    summary_lines(plan, &[], true).join("\n")
}

/// A header row and the rows under it: text on the left, figures right-aligned in
/// columns as wide as their widest entry.
fn columns_text(left: &[String], cells: &[Vec<String>], columns: &[&str]) -> Vec<String> {
    let width = |text: &str| text.chars().count();
    let left_width = left.iter().map(|text| width(text)).max().unwrap_or(0);
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(index, header)| {
            cells
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| width(cell))
                .chain([width(header)])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |left: &str, cells: &[String]| {
        let mut text = format!("{left:<left_width$}");
        for (cell, width) in cells.iter().zip(&widths) {
            text.push_str(&format!("  {cell:>width$}"));
        }
        text.trim_end().to_string()
    };
    let header: Vec<String> = columns.iter().map(|header| header.to_string()).collect();
    let mut out = vec![line("", &header)];
    out.extend(
        left.iter()
            .zip(cells)
            .map(|(left, cells)| line(left, cells)),
    );
    out
}

pub fn fmt_ms(ms: f64) -> String {
    if ms < 0.01 {
        "<0.01 ms".into()
    } else if ms < 10.0 {
        format!("{ms:.2} ms")
    } else if ms < 1000.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

fn fmt_rows(rows: Option<f64>) -> String {
    match rows {
        None => "?".into(),
        Some(rows) if rows < 100_000.0 => format!("{}", rows.round() as u64),
        Some(rows) if rows < 1_000_000.0 => format!("{:.0}k", rows / 1_000.0),
        Some(rows) if rows < 1_000_000_000.0 => format!("{:.1}M", rows / 1_000_000.0),
        Some(rows) => format!("{:.1}B", rows / 1_000_000_000.0),
    }
}

fn fmt_cost(cost: f64) -> String {
    if cost >= 1000.0 {
        format!("{cost:.0}")
    } else {
        format!("{cost:.2}")
    }
}

fn fmt_ratio(ratio: f64) -> String {
    if ratio >= 100.0 {
        format!("{ratio:.0}")
    } else {
        format!("{ratio:.1}")
    }
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
                } else if both(other.estimates.rows, node.estimates.rows) {
                    deltas.push(NodeDelta::Changed {
                        path: path.clone(),
                        kind: node.kind.clone(),
                        relation: node.relation.clone(),
                        field: "estimates.rows".into(),
                    });
                } else if both(other.actual.rows, node.actual.rows) {
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

/// The one statement an EXPLAIN covers. A file holding several used to reach the server
/// whole and come back as Postgres' "cannot insert multiple commands into a prepared
/// statement".
pub fn single_statement(sql: &str, dialect: dexo_sql::Dialect) -> Result<&str, AppError> {
    match dexo_sql::split_statements_in(sql, dialect).as_slice() {
        [span] => Ok(sql[span.byte_range.clone()].trim().trim_end_matches(';')),
        [] => Err(AppError::new(
            ErrorCategory::Syntax,
            "there is no statement to explain",
        )),
        many => Err(AppError::new(
            ErrorCategory::Syntax,
            format!(
                "EXPLAIN covers one statement and this has {}; pass the one to explain",
                many.len()
            ),
        )),
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

/// Two figures that both exist and differ. An estimated plan has no actual rows, and
/// comparing it with its analyzed run used to flag every node as changed.
fn both(left: Option<f64>, right: Option<f64>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}

fn index_nodes<'a>(node: &'a PlanNode, path: &str, out: &mut Vec<(String, &'a PlanNode)>) {
    out.push((path.to_string(), node));
    for (index, child) in node.children.iter().enumerate() {
        index_nodes(child, &format!("{path}.{index}"), out);
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeDelta, compare_plans, plan_view, render_summary, render_table, render_tree};
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
            detail: None,
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
                node("Seq Scan", Some("users"), 100.0, Some(1.0), vec![]),
            ],
        ));
        let tree = render_tree(&plan);
        assert!(tree.starts_with("Analyzed · 0.50 ms execution · 0.10 ms planning"));
        assert!(tree.contains("├─ Seq Scan on orders"));
        assert!(tree.contains("└─ Seq Scan on users"));
        assert!(tree.contains("rows off x100"), "{tree}");
        assert!(tree.contains("1000 → 1000"));
        let table = render_table(&plan);
        assert!(table.contains("self"));
        let summary = render_summary(&plan);
        assert!(summary.contains("3 nodes · 2 scans"));
        assert!(summary.contains("Off estimate: Seq Scan on users -- estimated 100 rows, got 1"));
    }

    /// The node that spent the most time of its own is the hotspot -- not the root, whose
    /// time includes everything under it.
    #[test]
    fn the_hotspot_is_the_most_time_of_its_own() {
        let mut root = node("Hash Join", None, 10.0, Some(10.0), vec![]);
        root.actual.time_ms = Some(5.0);
        let mut slow = node("Seq Scan", Some("orders"), 10.0, Some(10.0), vec![]);
        slow.actual.time_ms = Some(4.0);
        let mut quick = node("Seq Scan", Some("users"), 10.0, Some(10.0), vec![]);
        quick.actual.time_ms = Some(0.5);
        root.children = vec![slow, quick];
        let view = plan_view(&plan(root), true);
        let hottest: Vec<&str> = view
            .rows
            .iter()
            .filter(|row| row.hottest)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(hottest, ["Seq Scan on orders"]);
        assert_eq!(view.rows[0].own_ms, Some(0.5));
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

    /// An estimated plan has no actual rows; comparing it with its own analyzed run is
    /// not a change in every node.
    #[test]
    fn estimated_and_analyzed_runs_of_one_plan_do_not_differ() {
        let analyzed = plan(node("Seq Scan", Some("items"), 10.0, Some(12.0), vec![]));
        let mut estimated = analyzed.clone();
        estimated.root.actual.rows = None;
        assert!(compare_plans(&estimated, &analyzed).is_empty());
    }
}
