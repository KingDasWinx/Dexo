use std::ops::Range;
use std::sync::Arc;

use dexo_driver_api::{QueryEvent, QueryRequest, Session};
use dexo_sql::{Dialect, split_statements_in, statement_at_in};

use crate::error::AppError;
use crate::query_service::QueryService;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionTarget {
    Selection,
    CurrentStatement,
    Document,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptPolicy {
    StopOnError,
    ContinueOnError,
}

pub fn statements_for(
    sql: &str,
    target: ExecutionTarget,
    cursor: usize,
    selection: Option<Range<usize>>,
) -> Vec<String> {
    statements_for_dialect(sql, target, cursor, selection, Dialect::Postgres)
}

/// The SQL dialect a driver speaks: MariaDB's is MySQL's.
pub fn dialect_for_driver(driver: &str) -> Dialect {
    match dexo_driver_api::DriverDescriptor::family(driver) {
        "mysql" => Dialect::Mysql,
        "sqlite" => Dialect::Sqlite,
        _ => Dialect::Postgres,
    }
}

/// [`statements_for`] split the way `dialect` reads comments and strings, so a MySQL
/// `#` comment is a comment and not a statement of its own.
pub fn statements_for_dialect(
    sql: &str,
    target: ExecutionTarget,
    cursor: usize,
    selection: Option<Range<usize>>,
    dialect: Dialect,
) -> Vec<String> {
    let fragment = match target {
        ExecutionTarget::Document => sql.to_string(),
        ExecutionTarget::CurrentStatement => statement_at_in(sql, cursor, dialect)
            .map(|span| sql[span.byte_range].to_string())
            .unwrap_or_default(),
        ExecutionTarget::Selection => selection
            .and_then(|range| sql.get(range))
            .unwrap_or("")
            .to_string(),
    };
    split_statements_in(&fragment, dialect)
        .into_iter()
        .map(|span| fragment[span.byte_range].trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn run_statements<T, E>(
    statements: &[String],
    policy: ScriptPolicy,
    mut exec: impl FnMut(&str) -> Result<T, E>,
) -> Vec<Result<T, E>> {
    let mut out = Vec::new();
    for sql in statements {
        let result = exec(sql);
        let failed = result.is_err();
        out.push(result);
        if failed && policy == ScriptPolicy::StopOnError {
            break;
        }
    }
    out
}

impl QueryService {
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_script(
        &self,
        session: Arc<dyn Session>,
        sql: &str,
        dialect: Dialect,
        target: ExecutionTarget,
        cursor: usize,
        selection: Option<Range<usize>>,
        policy: ScriptPolicy,
        row_limit: u64,
        mutating: bool,
        parameters: Vec<dexo_driver_api::DbValue>,
        timeout: std::time::Duration,
    ) -> Vec<Result<Vec<QueryEvent>, AppError>> {
        let statements = statements_for_dialect(sql, target, cursor, selection, dialect);
        let mut out = Vec::new();
        for statement in statements {
            let mut request = if mutating {
                QueryRequest::write(statement)
            } else {
                QueryRequest::read(statement, row_limit)
            };
            request.timeout = timeout;
            request.parameters = parameters.clone();
            let result = self.collect(Arc::clone(&session), request).await;
            let failed = result.is_err();
            out.push(result);
            if failed && policy == ScriptPolicy::StopOnError {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecutionTarget, ScriptPolicy, run_statements, statements_for};

    #[test]
    fn three_statements_are_planned_in_order() {
        let stmts = statements_for(
            "select 1; select 2; select 3;",
            ExecutionTarget::Document,
            0,
            None,
        );
        assert_eq!(stmts, ["select 1", "select 2", "select 3"]);
    }

    #[test]
    fn stop_on_error_skips_later_statements() {
        let stmts = vec!["select 1".into(), "bad".into(), "select 3".into()];
        let results = run_statements(&stmts, ScriptPolicy::StopOnError, |sql| {
            if sql == "bad" {
                Err("fail")
            } else {
                Ok(sql.to_string())
            }
        });
        assert_eq!(results.len(), 2);
        assert!(results[1].is_err());
    }

    #[test]
    fn continue_on_error_runs_remaining() {
        let stmts = vec!["select 1".into(), "bad".into(), "select 3".into()];
        let results = run_statements(&stmts, ScriptPolicy::ContinueOnError, |sql| {
            if sql == "bad" {
                Err("fail")
            } else {
                Ok(sql.to_string())
            }
        });
        assert_eq!(results.len(), 3);
        assert!(results[2].is_ok());
    }
}
