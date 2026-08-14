use dexo_driver_api::{
    DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics,
    PlanNode,
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
}

pub fn select_format(
    analyze: bool,
    caps: MysqlExplainCaps,
) -> Result<NativeExplainFormat, DriverError> {
    if analyze {
        if caps.tree_analyze {
            return Ok(NativeExplainFormat::Tree);
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
        (NativeExplainFormat::Json, _) => format!("EXPLAIN FORMAT=JSON {inner}"),
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
    let root = parse_value(&value);
    Ok(ExplainPlan {
        planning_ms: None,
        execution_ms: None,
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

fn parse_block(value: &serde_json::Value) -> PlanNode {
    if let Some(nested) = value
        .get("nested_loop")
        .and_then(serde_json::Value::as_array)
    {
        let children: Vec<_> = nested.iter().map(parse_value).collect();
        return PlanNode {
            kind: "Nested loop".into(),
            relation: None,
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
            estimates: cost_metrics(value),
            actual: PlanMetrics::default(),
            loops: None,
            children: vec![parse_value(inner)],
            native: value.clone(),
        };
    }
    PlanNode {
        kind: "Query block".into(),
        relation: None,
        estimates: cost_metrics(value),
        actual: PlanMetrics::default(),
        loops: None,
        children: Vec::new(),
        native: value.clone(),
    }
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
    PlanNode {
        kind: kind.into(),
        relation: value
            .get("table_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        estimates: PlanMetrics {
            cost: value
                .pointer("/cost_info/prefix_cost")
                .and_then(json_f64)
                .or_else(|| value.pointer("/cost_info/query_cost").and_then(json_f64)),
            rows: value
                .get("rows_examined_per_scan")
                .and_then(json_f64)
                .or_else(|| value.get("rows").and_then(json_f64)),
            width: None,
            time_ms: None,
        },
        actual: PlanMetrics::default(),
        loops: None,
        children: Vec::new(),
        native: value.clone(),
    }
}

fn cost_metrics(value: &serde_json::Value) -> PlanMetrics {
    PlanMetrics {
        cost: value.pointer("/cost_info/query_cost").and_then(json_f64),
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
        execution_ms: None,
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
    let (kind, relation) = kind_rel
        .split_once(" on ")
        .map(|(kind, rel)| (kind.trim().to_string(), Some(rel.trim().to_string())))
        .unwrap_or_else(|| (kind_rel.trim().to_string(), None));
    let estimates = PlanMetrics {
        cost: extract_number(rest, "cost="),
        rows: extract_number(rest, "rows="),
        width: None,
        time_ms: None,
    };
    let actual_part = rest.split("(actual ").nth(1).unwrap_or("");
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
        estimates,
        actual,
        loops,
        children: Vec::new(),
        native: serde_json::Value::String(text.to_string()),
    }
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
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        let caps = MysqlExplainCaps {
            json: true,
            tree: true,
            tree_analyze: true,
        };
        let format = select_format(request.analyze, caps)?;
        let sql = wrap_explain(&request.sql, format, request.analyze);
        let raw = self.fetch_explain_text(&sql).await;
        let raw = match raw {
            Ok(raw) => raw,
            Err(error) if format == NativeExplainFormat::Json && !request.analyze => {
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
    async fn fetch_explain_text(&self, sql: &str) -> Result<String, DriverError> {
        let mut conn = self.conn.lock().await;
        let rows: Vec<(String,)> = conn.query(sql).await.map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.0)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::{MysqlExplainCaps, NativeExplainFormat, parse_json, select_format, wrap_explain};

    #[test]
    fn prefers_json_and_uses_tree_for_analyze() {
        let caps = MysqlExplainCaps {
            json: true,
            tree: true,
            tree_analyze: true,
        };
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
    fn parse_scan_golden() {
        let plan = parse_json(include_str!("../tests/fixtures/explain/scan.json")).unwrap();
        assert_eq!(plan.root.kind, "Table scan");
        assert_eq!(plan.root.relation.as_deref(), Some("items"));
        assert_eq!(plan.root.estimates.rows, Some(1000.0));
        assert!(plan.root.actual.rows.is_none());
        assert!(plan.root.actual.time_ms.is_none());
    }
}
