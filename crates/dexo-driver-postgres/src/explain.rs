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
    let children = value
        .get("Plans")
        .and_then(serde_json::Value::as_array)
        .map(|plans| plans.iter().map(parse_node).collect())
        .unwrap_or_default();
    PlanNode {
        kind: node_kind(value),
        relation: node_relation(value),
        detail: node_detail(value),
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

/// The node's name as psql prints it. The JSON spreads it over several keys: an
/// `Aggregate` with `Strategy: Hashed` is a HashAggregate, a `Hash Join` with
/// `Join Type: Left` a Hash Left Join, a `ModifyTable` with `Operation: Update` an Update.
fn node_kind(value: &serde_json::Value) -> String {
    let node = text(value, "Node Type").unwrap_or("Unknown");
    let join = text(value, "Join Type").filter(|join| *join != "Inner");
    match (node, join) {
        ("Aggregate", _) => match text(value, "Strategy") {
            Some("Hashed") => "HashAggregate",
            Some("Sorted") => "GroupAggregate",
            Some("Mixed") => "MixedAggregate",
            _ => "Aggregate",
        }
        .to_string(),
        ("Hash Join" | "Merge Join", Some(join)) => {
            format!("{} {join} Join", node.trim_end_matches(" Join"))
        }
        ("Nested Loop", Some(join)) => format!("Nested Loop {join} Join"),
        ("ModifyTable", _) => text(value, "Operation").unwrap_or(node).to_string(),
        _ => node.to_string(),
    }
}

/// What the node reads, with its alias when the query gave it one (`customers c`). An
/// index scan names its table here and its index in the detail; a bitmap index scan has
/// only the index.
fn node_relation(value: &serde_json::Value) -> Option<String> {
    let relation = text(value, "Relation Name")
        .or_else(|| text(value, "CTE Name"))
        .or_else(|| text(value, "Function Name"))
        .or_else(|| text(value, "Index Name"))?;
    match text(value, "Alias") {
        Some(alias) if alias != relation => Some(format!("{relation} {alias}")),
        _ => Some(relation.to_string()),
    }
}

fn node_detail(value: &serde_json::Value) -> Option<String> {
    let mut parts = Vec::new();
    if value.get("Relation Name").is_some()
        && let Some(index) = text(value, "Index Name")
    {
        parts.push(format!("using {index}"));
    }
    for (key, word) in [
        ("Hash Cond", "on"),
        ("Merge Cond", "on"),
        ("Index Cond", "cond"),
        ("Recheck Cond", "recheck"),
        ("Join Filter", "filter"),
        ("Filter", "filter"),
    ] {
        if let Some(condition) = text(value, key) {
            parts.push(format!("{word} {condition}"));
        }
    }
    for key in ["Sort Key", "Group Key"] {
        if let Some(keys) = value.get(key).and_then(serde_json::Value::as_array) {
            let keys: Vec<&str> = keys.iter().filter_map(serde_json::Value::as_str).collect();
            if !keys.is_empty() {
                parts.push(format!("by {}", keys.join(", ")));
            }
        }
    }
    if let Some(removed) = number(value, "Rows Removed by Filter").filter(|removed| *removed > 0.0)
    {
        parts.push(format!("{removed} removed by filter"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
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
        // the fence. Line breaks around the statement, because one ending in a `--`
        // comment would otherwise comment out the ROLLBACK and leave the change pending.
        let fenced = format!("{open};\n{}\n;\n{close}", wrap_explain(sql, true));
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
        if !request.hypothetical_indexes.is_empty() {
            return self
                .explain_with_indexes(&request.sql, &request.hypothetical_indexes, request.analyze)
                .await;
        }
        if request.analyze {
            return self.explain_analyzed(&request.sql).await;
        }
        self.explain_estimated(&request.sql).await
    }
}

impl PostgresSession {
    async fn explain_estimated(&self, sql: &str) -> Result<ExplainPlan, DriverError> {
        let sql = wrap_explain(sql, false);
        let row = self.client.query_one(&sql, &[]).await.map_err(map_error)?;
        if let Ok(value) = row.try_get::<_, serde_json::Value>(0) {
            return parse_value(&value, &value.to_string());
        }
        let text: String = row.try_get(0).map_err(map_error)?;
        parse_json(&text)
    }

    /// The estimated plan with `indexes` as hypopg's hypothetical ones: made on this
    /// session, which alone sees them, used by EXPLAIN only, and dropped after it however
    /// it went.
    async fn explain_with_indexes(
        &self,
        sql: &str,
        indexes: &[String],
        analyze: bool,
    ) -> Result<ExplainPlan, DriverError> {
        if analyze {
            return Err(DriverError::unsupported(
                "a hypothetical index exists only for the planner: try it on an estimated plan",
            ));
        }
        for index in indexes {
            if !is_create_index(index) {
                return Err(DriverError::new(
                    DriverErrorCategory::Syntax,
                    format!(
                        "not an index definition: {index} (write CREATE INDEX ON table (columns))"
                    ),
                ));
            }
        }
        let installed = self
            .client
            .query_opt("SELECT 1 FROM pg_extension WHERE extname = 'hypopg'", &[])
            .await
            .map_err(map_error)?
            .is_some();
        if !installed {
            return Err(DriverError::unsupported(
                "trying an index needs the hypopg extension: install its package on the server, then run CREATE EXTENSION hypopg",
            ));
        }
        let mut made = Ok(());
        for index in indexes {
            if let Err(error) = self
                .client
                .query("SELECT indexrelid FROM hypopg_create_index($1)", &[index])
                .await
            {
                made = Err(map_error(error));
                break;
            }
        }
        let plan = match made {
            Ok(()) => self.explain_estimated(sql).await,
            Err(error) => Err(error),
        };
        // Whatever happened, the session keeps no hypothetical index for a later plan.
        let _ = self.client.batch_execute("SELECT hypopg_reset()").await;
        plan
    }
}

/// `CREATE [UNIQUE] INDEX ...`: what hypopg takes, and nothing else.
fn is_create_index(definition: &str) -> bool {
    let words: Vec<String> = definition
        .split_whitespace()
        .take(3)
        .map(str::to_ascii_uppercase)
        .collect();
    !definition.contains(';')
        && words.first().map(String::as_str) == Some("CREATE")
        && (words.get(1).map(String::as_str) == Some("INDEX")
            || (words.get(1).map(String::as_str) == Some("UNIQUE")
                && words.get(2).map(String::as_str) == Some("INDEX")))
}

#[cfg(test)]
mod tests {
    use super::{parse_json, wrap_explain};

    #[test]
    fn only_an_index_definition_is_tried() {
        assert!(super::is_create_index(
            "CREATE INDEX ON orders (customer_id)"
        ));
        assert!(super::is_create_index("create unique index on t (a, b)"));
        assert!(!super::is_create_index("drop table orders"));
        assert!(!super::is_create_index(
            "create index on t (a); drop table t"
        ));
        assert!(!super::is_create_index("create table t (a int)"));
    }

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
