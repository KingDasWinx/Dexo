use dexo_driver_api::{
    DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics,
    PlanNode, TransactionControl, TransactionState,
};
use tokio_postgres::SimpleQueryMessage;

use crate::error::map_error;
use crate::session::PostgresSession;

pub fn wrap_explain(sql: &str, analyze: bool) -> String {
    let inner = sql.trim().trim_end_matches(';');
    if analyze {
        format!("EXPLAIN (ANALYZE, FORMAT JSON) {inner}")
    } else {
        format!("EXPLAIN (FORMAT JSON) {inner}")
    }
}

pub fn parse_json(raw: &str) -> Result<ExplainPlan, DriverError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        DriverError::new(
            DriverErrorCategory::Internal,
            format!("explain json: {error}"),
        )
    })?;
    parse_value(&value, raw)
}

pub fn parse_value(value: &serde_json::Value, raw: &str) -> Result<ExplainPlan, DriverError> {
    let root_obj = value
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(value);
    let plan = root_obj.get("Plan").ok_or_else(|| {
        DriverError::new(DriverErrorCategory::Internal, "explain json missing Plan")
    })?;
    Ok(ExplainPlan {
        planning_ms: number(root_obj, "Planning Time"),
        execution_ms: number(root_obj, "Execution Time"),
        root: parse_node(plan),
        raw: raw.to_string(),
    })
}

fn parse_node(value: &serde_json::Value) -> PlanNode {
    let kind = value
        .get("Node Type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Unknown")
        .to_string();
    let relation = value
        .get("Relation Name")
        .or_else(|| value.get("Index Name"))
        .or_else(|| value.get("CTE Name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let children = value
        .get("Plans")
        .and_then(serde_json::Value::as_array)
        .map(|plans| plans.iter().map(parse_node).collect())
        .unwrap_or_default();
    PlanNode {
        kind,
        relation,
        estimates: PlanMetrics {
            cost: number(value, "Total Cost"),
            rows: number(value, "Plan Rows"),
            width: number(value, "Plan Width"),
            time_ms: None,
        },
        actual: PlanMetrics {
            cost: None,
            rows: number(value, "Actual Rows"),
            width: None,
            time_ms: number(value, "Actual Total Time"),
        },
        loops: number(value, "Actual Loops").map(|value| value as u64),
        children,
        native: value.clone(),
    }
}

fn number(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(serde_json::Value::as_f64)
}

/// The statements around an EXPLAIN ANALYZE that undo what it ran. ANALYZE executes the
/// statement to time it, and an UPDATE or DELETE explained that way used to commit. Inside
/// the user's own transaction a savepoint does it, so their work is left as it was;
/// `BEGIN` there would only warn, and the `ROLLBACK` would take their transaction with it.
fn analyze_fence(state: TransactionState) -> (&'static str, &'static str) {
    if state == TransactionState::Idle {
        ("BEGIN", "ROLLBACK")
    } else {
        (
            "SAVEPOINT dexo_explain",
            "ROLLBACK TO SAVEPOINT dexo_explain; RELEASE SAVEPOINT dexo_explain",
        )
    }
}

impl PostgresSession {
    async fn explain_analyzed(&self, sql: &str) -> Result<ExplainPlan, DriverError> {
        let (open, close) = analyze_fence(self.state());
        // One simple query, so nothing else sent on this shared connection lands inside
        // the fence.
        let fenced = format!("{open}; {}; {close}", wrap_explain(sql, true));
        match self.client.simple_query(&fenced).await {
            Ok(messages) => {
                let text = messages
                    .iter()
                    .find_map(|message| match message {
                        SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        DriverError::new(DriverErrorCategory::Internal, "explain returned no plan")
                    })?;
                parse_json(&text)
            }
            Err(error) => {
                // The server skips the rest of the string after an error, so the fence is
                // still open; closing it keeps the session out of an aborted transaction.
                let _ = self.client.batch_execute(close).await;
                Err(map_error(error))
            }
        }
    }
}

#[async_trait::async_trait]
impl ExplainProvider for PostgresSession {
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError> {
        if request.analyze {
            return self.explain_analyzed(&request.sql).await;
        }
        let sql = wrap_explain(&request.sql, false);
        let row = self.client.query_one(&sql, &[]).await.map_err(map_error)?;
        if let Ok(value) = row.try_get::<_, serde_json::Value>(0) {
            return parse_value(&value, &value.to_string());
        }
        let text: String = row.try_get(0).map_err(map_error)?;
        parse_json(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_json, wrap_explain};

    #[test]
    fn wrap_keeps_analyze_opt_in() {
        assert_eq!(
            wrap_explain("select 1", false),
            "EXPLAIN (FORMAT JSON) select 1"
        );
        assert!(wrap_explain("select 1", true).contains("ANALYZE"));
        assert!(!wrap_explain("select 1", false).contains("ANALYZE"));
    }

    #[test]
    fn parse_scan_golden() {
        let plan = parse_json(include_str!("../tests/fixtures/explain/scan.json")).unwrap();
        assert_eq!(plan.root.kind, "Seq Scan");
        assert_eq!(plan.root.relation.as_deref(), Some("items"));
        assert_eq!(plan.root.estimates.rows, Some(1000.0));
        assert_eq!(plan.root.actual.rows, Some(1000.0));
        assert_eq!(plan.root.loops, Some(1));
        assert_eq!(plan.planning_ms, Some(0.042));
        assert!(plan.raw.contains("Seq Scan"));
    }
}
