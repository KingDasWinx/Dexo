//! What the SQL editor has to ask before it runs a script on a connection.

use dexo_sql::{Dialect, destructive, is_read};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunPolicy {
    pub connection: String,
    pub read_only: bool,
    pub confirm_destructive: bool,
    pub production: bool,
}

/// A statement the user has to see before it runs, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Flagged {
    pub index: usize,
    pub sql: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunVerdict {
    Run,
    /// Read-only connection: the first statement that is not a read, and nothing is sent.
    Refuse {
        index: usize,
        sql: String,
    },
    /// `typed` is the connection name that has to be typed, on production.
    Confirm {
        flagged: Vec<Flagged>,
        typed: Option<String>,
    },
}

const NOT_A_READ: &str = "not a read-only statement";

/// Why a statement that is not destructive still counts as a write. A query that calls a
/// function Dexo does not take for a read says which, rather than "not a read-only
/// statement" for what looks like a plain SELECT.
fn write_reason(sql: &str, dialect: Dialect) -> String {
    match dexo_sql::inspect_read(sql, dialect) {
        Err(dexo_sql::GuardRejection::Function(name)) => {
            format!("calls {name}, which Dexo does not take for a read")
        }
        _ => NOT_A_READ.to_string(),
    }
}

/// Read-only refuses any write; production asks for the connection's name before any
/// write; elsewhere, `confirm_destructive` asks before the destructive ones. Unknown
/// statements count as writes.
pub fn judge(statements: &[String], dialect: Dialect, policy: &RunPolicy) -> RunVerdict {
    let writes: Vec<usize> = statements
        .iter()
        .enumerate()
        .filter(|(_, sql)| !is_read(sql, dialect))
        .map(|(index, _)| index)
        .collect();
    if policy.read_only {
        return match writes.first() {
            Some(&index) => RunVerdict::Refuse {
                index,
                sql: statements[index].clone(),
            },
            None => RunVerdict::Run,
        };
    }
    let flag = |index: usize, reason: String| Flagged {
        index,
        sql: statements[index].clone(),
        reason,
    };
    if policy.production && !writes.is_empty() {
        let flagged = writes
            .iter()
            .map(|&index| {
                let reason = destructive(&statements[index], dialect).map_or_else(
                    || write_reason(&statements[index], dialect),
                    |found| found.describe().to_string(),
                );
                flag(index, reason)
            })
            .collect();
        return RunVerdict::Confirm {
            flagged,
            typed: Some(policy.connection.clone()),
        };
    }
    if policy.confirm_destructive {
        let flagged: Vec<Flagged> = writes
            .iter()
            .filter_map(|&index| {
                destructive(&statements[index], dialect)
                    .map(|found| flag(index, found.describe().to_string()))
            })
            .collect();
        if !flagged.is_empty() {
            return RunVerdict::Confirm {
                flagged,
                typed: None,
            };
        }
    }
    RunVerdict::Run
}

#[cfg(test)]
mod tests {
    use super::{RunPolicy, RunVerdict, judge};
    use dexo_sql::{Destructive, Dialect};

    fn policy(read_only: bool, confirm_destructive: bool, production: bool) -> RunPolicy {
        RunPolicy {
            connection: "shop".into(),
            read_only,
            confirm_destructive,
            production,
        }
    }

    fn script(statements: &[&str]) -> Vec<String> {
        statements.iter().map(|sql| sql.to_string()).collect()
    }

    #[test]
    fn reads_run_everywhere() {
        for policy in [
            policy(true, true, true),
            policy(false, true, true),
            policy(false, false, false),
        ] {
            assert_eq!(
                judge(
                    &script(&["select 1", "show tables"]),
                    Dialect::Postgres,
                    &policy
                ),
                RunVerdict::Run
            );
        }
    }

    #[test]
    fn a_read_only_connection_refuses_the_first_write() {
        let statements = script(&[
            "select 1",
            "set default_transaction_read_only = off",
            "delete from items",
        ]);
        assert_eq!(
            judge(&statements, Dialect::Postgres, &policy(true, false, false)),
            RunVerdict::Refuse {
                index: 1,
                sql: "set default_transaction_read_only = off".into()
            }
        );
    }

    #[test]
    fn production_asks_for_the_name_on_any_write() {
        let statements = script(&[
            "select 1",
            "insert into items values (1)",
            "delete from items",
        ]);
        let verdict = judge(&statements, Dialect::Postgres, &policy(false, false, true));
        let RunVerdict::Confirm { flagged, typed } = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(typed.as_deref(), Some("shop"));
        assert_eq!(
            flagged
                .iter()
                .map(|flagged| flagged.index)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(flagged[0].reason, "not a read-only statement");
        assert_eq!(
            flagged[1].reason,
            Destructive::DeleteWithoutWhere.describe()
        );
    }

    #[test]
    fn elsewhere_only_destructive_statements_ask() {
        let ask = policy(false, true, false);
        assert_eq!(
            judge(
                &script(&["delete from items where id = 1"]),
                Dialect::Postgres,
                &ask
            ),
            RunVerdict::Run
        );
        let verdict = judge(
            &script(&["update items set n = 0"]),
            Dialect::Postgres,
            &ask,
        );
        let RunVerdict::Confirm { flagged, typed } = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(typed, None);
        assert_eq!(flagged.len(), 1);
    }

    /// A SELECT that calls a function Dexo does not take for a read says which.
    #[test]
    fn a_query_calling_a_function_says_which() {
        let verdict = judge(
            &script(&["select pg_sleep(1)"]),
            Dialect::Postgres,
            &policy(false, false, true),
        );
        let RunVerdict::Confirm { flagged, .. } = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(
            flagged[0].reason,
            "calls pg_sleep, which Dexo does not take for a read"
        );
    }

    #[test]
    fn turning_confirmation_off_runs_destructive_statements_off_production() {
        assert_eq!(
            judge(
                &script(&["drop table items"]),
                Dialect::Postgres,
                &policy(false, false, false)
            ),
            RunVerdict::Run
        );
    }
}
