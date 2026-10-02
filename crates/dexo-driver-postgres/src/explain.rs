use dexo_driver_api::{
    DriverError, DriverErrorCategory, ExplainPlan, ExplainProvider, ExplainRequest, PlanMetrics,
    PlanNode,
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
fn analyze_fence(in_transaction: bool) -> (&'static str, &'static str) {
    if !in_transaction {
        ("BEGIN", "ROLLBACK")
    } else {
        (
            "SAVEPOINT dexo_explain",
            "ROLLBACK TO SAVEPOINT dexo_explain; RELEASE SAVEPOINT dexo_explain",
        )
    }
}

/// `$1` with no value bound (42P02), or one whose type nothing tells (42P18).
fn parameter_error(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| matches!(code.code(), "42P02" | "42P18"))
}

fn first_text(messages: &[SimpleQueryMessage]) -> Result<String, DriverError> {
    messages
        .iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        })
        .ok_or_else(|| DriverError::new(DriverErrorCategory::Internal, "explain returned no plan"))
}

impl PostgresSession {
    /// A statement with parameters has no values to plan with. Postgres 16 plans it for
    /// any value (`GENERIC_PLAN`); the caller asks only a server that knows the option.
    async fn explain_generic(&self, sql: &str) -> Result<ExplainPlan, DriverError> {
        let inner = sql.trim().trim_end_matches(';');
        let explain = format!("EXPLAIN (GENERIC_PLAN, FORMAT JSON)\n{inner}\n");
        match self.client.simple_query(&explain).await {
            Ok(messages) => parse_json(&first_text(&messages)?),
            // A parameter whose type nothing tells has no generic plan either.
            Err(error) if parameter_error(&error) => Err(dexo_driver_api::parameters_unsupported()),
            Err(error) => Err(map_error(error)),
        }
    }

    /// Runs `step` inside a savepoint when the session is in a transaction -- begun from
    /// Dexo or typed -- and rolls back to it after. A failed statement aborts the user's
    /// transaction, and everything after it fails with it: a plan asked of a statement
    /// with parameters used to leave them "current transaction is aborted". A plan
    /// changes nothing, so rolling back always is safe.
    async fn fenced<T>(
        &self,
        step: impl std::future::Future<Output = Result<T, DriverError>>,
    ) -> Result<T, DriverError> {
        let fenced = match self.client.batch_execute("SAVEPOINT dexo_plan").await {
            Ok(()) => true,
            Err(error)
                if error.code()
                    == Some(&tokio_postgres::error::SqlState::NO_ACTIVE_SQL_TRANSACTION) =>
            {
                false
            }
            Err(error) => return Err(map_error(error)),
        };
        let outcome = step.await;
        if fenced
            && let Err(error) = self
                .client
                .batch_execute("ROLLBACK TO SAVEPOINT dexo_plan; RELEASE SAVEPOINT dexo_plan")
                .await
        {
            return Err(outcome.err().unwrap_or_else(|| map_error(error)));
        }
        outcome
    }

    /// Whether a transaction is open: one Dexo began, or one the user typed, which the
    /// session's own state never hears of. Outside a transaction block -- the implicit
    /// one two statements sent together make included -- a SAVEPOINT is refused, and
    /// inside one this pair leaves nothing behind.
    async fn in_transaction(&self) -> Result<bool, DriverError> {
        match self
            .client
            .batch_execute("SAVEPOINT dexo_probe; RELEASE SAVEPOINT dexo_probe")
            .await
        {
            Ok(()) => Ok(true),
            Err(error)
                if error.code()
                    == Some(&tokio_postgres::error::SqlState::NO_ACTIVE_SQL_TRANSACTION) =>
            {
                Ok(false)
            }
            Err(error) => Err(map_error(error)),
        }
    }

    async fn explain_analyzed(&self, sql: &str) -> Result<ExplainPlan, DriverError> {
        let (open, close) = analyze_fence(self.in_transaction().await?);
        // One simple query, so nothing else sent on this shared connection lands inside
        // the fence. Line breaks around the statement, because one ending in a `--`
        // comment would otherwise comment out the ROLLBACK and leave the change pending.
        let fenced = format!("{open};\n{}\n;\n{close}", wrap_explain(sql, true));
        match self.client.simple_query(&fenced).await {
            Ok(messages) => parse_json(&first_text(&messages)?),
            Err(error) => {
                // The server skips the rest of the string after an error, so the fence is
                // still open; closing it keeps the session out of an aborted transaction.
                let _ = self.client.batch_execute(close).await;
                // ANALYZE runs the statement, which needs its parameters' values.
                if parameter_error(&error) {
                    return Err(dexo_driver_api::parameters_unsupported());
                }
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
        if let Some(plan) = self.fenced(self.explain_valued(sql)).await? {
            return Ok(plan);
        }
        // Before 16 the server knows no GENERIC_PLAN, and asking it would be one more
        // error for nothing.
        if self.server_version().await? >= 160_000 {
            return self.fenced(self.explain_generic(sql)).await;
        }
        Err(dexo_driver_api::parameters_unsupported())
    }

    /// The plan of a statement without parameters, or `None` for one with them.
    async fn explain_valued(&self, sql: &str) -> Result<Option<ExplainPlan>, DriverError> {
        let statement = match self.client.prepare(&wrap_explain(sql, false)).await {
            Ok(statement) if statement.params().is_empty() => statement,
            Ok(_) => return Ok(None),
            Err(error) if parameter_error(&error) => return Ok(None),
            Err(error) => return Err(map_error(error)),
        };
        // An EXPLAIN is not planned until it runs, so a parameter can show only then.
        let row = match self.client.query_one(&statement, &[]).await {
            Ok(row) => row,
            Err(error) if parameter_error(&error) => return Ok(None),
            Err(error) => return Err(map_error(error)),
        };
        if let Ok(value) = row.try_get::<_, serde_json::Value>(0) {
            return parse_value(&value, &value.to_string()).map(Some);
        }
        let text: String = row.try_get(0).map_err(map_error)?;
        parse_json(&text).map(Some)
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
        let indexes = indexes
            .iter()
            .map(|index| {
                index_definition(index).ok_or_else(|| {
                    DriverError::new(
                        DriverErrorCategory::Syntax,
                        format!(
                            "not an index definition: {index} (write CREATE INDEX ON table (columns))"
                        ),
                    )
                })
            })
            .collect::<Result<Vec<&str>, _>>()?;
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
        // Inside the user's transaction a failure would abort it, and the reset after it
        // would fail too while hypopg's indexes, which no rollback touches, stayed for
        // every later plan. A savepoint fences them: rolled back to, the transaction can
        // run the reset again.
        let fenced = match self.client.batch_execute("SAVEPOINT dexo_hypopg").await {
            Ok(()) => true,
            Err(error)
                if error.code()
                    == Some(&tokio_postgres::error::SqlState::NO_ACTIVE_SQL_TRANSACTION) =>
            {
                false
            }
            Err(error) => return Err(map_error(error)),
        };
        // Dexo drops the indexes it made and only those: hypopg_reset() would also take
        // the ones the user made on this session.
        let mut made = Vec::new();
        let mut failed = None;
        for index in &indexes {
            match self
                .client
                .query("SELECT indexrelid FROM hypopg_create_index($1)", &[index])
                .await
            {
                Ok(rows) => made.extend(rows.iter().map(|row| row.get::<_, u32>(0))),
                Err(error) => {
                    failed = Some(map_error(error));
                    break;
                }
            }
        }
        let plan = match failed {
            None => self.explain_estimated(sql).await,
            Some(error) => Err(error),
        };
        // Whatever happened, the session keeps no hypothetical index of Dexo's for a later
        // plan, or says it could not get rid of them.
        let dropped = async {
            if fenced {
                self.client
                    .batch_execute(
                        "ROLLBACK TO SAVEPOINT dexo_hypopg; RELEASE SAVEPOINT dexo_hypopg",
                    )
                    .await?;
            }
            self.client
                .execute(
                    "SELECT hypopg_drop_index(id) FROM unnest($1::oid[]) id",
                    &[&made],
                )
                .await
        };
        if let Err(error) = dropped.await {
            return Err(DriverError::new(
                DriverErrorCategory::Internal,
                format!(
                    "the hypothetical indexes could not be dropped, and this session's plans may still use them until it reconnects: {}",
                    map_error(error)
                ),
            ));
        }
        plan
    }
}

/// `CREATE [UNIQUE] INDEX ...` as one statement, what hypopg takes and nothing else,
/// without its trailing `;`. A `;` in a string, a quoted name or a comment is part of the
/// definition; any other ends it, and something after that is a second statement.
fn index_definition(definition: &str) -> Option<&str> {
    use dexo_sql::TokenKind;
    let tokens: Vec<dexo_sql::Token> = dexo_sql::tokenize(definition, dexo_sql::Dialect::Postgres)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Comment)
        .collect();
    let semicolon =
        |token: &dexo_sql::Token| token.kind == TokenKind::Punct && token.text(definition) == ";";
    let (body, end) = match tokens.split_last() {
        Some((last, body)) if semicolon(last) => (body, last.span.start),
        _ => (&tokens[..], definition.len()),
    };
    let words: Vec<String> = body
        .iter()
        .take(3)
        .map(|token| token.text(definition).to_ascii_uppercase())
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let create_index = matches!(
        words.as_slice(),
        ["CREATE", "INDEX", ..] | ["CREATE", "UNIQUE", "INDEX"]
    );
    (create_index && !body.iter().any(semicolon)).then(|| definition[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::{parse_json, wrap_explain};

    #[test]
    fn only_an_index_definition_is_tried() {
        let tried = super::index_definition;
        assert_eq!(
            tried("CREATE INDEX ON orders (customer_id)"),
            Some("CREATE INDEX ON orders (customer_id)")
        );
        assert!(tried("create unique index on t (a, b)").is_some());
        assert!(tried("drop table orders").is_none());
        assert!(tried("create index on t (a); drop table t").is_none());
        assert!(tried("create index on t (a);;").is_none());
        assert!(tried("create table t (a int)").is_none());
        // One trailing `;` is dropped, and one in a literal, a quoted name or a comment
        // is part of the definition.
        assert_eq!(
            tried("CREATE INDEX ON orders (customer_id);  -- try it\n"),
            Some("CREATE INDEX ON orders (customer_id)")
        );
        assert_eq!(
            tried("create index on \"a;b\" (c) where note <> 'x;y' /* ; */;"),
            Some("create index on \"a;b\" (c) where note <> 'x;y' /* ; */")
        );
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
