use dexo_driver_api::{
    DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics,
    PlanNode, TransactionControl, TransactionState,
};
use mysql_async::prelude::Queryable;

use crate::error::map_error;
use crate::session::MysqlSession;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeExplainFormat {
    Json,
    Tree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MysqlExplainCaps {
    pub json: bool,
    pub tree: bool,
    pub tree_analyze: bool,
    /// MariaDB's `ANALYZE FORMAT=JSON`, its form of EXPLAIN ANALYZE.
    pub json_analyze: bool,
}

impl MysqlExplainCaps {
    pub fn mysql() -> Self {
        Self {
            json: true,
            tree: true,
            tree_analyze: true,
            json_analyze: false,
        }
    }

    /// MariaDB has no FORMAT=TREE, plain or analyzed.
    pub fn mariadb() -> Self {
        Self {
            json: true,
            tree: false,
            tree_analyze: false,
            json_analyze: true,
        }
    }
}

pub fn select_format(
    analyze: bool,
    caps: MysqlExplainCaps,
) -> Result<NativeExplainFormat, DriverError> {
    if analyze {
        if caps.tree_analyze {
            return Ok(NativeExplainFormat::Tree);
        }
        if caps.json_analyze {
            return Ok(NativeExplainFormat::Json);
        }
        return Err(DriverError::unsupported(
            "EXPLAIN ANALYZE FORMAT=TREE is unavailable on this server version",
        ));
    }
    if caps.json {
        Ok(NativeExplainFormat::Json)
    } else if caps.tree {
        Ok(NativeExplainFormat::Tree)
    } else {
        Err(DriverError::unsupported(
            "no structured explain format is available",
        ))
    }
}

pub fn wrap_explain(sql: &str, format: NativeExplainFormat, analyze: bool) -> String {
    let inner = sql.trim().trim_end_matches(';');
    match (format, analyze) {
        (NativeExplainFormat::Json, false) => format!("EXPLAIN FORMAT=JSON {inner}"),
        (NativeExplainFormat::Json, true) => format!("ANALYZE FORMAT=JSON {inner}"),
        (NativeExplainFormat::Tree, true) => format!("EXPLAIN ANALYZE FORMAT=TREE {inner}"),
        (NativeExplainFormat::Tree, false) => format!("EXPLAIN FORMAT=TREE {inner}"),
    }
}

pub fn parse_json(raw: &str) -> Result<ExplainPlan, DriverError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        DriverError::new(
            DriverErrorCategory::Internal,
            format!("explain json: {error}"),
        )
    })?;
    // MySQL's `explain_json_format_version = 2` has no query block: read as the first
    // format, its plan came out empty.
    let iterators = value.get("query_block").is_none() && value.get("operation").is_some();
    let root = if iterators {
        parse_iterator(&value)
    } else {
        parse_value(&value)
    };
    // MariaDB's ANALYZE reports the whole run on the query block; the second format,
    // like TREE, on the root's last row.
    let execution_ms = if iterators {
        root.actual.time_ms
    } else {
        value
            .pointer("/query_block/r_total_time_ms")
            .and_then(json_f64)
    };
    Ok(ExplainPlan {
        planning_ms: None,
        execution_ms,
        root,
        raw: raw.to_string(),
    })
}

fn parse_value(value: &serde_json::Value) -> PlanNode {
    if let Some(block) = value.get("query_block") {
        return parse_block(block);
    }
    parse_block(value)
}

/// A step of the second JSON format: the iterator the TREE format draws, its
/// `operation` labelled the same way, with what it reads under `inputs` -- and a
/// subquery's under `inputs_from_select_list` and the like.
fn parse_iterator(value: &serde_json::Value) -> PlanNode {
    let operation = value
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let (kind, relation, detail) = split_tree_label(operation);
    PlanNode {
        kind,
        relation: value
            .get("table_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or(relation),
        detail,
        estimates: PlanMetrics {
            cost: value.get("estimated_total_cost").and_then(json_f64),
            rows: value.get("estimated_rows").and_then(json_f64),
            width: None,
            time_ms: None,
        },
        actual: PlanMetrics {
            cost: None,
            rows: value.get("actual_rows").and_then(json_f64),
            width: None,
            time_ms: value.get("actual_last_row_ms").and_then(json_f64),
        },
        loops: value
            .get("actual_loops")
            .and_then(json_f64)
            .map(|loops| loops as u64),
        children: value
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.starts_with("inputs"))
            .filter_map(|(_, inputs)| inputs.as_array())
            .flatten()
            .map(parse_iterator)
            .collect(),
        native: value.clone(),
    }
}

/// The steps [`parse_step`] draws by name, in the order it looks for them.
const STEPS: &[&str] = &[
    "union_result",
    "window_functions_computation",
    "windowing",
    "nested_loop",
    "table",
    "read_sorted_file",
    "filesort",
    "temporary_table",
    "ordering_operation",
    "grouping_operation",
    "duplicates_removal",
];

/// A query block's plan, with what sits beside its step under it: MariaDB's
/// `subqueries`, MySQL's `select_list_subqueries` and the like. A step this does not
/// know by name has made them its children already.
fn parse_block(value: &serde_json::Value) -> PlanNode {
    let mut node = parse_step(value);
    if let Some(step) = STEPS.iter().find(|step| value.get(**step).is_some()) {
        node.children.extend(plan_members(value, Some(step)));
    }
    node
}

/// Each member of `value` but `drawn` that holds a plan, as a node named after its key
/// -- a derived table's `materialized_from_subquery`, a subquery list, a step this does
/// not know -- with that plan under it.
fn plan_members(value: &serde_json::Value, drawn: Option<&str>) -> Vec<PlanNode> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, inner)| Some(key.as_str()) != drawn && holds_plan(inner))
        .map(|(key, inner)| PlanNode {
            kind: humanize(key),
            relation: None,
            detail: None,
            estimates: PlanMetrics::default(),
            actual: PlanMetrics::default(),
            loops: None,
            children: match inner {
                serde_json::Value::Array(items) => items.iter().map(parse_value).collect(),
                _ => vec![parse_value(inner)],
            },
            native: inner.clone(),
        })
        .collect()
}

fn parse_step(value: &serde_json::Value) -> PlanNode {
    if let Some(union) = value.get("union_result") {
        let parts = union
            .get("query_specifications")
            .and_then(serde_json::Value::as_array)
            .map(|parts| parts.iter().map(parse_value).collect())
            .unwrap_or_default();
        return PlanNode {
            kind: "Union".into(),
            relation: None,
            detail: union
                .get("table_name")
                .and_then(serde_json::Value::as_str)
                .map(|name| format!("into {name}")),
            estimates: cost_metrics(value),
            actual: analyzed_metrics(union),
            loops: None,
            children: parts,
            native: value.clone(),
        };
    }
    // MariaDB's `window_functions_computation`, MySQL's `windowing`: the window
    // functions are computed over the rows of what is inside.
    if let Some(inner) = value
        .get("window_functions_computation")
        .or_else(|| value.get("windowing"))
    {
        let order = inner
            .pointer("/sorts/0/filesort/sort_key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                inner
                    .pointer("/windows/0/filesort_key")
                    .and_then(serde_json::Value::as_array)
                    .map(|keys| {
                        keys.iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
            });
        return PlanNode {
            kind: "Window".into(),
            relation: None,
            detail: order.map(|order| format!("by {order}")),
            estimates: cost_metrics(value),
            actual: PlanMetrics::default(),
            loops: None,
            children: vec![parse_block(inner)],
            native: value.clone(),
        };
    }
    if let Some(nested) = value
        .get("nested_loop")
        .and_then(serde_json::Value::as_array)
    {
        let children: Vec<_> = nested.iter().map(parse_value).collect();
        return PlanNode {
            kind: "Nested loop".into(),
            relation: None,
            detail: None,
            estimates: cost_metrics(value),
            actual: PlanMetrics::default(),
            loops: None,
            children,
            native: value.clone(),
        };
    }
    if let Some(table) = value.get("table") {
        return parse_table(table);
    }
    // MariaDB's names for the same steps: sorted output arrives as `read_sorted_file`
    // around a `filesort`, and a GROUP BY goes through a `temporary_table`.
    if let Some(inner) = value.get("read_sorted_file") {
        return parse_block(inner);
    }
    if let Some(sort) = value.get("filesort") {
        return PlanNode {
            kind: "Sort".into(),
            relation: None,
            detail: sort
                .get("sort_key")
                .and_then(serde_json::Value::as_str)
                .map(|key| format!("by {key}")),
            estimates: cost_metrics(value),
            actual: analyzed_metrics(sort),
            loops: None,
            children: vec![parse_block(sort)],
            native: value.clone(),
        };
    }
    if let Some(inner) = value.get("temporary_table") {
        return PlanNode {
            kind: "Temporary table".into(),
            relation: None,
            detail: None,
            estimates: PlanMetrics::default(),
            actual: analyzed_metrics(inner),
            loops: None,
            children: vec![parse_block(inner)],
            native: value.clone(),
        };
    }
    if let Some(inner) = value
        .get("ordering_operation")
        .or_else(|| value.get("grouping_operation"))
        .or_else(|| value.get("duplicates_removal"))
    {
        let kind = if value.get("ordering_operation").is_some() {
            "Sort"
        } else if value.get("grouping_operation").is_some() {
            "Aggregate"
        } else {
            "Distinct"
        };
        return PlanNode {
            kind: kind.into(),
            relation: None,
            detail: None,
            estimates: cost_metrics(value),
            actual: PlanMetrics::default(),
            loops: None,
            children: vec![parse_value(inner)],
            native: value.clone(),
        };
    }
    // A step this does not know by name still shows what is under it: each member
    // that holds a plan becomes a child, named after its key.
    PlanNode {
        kind: "Query block".into(),
        relation: None,
        detail: None,
        estimates: cost_metrics(value),
        actual: PlanMetrics::default(),
        loops: None,
        children: plan_members(value, None),
        native: value.clone(),
    }
}

/// Whether a table, a join or a query block is somewhere inside `value`.
fn holds_plan(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(members) => members.iter().any(|(key, inner)| {
            matches!(key.as_str(), "table" | "nested_loop" | "query_block") || holds_plan(inner)
        }),
        serde_json::Value::Array(items) => items.iter().any(holds_plan),
        _ => false,
    }
}

/// `materialized_from_subquery` as `Materialized from subquery`.
fn humanize(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn parse_table(value: &serde_json::Value) -> PlanNode {
    let access = value
        .get("access_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ALL");
    let kind = match access {
        "ALL" => "Table scan",
        "index" => "Index scan",
        "range" => "Index range",
        "ref" | "eq_ref" | "const" | "system" => "Index lookup",
        other => other,
    };
    let mut detail = Vec::new();
    if let Some(key) = value.get("key").and_then(serde_json::Value::as_str) {
        detail.push(format!("using {key}"));
    }
    if let Some(condition) = value
        .get("attached_condition")
        .and_then(serde_json::Value::as_str)
    {
        detail.push(format!("filter {condition}"));
    }
    PlanNode {
        kind: kind.into(),
        relation: value
            .get("table_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        detail: (!detail.is_empty()).then(|| detail.join(" · ")),
        estimates: PlanMetrics {
            cost: value
                .pointer("/cost_info/prefix_cost")
                .and_then(json_f64)
                .or_else(|| value.pointer("/cost_info/query_cost").and_then(json_f64))
                // MariaDB puts a plain `cost` on the table.
                .or_else(|| value.get("cost").and_then(json_f64)),
            rows: value
                .get("rows_examined_per_scan")
                .and_then(json_f64)
                .or_else(|| value.get("rows").and_then(json_f64)),
            width: None,
            time_ms: None,
        },
        actual: analyzed_metrics(value),
        loops: value
            .get("r_loops")
            .and_then(json_f64)
            .map(|loops| loops as u64),
        // What a derived table or a CTE is made from, and the subqueries checked per
        // row, hang off the table that reads them.
        children: plan_members(value, None),
        native: value.clone(),
    }
}

/// What MariaDB's `ANALYZE FORMAT=JSON` measured on a table: `r_rows` per loop, and the
/// time split into reading the table and the rest. MySQL's JSON has none of these.
fn analyzed_metrics(value: &serde_json::Value) -> PlanMetrics {
    let time = value.get("r_total_time_ms").and_then(json_f64).or_else(|| {
        let table = value.get("r_table_time_ms").and_then(json_f64);
        let other = value.get("r_other_time_ms").and_then(json_f64);
        match (table, other) {
            (None, None) => None,
            (table, other) => Some(table.unwrap_or(0.0) + other.unwrap_or(0.0)),
        }
    });
    PlanMetrics {
        cost: None,
        // A filesort says how many rows it put out, not `r_rows`.
        rows: value
            .get("r_rows")
            .or_else(|| value.get("r_output_rows"))
            .and_then(json_f64),
        width: None,
        time_ms: time,
    }
}

fn cost_metrics(value: &serde_json::Value) -> PlanMetrics {
    PlanMetrics {
        cost: value
            .pointer("/cost_info/query_cost")
            .and_then(json_f64)
            .or_else(|| value.get("cost").and_then(json_f64)),
        rows: None,
        width: None,
        time_ms: None,
    }
}

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

pub fn parse_tree(raw: &str) -> Result<ExplainPlan, DriverError> {
    let lines: Vec<(usize, &str)> = raw
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let marker = trimmed.strip_prefix("->")?;
            Some((line.len() - trimmed.len(), marker.trim()))
        })
        .collect();
    if lines.is_empty() {
        return Err(DriverError::new(
            DriverErrorCategory::Internal,
            "empty explain tree",
        ));
    }
    let (root, _) = build_tree(&lines, 0, lines[0].0);
    Ok(ExplainPlan {
        planning_ms: None,
        // MySQL reports no total; the root's last-row time is the time the query took.
        execution_ms: root.actual.time_ms,
        root,
        raw: raw.to_string(),
    })
}

fn build_tree(lines: &[(usize, &str)], index: usize, indent: usize) -> (PlanNode, usize) {
    let mut node = parse_tree_node(lines[index].1);
    let mut next = index + 1;
    while next < lines.len() && lines[next].0 > indent {
        let (child, consumed) = build_tree(lines, next, lines[next].0);
        node.children.push(child);
        next = consumed;
    }
    (node, next)
}

fn parse_tree_node(text: &str) -> PlanNode {
    let (kind_rel, rest) = text.split_once("  (").unwrap_or((text, ""));
    let (kind, relation, detail) = split_tree_label(kind_rel);
    // `(cost=… rows=…) (actual time=… rows=… loops=…)`, either group optional. A node
    // with no estimate starts straight at `actual`, and reading `rows=` from the whole
    // text used to file its actual rows as the estimate.
    let (estimate_part, actual_part) = match rest.strip_prefix("actual ") {
        Some(actual) => ("", actual),
        None => rest.split_once("(actual ").unwrap_or((rest, "")),
    };
    let estimates = PlanMetrics {
        cost: extract_number(estimate_part, "cost="),
        rows: extract_number(estimate_part, "rows="),
        width: None,
        time_ms: None,
    };
    let actual = PlanMetrics {
        cost: None,
        rows: extract_number(actual_part, "rows="),
        width: None,
        time_ms: extract_actual_time(actual_part),
    };
    let loops = extract_number(actual_part, "loops=").map(|value| value as u64);
    PlanNode {
        kind,
        relation,
        detail,
        estimates,
        actual,
        loops,
        children: Vec::new(),
        native: serde_json::Value::String(text.to_string()),
    }
}

/// `Filter: (c.id <= 10)`, `Index lookup on o using customer_id (customer_id=c.id)`:
/// the node, the table it reads, and the rest -- key, index, condition -- as detail. The
/// colon form goes first, since a condition may itself contain " on ".
fn split_tree_label(label: &str) -> (String, Option<String>, Option<String>) {
    let label = label.trim();
    if let Some((kind, detail)) = label.split_once(": ") {
        return (kind.to_string(), None, Some(detail.trim().to_string()));
    }
    let Some((kind, target)) = label.split_once(" on ") else {
        return (label.to_string(), None, None);
    };
    let (relation, detail) = target
        .trim()
        .split_once(' ')
        .map(|(relation, detail)| (relation, Some(detail.trim().to_string())))
        .unwrap_or((target.trim(), None));
    (kind.trim().to_string(), Some(relation.to_string()), detail)
}

fn extract_number(text: &str, key: &str) -> Option<f64> {
    let rest = text.split(key).nth(1)?;
    let token: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    if token.is_empty() {
        None
    } else {
        token.parse().ok()
    }
}

fn extract_actual_time(text: &str) -> Option<f64> {
    let rest = text
        .strip_prefix("time=")
        .or_else(|| text.split("time=").nth(1))?;
    let end = rest.split("..").nth(1).unwrap_or(rest);
    end.split([' ', ')']).next()?.parse().ok()
}

#[async_trait::async_trait]
impl ExplainProvider for MysqlSession {
    /// MySQL and MariaDB refuse a `?` in an EXPLAIN as a syntax error; a statement that
    /// failed is prepared on its own to tell that apart, and refused for what it is.
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        match self.plan(&request).await {
            Err(error) if self.has_parameters(&request.sql).await => {
                Err(dexo_driver_api::parameters_unsupported().with_detail(error.to_string()))
            }
            planned => planned,
        }
    }
}

impl MysqlSession {
    async fn plan(&self, request: &ExplainRequest) -> Result<ExplainPlan, DriverError> {
        if !request.hypothetical_indexes.is_empty() {
            return Err(dexo_driver_api::hypothetical_unsupported());
        }
        let caps = if self.is_mariadb() {
            MysqlExplainCaps::mariadb()
        } else {
            MysqlExplainCaps::mysql()
        };
        let format = select_format(request.analyze, caps)?;
        let sql = wrap_explain(&request.sql, format, request.analyze);
        if request.analyze {
            let raw = self.fetch_explain_analyzed(&sql).await?;
            // MySQL answers this, instead of an error, for a statement ANALYZE cannot run
            // (a single-table UPDATE or DELETE, for one); it is not a plan to draw.
            if raw.contains("<not executable by iterator executor>") {
                return Err(DriverError::unsupported(
                    "MySQL can only EXPLAIN ANALYZE SELECT, TABLE and multi-table UPDATE or DELETE statements; use the estimated plan for this one",
                ));
            }
            return match format {
                NativeExplainFormat::Json => parse_json(&raw),
                NativeExplainFormat::Tree => parse_tree(&raw),
            };
        }
        let raw = self.fetch_explain_text(&sql).await;
        let raw = match raw {
            Ok(raw) => raw,
            Err(error) if format == NativeExplainFormat::Json && !request.analyze && caps.tree => {
                let fallback = wrap_explain(&request.sql, NativeExplainFormat::Tree, false);
                self.fetch_explain_text(&fallback)
                    .await
                    .map_err(|_| error)?
            }
            Err(error) => return Err(error),
        };
        match format {
            NativeExplainFormat::Json if raw.trim_start().starts_with('{') => parse_json(&raw),
            _ => parse_tree(&raw),
        }
    }
}

impl MysqlSession {
    /// Whether `sql` prepares with placeholders. Preparing runs nothing.
    async fn has_parameters(&self, sql: &str) -> bool {
        let mut conn = self.conn.lock().await;
        let Ok(statement) = conn.prep(sql.trim().trim_end_matches(';')).await else {
            return false;
        };
        let parameters = statement.num_params();
        let _ = conn.close(statement).await;
        parameters > 0
    }

    async fn fetch_explain_text(&self, sql: &str) -> Result<String, DriverError> {
        let mut conn = self.conn.lock().await;
        explain_text(&mut conn, sql).await
    }

    /// EXPLAIN ANALYZE executes the statement to time it, and what it changed used to stay
    /// committed. It runs inside a transaction, or a savepoint when the user has one open
    /// (`START TRANSACTION` there would commit their work), and is always rolled back. The
    /// connection stays locked throughout, so nothing else runs inside the fence.
    async fn fetch_explain_analyzed(&self, sql: &str) -> Result<String, DriverError> {
        let (open, close): (&str, &[&str]) = if self.state() == TransactionState::Idle {
            ("START TRANSACTION", &["ROLLBACK"])
        } else {
            (
                "SAVEPOINT dexo_explain",
                &[
                    "ROLLBACK TO SAVEPOINT dexo_explain",
                    "RELEASE SAVEPOINT dexo_explain",
                ],
            )
        };
        let mut conn = self.conn.lock().await;
        conn.query_drop(open).await.map_err(map_error)?;
        let result = explain_text(&mut conn, sql).await;
        for statement in close {
            if let Err(error) = conn.query_drop(*statement).await {
                // A plan from a fence that did not close is not worth the doubt about
                // what it left behind.
                return Err(result.err().unwrap_or_else(|| map_error(error)));
            }
        }
        result
    }
}

async fn explain_text(conn: &mut mysql_async::Conn, sql: &str) -> Result<String, DriverError> {
    let rows: Vec<(String,)> = conn.query(sql).await.map_err(map_error)?;
    Ok(rows
        .into_iter()
        .map(|row| row.0)
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{MysqlExplainCaps, NativeExplainFormat, parse_json, select_format, wrap_explain};

    /// MariaDB wraps sorted output in `read_sorted_file` and `filesort`, and a GROUP BY
    /// in a `temporary_table`; every table under them shows, with its cost.
    #[test]
    fn mariadb_sorts_and_temporary_tables_keep_their_tables() {
        let joined = parse_json(include_str!(
            "../tests/fixtures/mariadb-analyze-join-sorted.json"
        ))
        .unwrap();
        let root = &joined.root;
        assert_eq!(root.kind, "Nested loop");
        assert_eq!(root.children[0].kind, "Sort");
        assert_eq!(root.children[0].children[0].relation.as_deref(), Some("c"));
        assert_eq!(root.children[0].actual.rows, Some(200.0));
        assert_eq!(root.children[1].relation.as_deref(), Some("o"));
        assert!(root.estimates.cost.is_some());
        assert!(root.children[1].estimates.cost.is_some());

        let grouped = parse_json(include_str!(
            "../tests/fixtures/mariadb-explain-group-sorted.json"
        ))
        .unwrap();
        let sort = &grouped.root;
        assert_eq!(sort.kind, "Sort");
        assert_eq!(sort.children[0].kind, "Temporary table");
        assert_eq!(
            sort.children[0].children[0].children[0].relation.as_deref(),
            Some("o")
        );
    }

    /// A UNION, window functions and subqueries showed as an empty "Query block".
    #[test]
    fn unions_windows_and_subqueries_keep_their_tables() {
        let relations = |node: &dexo_driver_api::PlanNode| {
            fn walk(node: &dexo_driver_api::PlanNode, out: &mut Vec<String>) {
                out.extend(node.relation.clone());
                for child in &node.children {
                    walk(child, out);
                }
            }
            let mut out = Vec::new();
            walk(node, &mut out);
            out
        };
        for (fixture, kind, tables) in [
            (
                include_str!("../tests/fixtures/mariadb-explain-union.json"),
                "Union",
                vec!["o", "c"],
            ),
            (
                include_str!("../tests/fixtures/mysql-explain-union.json"),
                "Union",
                vec!["o", "c"],
            ),
            (
                include_str!("../tests/fixtures/mariadb-explain-window.json"),
                "Window",
                vec!["o"],
            ),
            (
                include_str!("../tests/fixtures/mysql-explain-window.json"),
                "Window",
                vec!["o"],
            ),
        ] {
            let plan = parse_json(fixture).unwrap();
            assert_eq!(plan.root.kind, kind, "{fixture}");
            assert_eq!(relations(&plan.root), tables, "{fixture}");
            if kind == "Window" {
                assert!(
                    plan.root
                        .detail
                        .as_deref()
                        .is_some_and(|detail| detail.starts_with("by "))
                );
            }
        }
        let plan = parse_json(include_str!(
            "../tests/fixtures/mariadb-explain-subqueries.json"
        ))
        .unwrap();
        assert_eq!(relations(&plan.root), ["o", "o2", "c"]);
        assert_eq!(plan.root.children.last().unwrap().kind, "Subqueries");
        // A subquery in the select list, a derived table and a CTE: each read through
        // a table of the outer query, whose plan holds theirs.
        for (fixture, tables) in [
            (
                include_str!("../tests/fixtures/mysql-explain-select-subquery.json"),
                vec!["c", "o"],
            ),
            (
                include_str!("../tests/fixtures/mysql-explain-derived.json"),
                vec!["c", "t", "o"],
            ),
            (
                include_str!("../tests/fixtures/mysql-explain-cte.json"),
                vec!["c", "t", "o"],
            ),
            (
                include_str!("../tests/fixtures/mariadb-explain-derived.json"),
                vec!["c", "<derived2>", "o"],
            ),
            (
                include_str!("../tests/fixtures/mariadb-explain-cte.json"),
                vec!["c", "<derived2>", "o"],
            ),
        ] {
            let plan = parse_json(fixture).unwrap();
            assert_eq!(relations(&plan.root), tables, "{fixture}");
        }
        // MySQL's second JSON format (`explain_json_format_version = 2`) is the
        // iterator tree: it came out as an empty plan.
        let plan = parse_json(include_str!(
            "../tests/fixtures/mysql-explain-v2-select-subquery.json"
        ))
        .unwrap();
        assert_eq!(plan.root.kind, "Covering index scan");
        assert_eq!(plan.root.detail.as_deref(), Some("using PRIMARY"));
        assert_eq!(plan.root.estimates.rows, Some(3.0));
        assert_eq!(plan.root.estimates.cost, Some(0.55));
        assert_eq!(plan.root.children[0].kind, "Aggregate");
        assert_eq!(relations(&plan.root), ["c", "o"]);
        let plan = parse_json(include_str!(
            "../tests/fixtures/mysql-explain-v2-derived.json"
        ))
        .unwrap();
        assert_eq!(plan.root.kind, "Nested loop inner join");
        assert_eq!(relations(&plan.root), ["c", "t", "o"]);
        // An unknown wrapper is descended into, not left empty.
        let wrapped =
            parse_json(r#"{"query_block": {"some_new_step": {"table": {"table_name": "t"}}}}"#)
                .unwrap();
        assert_eq!(relations(&wrapped.root), ["t"]);
        assert_eq!(wrapped.root.children[0].kind, "Some new step");
    }

    #[test]
    fn prefers_json_and_uses_tree_for_analyze() {
        let caps = MysqlExplainCaps::mysql();
        assert_eq!(
            select_format(false, caps).unwrap(),
            NativeExplainFormat::Json
        );
        assert_eq!(
            select_format(true, caps).unwrap(),
            NativeExplainFormat::Tree
        );
        let no_json = MysqlExplainCaps {
            json: false,
            tree: true,
            tree_analyze: false,
            json_analyze: false,
        };
        assert_eq!(
            select_format(false, no_json).unwrap(),
            NativeExplainFormat::Tree
        );
        assert!(select_format(true, no_json).is_err());
    }

    #[test]
    fn wrap_never_adds_analyze_unless_requested() {
        assert!(!wrap_explain("select 1", NativeExplainFormat::Json, false).contains("ANALYZE"));
        assert!(wrap_explain("select 1", NativeExplainFormat::Tree, true).contains("ANALYZE"));
    }

    #[test]
    fn analyze_tree_keeps_actual_rows_apart_from_estimates() {
        let plan = super::parse_tree(
            "-> Limit: 5 row(s)  (actual time=1.78..1.78 rows=5 loops=1)\n    -> Nested loop inner join  (cost=476 rows=3000) (actual time=0.0307..1.03 rows=3000 loops=1)",
        )
        .unwrap();
        assert_eq!(plan.root.estimates.rows, None);
        assert_eq!(plan.root.actual.rows, Some(5.0));
        assert_eq!(plan.root.loops, Some(1));
        assert_eq!(plan.execution_ms, Some(1.78));
        let join = &plan.root.children[0];
        assert_eq!(join.estimates.rows, Some(3000.0));
        assert_eq!(join.estimates.cost, Some(476.0));
        assert_eq!(join.actual.rows, Some(3000.0));
        assert_eq!(join.actual.time_ms, Some(1.03));
    }

    #[test]
    fn parse_scan_golden() {
        let plan = parse_json(include_str!("../tests/fixtures/explain/scan.json")).unwrap();
        assert_eq!(plan.root.kind, "Table scan");
        assert_eq!(plan.root.relation.as_deref(), Some("items"));
        assert_eq!(plan.root.estimates.rows, Some(1000.0));
        assert!(plan.root.actual.rows.is_none());
        assert!(plan.root.actual.time_ms.is_none());
    }
}
