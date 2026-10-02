use std::collections::HashMap;

use dexo_driver_api::{
    DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics,
    PlanNode,
};
use duckdb::Connection;
use duckdb::profiling::ProfilingInfo;
use serde_json::{Value, json};

use crate::error::{map_error, writes_refused};
use crate::session::{
    DuckdbSession, begin_own, close_own, first_word, reads, split_differently, statements,
};

#[async_trait::async_trait]
impl ExplainProvider for DuckdbSession {
    /// `EXPLAIN (FORMAT JSON)`, which plans the statement and never runs it. ANALYZE runs
    /// it with DuckDB's profiler on -- its EXPLAIN ANALYZE has no JSON -- inside a
    /// transaction rolled back after it.
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        if !request.hypothetical_indexes.is_empty() {
            return Err(dexo_driver_api::hypothetical_unsupported());
        }
        let sql = match statements(&request.sql)[..] {
            [one] => one.trim_end_matches(';').trim().to_string(),
            _ => {
                return Err(DriverError::new(
                    DriverErrorCategory::Syntax,
                    "explain one statement at a time",
                ));
            }
        };
        if crate::parse::statement_count(&sql).is_some_and(|count| count > 1) {
            return Err(split_differently(&sql));
        }
        let read_only = self.read_only();
        if request.analyze {
            return self
                .with_conn(move |conn| analyzed(conn, &sql, read_only))
                .await;
        }
        let text = self
            .with_conn(move |conn| {
                // DuckDB folds the values it is given into the plan, so a parameter has no
                // value to stand in for it.
                let mut statement = conn
                    .prepare(&format!("EXPLAIN (FORMAT JSON) {sql}"))
                    .map_err(map_error)?;
                if statement.parameter_count() > 0 {
                    return Err(dexo_driver_api::parameters_unsupported());
                }
                statement
                    .query_row([], |row| row.get::<_, String>(1))
                    .map_err(map_error)
            })
            .await?;
        let parsed: Value = serde_json::from_str(&text).map_err(|error| {
            DriverError::new(
                DriverErrorCategory::Internal,
                format!("DuckDB's plan is not the JSON expected: {error}"),
            )
        })?;
        let roots = parsed
            .as_array()
            .map(|nodes| nodes.iter().map(estimated_node).collect())
            .unwrap_or_default();
        Ok(ExplainPlan {
            planning_ms: None,
            execution_ms: None,
            root: one_root(roots),
            raw: text,
        })
    }
}

/// Runs `sql` under the profiler. Outside a transaction it runs in one rolled back after
/// it, so a write explained this way changes nothing; inside the user's, DuckDB has no
/// savepoint to undo it with, so only a read is run there. Only a query or an INSERT,
/// UPDATE or DELETE: a COPY, an EXPORT or a SET does what it does outside any
/// transaction, and a rollback undoes none of it.
fn analyzed(conn: &Connection, sql: &str, read_only: bool) -> Result<ExplainPlan, DriverError> {
    let reads = reads(conn, sql)?;
    let changes_rows = matches!(
        first_word(sql).as_deref(),
        Some("INSERT" | "UPDATE" | "DELETE")
    );
    if !reads && !changes_rows {
        return Err(DriverError::unsupported(
            "EXPLAIN ANALYZE runs queries and INSERT, UPDATE or DELETE; use the estimated plan for this one",
        ));
    }
    if read_only && !reads {
        return Err(writes_refused());
    }
    let fenced = begin_own(conn, "BEGIN TRANSACTION")?;
    if !fenced && !reads {
        return Err(DriverError::new(
            DriverErrorCategory::Capability,
            "EXPLAIN ANALYZE runs the statement, and inside a transaction DuckDB cannot undo it: \
             commit or roll back first, or use the estimated plan",
        ));
    }
    let run = || -> Result<Option<ProfilingInfo>, DriverError> {
        conn.execute_batch(
            "PRAGMA enable_profiling = 'no_output'; PRAGMA profiling_mode = 'standard'",
        )
        .map_err(map_error)?;
        let mut statement = conn.prepare(sql).map_err(map_error)?;
        if statement.parameter_count() > 0 {
            return Err(dexo_driver_api::parameters_unsupported());
        }
        let _ = statement.stream_arrow([]).map_err(map_error)?;
        while statement.step().map_err(map_error)?.is_some() {}
        drop(statement);
        Ok(conn.get_profiling_info())
    };
    let profiled = run();
    let _ = conn.execute_batch("PRAGMA disable_profiling");
    if fenced {
        close_own(conn)?;
    }
    let info = profiled?.ok_or_else(|| {
        DriverError::new(
            DriverErrorCategory::Internal,
            "DuckDB kept no profile of the run",
        )
    })?;
    let execution_ms = info
        .metrics
        .get("LATENCY")
        .and_then(|seconds| seconds.parse::<f64>().ok())
        .map(|seconds| seconds * 1000.0);
    Ok(ExplainPlan {
        planning_ms: None,
        execution_ms,
        root: one_root(info.children.iter().map(analyzed_node).collect()),
        raw: serde_json::to_string_pretty(&profile_json(&info)).unwrap_or_default(),
    })
}

/// DuckDB's plan has a root per pipeline sink, usually one.
fn one_root(mut roots: Vec<PlanNode>) -> PlanNode {
    if roots.len() == 1 {
        return roots.remove(0);
    }
    PlanNode {
        kind: "QUERY PLAN".into(),
        relation: None,
        detail: None,
        estimates: PlanMetrics::default(),
        actual: PlanMetrics::default(),
        loops: None,
        children: roots,
        native: json!({}),
    }
}

/// `{"name", "children", "extra_info": {"Table": ..., "Estimated Cardinality": ...}}`.
fn estimated_node(node: &Value) -> PlanNode {
    let info: Vec<(String, String)> = node
        .get("extra_info")
        .and_then(Value::as_object)
        .map(|info| {
            info.iter()
                .map(|(key, value)| {
                    let text = match value {
                        Value::String(text) => text.clone(),
                        Value::Array(items) => items
                            .iter()
                            .map(|item| {
                                item.as_str()
                                    .map_or_else(|| item.to_string(), str::to_string)
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        other => other.to_string(),
                    };
                    (key.clone(), text)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut native = node.clone();
    if let Some(object) = native.as_object_mut() {
        object.remove("children");
    }
    let mut built = plan_node(
        node.get("name").and_then(Value::as_str).unwrap_or("?"),
        &info,
        native,
    );
    built.children = node
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().map(estimated_node).collect())
        .unwrap_or_default();
    built
}

/// A profiled operator: `OPERATOR_NAME`, `OPERATOR_CARDINALITY` (its actual rows),
/// `OPERATOR_TIMING` in seconds, and `EXTRA_INFO` as DuckDB prints a map.
fn analyzed_node(info: &ProfilingInfo) -> PlanNode {
    let metric = |key: &str| info.metrics.get(key).map(String::as_str);
    let extra = metric("EXTRA_INFO").map(parse_map).unwrap_or_default();
    let mut built = plan_node(
        metric("OPERATOR_NAME").unwrap_or("?"),
        &extra,
        json!(sorted(&info.metrics)),
    );
    built.actual = PlanMetrics {
        rows: metric("OPERATOR_CARDINALITY").and_then(|rows| rows.parse().ok()),
        time_ms: metric("OPERATOR_TIMING")
            .and_then(|seconds| seconds.parse::<f64>().ok())
            .map(|seconds| seconds * 1000.0),
        ..PlanMetrics::default()
    };
    built.children = info.children.iter().map(analyzed_node).collect();
    built
}

/// The keys that name what a node works on are its relation and estimate; the rest --
/// conditions, filters, groups, sort keys -- are its detail, in DuckDB's order.
fn plan_node(name: &str, info: &[(String, String)], native: Value) -> PlanNode {
    let get = |key: &str| {
        info.iter()
            .find(|(own, _)| own == key)
            .map(|(_, value)| value.clone())
    };
    let estimate = get("Estimated Cardinality")
        .or_else(|| get("__estimated_cardinality__"))
        .and_then(|rows| rows.parse().ok());
    // JSON objects may arrive sorted by key; DuckDB's own order is kept by name.
    const ORDER: [&str; 7] = [
        "Join Type",
        "Conditions",
        "Filters",
        "Groups",
        "Aggregates",
        "Order By",
        "Top",
    ];
    let mut info: Vec<&(String, String)> = info.iter().collect();
    info.sort_by_key(|(key, _)| {
        ORDER
            .iter()
            .position(|known| known == key)
            .unwrap_or(ORDER.len())
    });
    let detail = info
        .iter()
        .filter(|(key, value)| {
            // A table function's node is named after the function already.
            let names_itself = key == "Function" && value.eq_ignore_ascii_case(name.trim());
            !names_itself
                && !matches!(
                    key.as_str(),
                    "Table"
                        | "Type"
                        | "Projections"
                        | "Estimated Cardinality"
                        | "__estimated_cardinality__"
                        | "__projections__"
                )
        })
        .map(|(key, value)| format!("{key}: {}", value.replace('\n', ", ")))
        .collect::<Vec<_>>()
        .join("; ");
    PlanNode {
        kind: name.trim().to_string(),
        relation: get("Table"),
        detail: Some(detail).filter(|detail| !detail.is_empty()),
        estimates: PlanMetrics {
            rows: estimate,
            ..PlanMetrics::default()
        },
        actual: PlanMetrics::default(),
        loops: None,
        children: Vec::new(),
        native,
    }
}

/// DuckDB prints a profiled operator's extra info as `{Key=value, 'Key (s)'='a, b'}`, a
/// quote inside a quoted key or value escaped with a backslash.
fn parse_map(text: &str) -> Vec<(String, String)> {
    let inner = text
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or("");
    let mut pairs = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.next_if(|ch| *ch == ',' || *ch == ' ').is_some() {}
        if chars.peek().is_none() {
            break;
        }
        let key = token(&mut chars, '=');
        if chars.next().is_none() {
            break;
        }
        let value = token(&mut chars, ',');
        pairs.push((key.trim().to_string(), value));
    }
    pairs
}

/// One key or value: quoted, or bare up to `end`.
fn token(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, end: char) -> String {
    let mut text = String::new();
    if chars.next_if_eq(&'\'').is_some() {
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => text.extend(chars.next()),
                '\'' => break,
                other => text.push(other),
            }
        }
    } else {
        while let Some(ch) = chars.next_if(|ch| *ch != end) {
            text.push(ch);
        }
    }
    text
}

fn sorted(metrics: &HashMap<String, String>) -> serde_json::Map<String, Value> {
    let mut keys: Vec<_> = metrics.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|key| (key.clone(), json!(metrics[key])))
        .collect()
}

fn profile_json(info: &ProfilingInfo) -> Value {
    json!({
        "metrics": sorted(&info.metrics),
        "children": info.children.iter().map(profile_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{estimated_node, parse_map};

    #[test]
    fn profiled_extra_info_reads_as_pairs() {
        let pairs = parse_map(
            "{Table=memory.main.t, Type=Sequential Scan, Projections=a\nb, Filters='b>=\\'x1\\' AND b<\\'x2\\'', __estimated_cardinality__=200}",
        );
        assert_eq!(pairs[0], ("Table".into(), "memory.main.t".into()));
        assert_eq!(pairs[2], ("Projections".into(), "a\nb".into()));
        assert_eq!(pairs[3], ("Filters".into(), "b>='x1' AND b<'x2'".into()));
        assert_eq!(pairs[4], ("__estimated_cardinality__".into(), "200".into()));
        assert!(parse_map("{}").is_empty());
        let file = parse_map("{Function=READ_CSV_AUTO, 'Filename(s)'=/d/sales.csv}");
        assert_eq!(file[1], ("Filename(s)".into(), "/d/sales.csv".into()));
    }

    #[test]
    fn estimated_nodes_carry_relation_detail_and_rows() {
        let node = estimated_node(&json!({
            "name": "HASH_JOIN",
            "children": [{
                "name": "SEQ_SCAN",
                "children": [],
                "extra_info": {
                    "Table": "memory.main.t",
                    "Projections": ["a", "b"],
                    "Filters": "b>='x1'",
                    "Estimated Cardinality": "200"
                }
            }],
            "extra_info": {"Join Type": "INNER", "Conditions": "t_a = a", "Estimated Cardinality": "877"}
        }));
        assert_eq!(node.kind, "HASH_JOIN");
        assert_eq!(
            node.detail.as_deref(),
            Some("Join Type: INNER; Conditions: t_a = a")
        );
        assert_eq!(node.estimates.rows, Some(877.0));
        let scan = &node.children[0];
        assert_eq!(scan.relation.as_deref(), Some("memory.main.t"));
        assert_eq!(scan.detail.as_deref(), Some("Filters: b>='x1'"));
        assert_eq!(scan.estimates.rows, Some(200.0));
    }
}
