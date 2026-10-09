//! Writes a person lets run without asking. Every write waits for their approval unless
//! the write access it runs under lets its operations through: inserts only, say, or
//! everything. A script runs without asking only when every statement of it may.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCategory};
use crate::mcp::grant::{Grant, WriteKind};

/// What a write does, as far as asking goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    /// INSERT, and `data_insert`.
    Insert,
    /// UPDATE with a WHERE that picks rows, and `data_update`.
    Update,
    /// DELETE with a WHERE that picks rows, and `data_delete`.
    Delete,
    /// What reaches every row: UPDATE or DELETE without a WHERE, or with one every row
    /// passes, and TRUNCATE.
    EveryRow,
    /// CREATE TABLE, INDEX or VIEW, and an ALTER TABLE that drops nothing.
    Create,
    /// DROP, and ALTER TABLE … DROP.
    Drop,
    /// Cancelling a query, ending a server session.
    Sessions,
}

impl Operation {
    pub const ALL: [Operation; 7] = [
        Self::Insert,
        Self::Update,
        Self::Delete,
        Self::EveryRow,
        Self::Create,
        Self::Drop,
        Self::Sessions,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::EveryRow => "every-row",
            Self::Create => "create",
            Self::Drop => "drop",
            Self::Sessions => "sessions",
        }
    }

    /// `insert`, `every-row`, …; `all` is every one of them.
    pub fn parse_list(value: &str) -> Result<Vec<Self>, AppError> {
        let mut operations = Vec::new();
        for word in value
            .split(',')
            .map(str::trim)
            .filter(|word| !word.is_empty())
        {
            if word.eq_ignore_ascii_case("all") {
                return Ok(Self::ALL.to_vec());
            }
            let operation = Self::ALL
                .into_iter()
                .find(|operation| operation.as_str().eq_ignore_ascii_case(word))
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::Configuration,
                        format!(
                            "'{word}' is not an operation: use insert, update, delete, every-row, create, drop, sessions or all"
                        ),
                    )
                })?;
            if !operations.contains(&operation) {
                operations.push(operation);
            }
        }
        operations.sort();
        Ok(operations)
    }

    /// The kind of write access it needs.
    pub fn kind(self) -> WriteKind {
        match self {
            Self::Insert | Self::Update | Self::Delete | Self::EveryRow => WriteKind::Rows,
            Self::Create | Self::Drop => WriteKind::Schema,
            Self::Sessions => WriteKind::Sessions,
        }
    }

    /// It destroys what is there, at once and for good: a person should see it first.
    pub fn destructive(self) -> bool {
        matches!(self, Self::EveryRow | Self::Drop | Self::Sessions)
    }
}

/// `insert, update`.
pub fn operations_words(operations: &[Operation]) -> String {
    operations
        .iter()
        .map(|operation| operation.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What letting `operations` run without asking risks, said before it is given; none when
/// every write asks.
pub fn bypass_risk(operations: &[Operation], production: bool) -> Option<String> {
    if operations.is_empty() {
        return None;
    }
    let place = if production { " on production" } else { "" };
    let harms: Vec<&str> = operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::EveryRow => Some("empty tables"),
            Operation::Drop => Some("drop tables"),
            Operation::Sessions => Some("end sessions"),
            _ => None,
        })
        .collect();
    let consequence = match harms.as_slice() {
        [] => "you see them only afterwards, in Activity".to_string(),
        [one] => format!("an agent can {one} before you see it"),
        [rest @ .., last] => format!(
            "an agent can {} or {last} before you see it",
            rest.join(", ")
        ),
    };
    Some(format!(
        "{} without asking{place}: {consequence}",
        operations_words(operations)
    ))
}

/// What a call of the write tool `tool` with `arguments` does, statement by statement for
/// a script. A statement the guard cannot read is refused where it runs; it is never let
/// through here, since it is no operation at all.
pub fn operations_of(
    tool: &str,
    arguments: &serde_json::Map<String, serde_json::Value>,
    dialect: dexo_sql::Dialect,
) -> Vec<Operation> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut operations = match tool {
        "data_insert" => vec![Operation::Insert],
        "data_update" => vec![Operation::Update],
        "data_delete" => vec![Operation::Delete],
        "admin_cancel_query" | "admin_terminate_session" => vec![Operation::Sessions],
        "data_execute_sql" => dexo_sql::inspect_data_script(sql, dialect)
            .map(|statements| {
                statements
                    .iter()
                    .flat_map(|statement| data_operations(&statement.sql, dialect))
                    .collect()
            })
            .unwrap_or_default(),
        "schema_apply_ddl" => vec![match dexo_sql::destructive(sql, dialect) {
            Some(dexo_sql::Destructive::Drop | dexo_sql::Destructive::AlterDrop) => Operation::Drop,
            _ => Operation::Create,
        }],
        _ => Vec::new(),
    };
    operations.sort();
    operations.dedup();
    operations
}

fn data_operations(sql: &str, dialect: dexo_sql::Dialect) -> Vec<Operation> {
    use dexo_sql::Destructive;
    if matches!(
        dexo_sql::destructive(sql, dialect),
        Some(
            Destructive::DeleteWithoutWhere
                | Destructive::UpdateWithoutWhere
                | Destructive::DeleteAlways
                | Destructive::UpdateAlways
                | Destructive::Truncate
        )
    ) {
        return vec![Operation::EveryRow];
    }
    let upper = sql.to_ascii_uppercase();
    match upper.split_whitespace().next() {
        // An upsert changes the rows already there too.
        Some("INSERT")
            if (upper.contains("ON CONFLICT") && upper.contains("DO UPDATE"))
                || upper.contains("ON DUPLICATE KEY UPDATE") =>
        {
            vec![Operation::Insert, Operation::Update]
        }
        Some("INSERT") => vec![Operation::Insert],
        Some("UPDATE") => vec![Operation::Update],
        Some("DELETE") => vec![Operation::Delete],
        // Anything else -- MySQL's REPLACE among them -- is taken as reaching every row,
        // so it asks unless that is let through too.
        _ => vec![Operation::EveryRow],
    }
}

impl Grant {
    /// It lets `operations` run without asking, those of the kinds it was given.
    pub fn with_bypass(mut self, operations: &[Operation]) -> Self {
        let kinds = self.kinds();
        self.bypass = operations
            .iter()
            .copied()
            .filter(|operation| kinds.contains(&operation.kind()))
            .collect();
        self.bypass.sort();
        self.bypass.dedup();
        self
    }

    /// A write doing `operations` runs without asking: every one of them is let through.
    /// One that does nothing it can tell asks.
    pub fn bypasses(&self, operations: &[Operation]) -> bool {
        !operations.is_empty()
            && operations
                .iter()
                .all(|operation| self.bypass.contains(operation))
    }
}

#[cfg(test)]
mod tests {
    use super::{Operation, operations_of};
    use dexo_sql::Dialect;
    use serde_json::json;

    fn of(tool: &str, sql: &str) -> Vec<Operation> {
        operations_of(
            tool,
            json!({ "sql": sql }).as_object().unwrap(),
            Dialect::Postgres,
        )
    }

    /// Each statement is what it does: a WHERE that picks rows is a change, one that
    /// reaches every row is not, and a script is all of its statements.
    #[test]
    fn a_write_is_named_by_what_it_does() {
        use Operation::*;
        assert_eq!(of("data_insert", ""), [Insert]);
        assert_eq!(
            of(
                "data_execute_sql",
                "insert into a values (1); update a set x = 1 where id = 2; delete from a where id = 3"
            ),
            [Insert, Update, Delete]
        );
        for sql in [
            "delete from a",
            "update a set x = 1",
            "delete from a where 1 = 1",
            "truncate a",
        ] {
            assert_eq!(of("data_execute_sql", sql), [EveryRow], "{sql}");
        }
        assert_eq!(of("schema_apply_ddl", "create index i on a (x)"), [Create]);
        assert_eq!(
            of("schema_apply_ddl", "alter table a add column y int"),
            [Create]
        );
        assert_eq!(
            of("schema_apply_ddl", "alter table a drop column y"),
            [Drop]
        );
        assert_eq!(of("schema_apply_ddl", "drop table a"), [Drop]);
        assert!(of("data_execute_sql", "select 1").is_empty());
        assert_eq!(
            of(
                "data_execute_sql",
                "insert into a values (1) on conflict (id) do update set x = 2"
            ),
            [Insert, Update]
        );
        assert_eq!(
            Operation::parse_list("insert, update").unwrap(),
            [Insert, Update]
        );
        assert_eq!(Operation::parse_list("all").unwrap(), Operation::ALL);
        assert!(Operation::parse_list("insrt").is_err());
    }

    /// A script runs without asking only when every operation of it may.
    /// The risk names what it lets an agent do, and only that.
    #[test]
    fn the_risk_says_what_runs_without_asking_and_what_it_can_do() {
        use super::bypass_risk;
        assert_eq!(bypass_risk(&[], true), None);
        assert_eq!(
            bypass_risk(&[Operation::Insert], false).unwrap(),
            "insert without asking: you see them only afterwards, in Activity"
        );
        assert_eq!(
            bypass_risk(
                &[Operation::Delete, Operation::EveryRow, Operation::Drop],
                true
            )
            .unwrap(),
            "delete, every-row, drop without asking on production: an agent can empty tables or drop tables before you see it"
        );
    }

    #[test]
    fn a_script_runs_without_asking_only_when_all_of_it_may() {
        use crate::mcp::grant::{DEFAULT_TTL_SECS, Grant, WriteKind};
        let grant = Grant::new(
            &crate::mcp::profile::McpProfile::new("assistant"),
            "local",
            &[WriteKind::Rows],
            0,
            DEFAULT_TTL_SECS,
            120,
        )
        .unwrap()
        .with_bypass(&[Operation::Insert, Operation::Update, Operation::Drop]);
        assert_eq!(grant.bypass, [Operation::Insert, Operation::Update]);
        assert!(grant.bypasses(&[Operation::Insert, Operation::Update]));
        assert!(!grant.bypasses(&[Operation::Insert, Operation::Delete]));
        assert!(!grant.bypasses(&[]));
    }
}
