use dexo_driver_api::{
    DriverError, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics, PlanNode,
};
use serde_json::json;

use crate::error::map_error;
use crate::session::SqliteSession;

#[async_trait::async_trait]
impl ExplainProvider for SqliteSession {
    /// `EXPLAIN QUERY PLAN`, which compiles the statement and never runs it, so it is as
    /// safe on a write as on a read. SQLite has no plan with actual figures.
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        if !request.hypothetical_indexes.is_empty() {
            return Err(dexo_driver_api::hypothetical_unsupported());
        }
        if request.analyze {
            return Err(DriverError::unsupported(
                "SQLite has no EXPLAIN ANALYZE; use the estimated plan",
            ));
        }
        let sql = format!(
            "EXPLAIN QUERY PLAN {}",
            request.sql.trim().trim_end_matches(';')
        );
        let rows = self
            .with_conn(move |conn| {
                let mut statement = conn.prepare(&sql).map_err(map_error)?;
                statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(3)?)))
                    .map_err(map_error)?
                    .collect::<rusqlite::Result<Vec<(i64, i64, String)>>>()
                    .map_err(map_error)
            })
            .await?;
        let raw = rows
            .iter()
            .map(|(id, parent, detail)| format!("{id}|{parent}|{detail}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ExplainPlan {
            planning_ms: None,
            execution_ms: None,
            root: plan_tree(&rows),
            raw,
        })
    }
}

/// The `(id, parent, detail)` rows of `EXPLAIN QUERY PLAN` as a tree under one root, the
/// way the sqlite3 shell draws them.
fn plan_tree(rows: &[(i64, i64, String)]) -> PlanNode {
    let mut root = node("QUERY PLAN", json!({}));
    root.children = children(rows, 0);
    root
}

fn children(rows: &[(i64, i64, String)], parent: i64) -> Vec<PlanNode> {
    rows.iter()
        .filter(|(id, of, _)| *of == parent && *id != parent)
        .map(|(id, of, detail)| {
            let mut node = parse_detail(detail);
            node.native = json!({ "id": id, "parent": of, "detail": detail });
            node.children = children(rows, *id);
            node
        })
        .collect()
}

fn node(kind: &str, native: serde_json::Value) -> PlanNode {
    PlanNode {
        kind: kind.into(),
        relation: None,
        detail: None,
        estimates: PlanMetrics::default(),
        actual: PlanMetrics::default(),
        loops: None,
        children: Vec::new(),
        native,
    }
}

/// `SEARCH orders USING INDEX orders_customer (customer_id=?)` is a SEARCH of `orders`
/// by the rest; `USE TEMP B-TREE FOR ORDER BY` is the B-tree, for what follows. Any other
/// line is a kind of its own.
fn parse_detail(detail: &str) -> PlanNode {
    let mut parsed = node(detail, serde_json::Value::Null);
    let rest = |text: &str| Some(text.trim().to_string()).filter(|text| !text.is_empty());
    for kind in ["SCAN", "SEARCH"] {
        let Some(tail) = detail
            .strip_prefix(kind)
            .and_then(|tail| tail.strip_prefix(' '))
        else {
            continue;
        };
        if tail.starts_with("CONSTANT ROW") || tail.starts_with('(') {
            return parsed;
        }
        let (relation, tail) = tail.split_once(' ').unwrap_or((tail, ""));
        parsed.kind = kind.into();
        parsed.relation = Some(relation.to_string());
        parsed.detail = rest(tail);
        return parsed;
    }
    for kind in ["USE TEMP B-TREE", "CO-ROUTINE", "MATERIALIZE"] {
        if let Some(tail) = detail.strip_prefix(kind) {
            parsed.kind = kind.into();
            parsed.detail = rest(tail);
            return parsed;
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::plan_tree;

    #[test]
    fn plan_rows_nest_under_their_parent() {
        let rows = [
            (
                2,
                0,
                "SEARCH o USING INDEX orders_customer (customer_id=?)".to_string(),
            ),
            (5, 0, "USE TEMP B-TREE FOR ORDER BY".to_string()),
            (7, 2, "SCAN CONSTANT ROW".to_string()),
        ];
        let root = plan_tree(&rows);
        assert_eq!(root.kind, "QUERY PLAN");
        let search = &root.children[0];
        assert_eq!(search.kind, "SEARCH");
        assert_eq!(search.relation.as_deref(), Some("o"));
        assert_eq!(
            search.detail.as_deref(),
            Some("USING INDEX orders_customer (customer_id=?)")
        );
        assert_eq!(search.children[0].kind, "SCAN CONSTANT ROW");
        assert_eq!(root.children[1].kind, "USE TEMP B-TREE");
        assert_eq!(root.children[1].detail.as_deref(), Some("FOR ORDER BY"));
    }
}
