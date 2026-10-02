use std::ops::Range;
use std::sync::Arc;

use dexo_driver_api::{QueryEvent, QueryRequest, Session};
use dexo_sql::{Dialect, StatementEffect, split_statements_in, statement_at_in};

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
        "duckdb" => Dialect::Duckdb,
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
    statement_spans_for_dialect(sql, target, cursor, selection, dialect)
        .into_iter()
        .map(|(_, statement)| statement)
        .collect()
}

/// [`statements_for_dialect`], each with the byte offset in `sql` it starts at.
pub fn statement_spans_for_dialect(
    sql: &str,
    target: ExecutionTarget,
    cursor: usize,
    selection: Option<Range<usize>>,
    dialect: Dialect,
) -> Vec<(usize, String)> {
    let fragment = match target {
        ExecutionTarget::Document => Some(0..sql.len()),
        ExecutionTarget::CurrentStatement => {
            statement_at_in(sql, cursor, dialect).map(|span| span.byte_range)
        }
        ExecutionTarget::Selection => selection.filter(|range| sql.get(range.clone()).is_some()),
    }
    .unwrap_or(0..0);
    let base = fragment.start;
    let fragment = &sql[fragment];
    split_statements_in(fragment, dialect)
        .into_iter()
        .filter_map(|span| {
            let text = &fragment[span.byte_range.clone()];
            let trimmed = text.trim();
            let leading = text.len() - text.trim_start().len();
            (!trimmed.is_empty())
                .then(|| (base + span.byte_range.start + leading, trimmed.to_string()))
        })
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
        read_only: bool,
        parameters: Vec<(String, dexo_driver_api::DbValue)>,
        timeout: std::time::Duration,
    ) -> Vec<Result<Vec<QueryEvent>, AppError>> {
        let statements = statements_for_dialect(sql, target, cursor, selection, dialect);
        let mut out = Vec::new();
        for statement in statements {
            let effect = split_statements_in(&statement, dialect)
                .first()
                .map_or(StatementEffect::Unknown, |span| span.effect);
            // `:name` is bound by name, each statement taking only its own values.
            let (statement, values) = dexo_sql::bind_named(&statement, dialect, &parameters);
            let mut request = if effect == StatementEffect::ReadOnly {
                QueryRequest::read(statement, row_limit)
            } else {
                QueryRequest::write(statement)
            };
            // Whatever it is taken for, what it returns stops at the limit, and on a
            // read-only connection it runs where it cannot write.
            request.row_limit = row_limit;
            request.read_only = read_only;
            request.timeout = timeout;
            request.parameters = values;
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
