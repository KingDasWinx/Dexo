use std::ops::ControlFlow;

use sqlparser::ast::{Expr, ObjectName, ObjectNamePart, Query, Select, Statement, Visit, Visitor};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::Dialect;
use crate::statement::{first_keyword, mysql_mask};

/// What a statement touches, as identifier paths such as `["db", "public", "orders"]`.
/// Unquoted Postgres identifiers are folded to lowercase, as the server folds them, so
/// the allowlist compares the name the server will actually resolve. SQLite ignores ASCII
/// case in every name, quoted or not, so all of its are folded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inspection {
    pub relations: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum GuardRejection {
    #[error("statement could not be parsed: {0}")]
    Unparsed(String),
    #[error("exactly one statement is required")]
    NotSingleStatement,
    #[error("statement is not allowed for this tool")]
    WrongKind,
    #[error("EXPLAIN ANALYZE executes the statement")]
    ExplainAnalyze,
    #[error("locking clauses (FOR UPDATE/SHARE) are not reads")]
    LockingClause,
    #[error("SELECT INTO creates a table")]
    SelectInto,
    #[error("nested data-modifying statements are not allowed")]
    NestedStatement,
    #[error("function {0} is not allowed")]
    Function(String),
}

/// Functions with side effects, or that run SQL passed as text and so would slip past the
/// relation allowlist. The read-only transaction is the real guarantee; this list turns
/// the known escapes into a clear refusal before anything reaches the server.
const DENIED_FUNCTIONS: &[&str] = &[
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_promote",
    "pg_switch_wal",
    "pg_create_restore_point",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_ls_logdir",
    "pg_ls_waldir",
    "pg_stat_file",
    "lo_import",
    "lo_export",
    "lo_unlink",
    "lo_create",
    "lo_from_bytea",
    "set_config",
    "pg_advisory_lock",
    "pg_advisory_xact_lock",
    "pg_advisory_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_xact_lock",
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
    "nextval",
    "setval",
    "pg_notify",
    "dblink",
    "dblink_exec",
    "dblink_connect",
    "dblink_send_query",
    "query_to_xml",
    "query_to_xml_and_xmlschema",
    "query_to_xmlschema",
    "cursor_to_xml",
    "table_to_xml",
    "table_to_xml_and_xmlschema",
    "schema_to_xml",
    "database_to_xml",
    "ts_stat",
    "ts_rewrite",
    "sleep",
    "benchmark",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "load_file",
    "sys_exec",
    "sys_eval",
];

/// A single `SELECT`/`VALUES`/`TABLE`/`WITH … SELECT`, or a plain `EXPLAIN` of one.
pub fn inspect_read(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    let query = match &statement {
        Statement::Query(query) => query.as_ref(),
        Statement::Explain {
            analyze,
            statement,
            options,
            ..
        } => {
            // Postgres also takes the British spelling, and runs the statement either way.
            let analyze_option = options.iter().flatten().any(|option| {
                (option.name.value.eq_ignore_ascii_case("analyze")
                    || option.name.value.eq_ignore_ascii_case("analyse"))
                    && !matches!(&option.arg, Some(Expr::Value(value)) if value.to_string().eq_ignore_ascii_case("false"))
            });
            if *analyze || analyze_option {
                return Err(GuardRejection::ExplainAnalyze);
            }
            match statement.as_ref() {
                Statement::Query(query) => query.as_ref(),
                _ => return Err(GuardRejection::WrongKind),
            }
        }
        _ => return Err(GuardRejection::WrongKind),
    };
    let mut guard = Guard::new(dialect, 0);
    let _ = query.visit(&mut guard);
    guard.finish(Vec::new())
}

/// A single top-level `INSERT`, `UPDATE` or `DELETE`. Every table it reads or writes is
/// returned, so the caller can hold all of them to the grant.
pub fn inspect_data_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    if !matches!(
        statement,
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_)
    ) {
        return Err(GuardRejection::WrongKind);
    }
    let mut guard = Guard::new(dialect, 1);
    let _ = statement.visit(&mut guard);
    guard.finish(Vec::new())
}

/// A single `CREATE TABLE`, `ALTER TABLE`, `DROP`, `CREATE INDEX` or `CREATE VIEW`.
/// sqlparser does not visit `DROP` names or a view's own name as relations, so those are
/// added here explicitly.
pub fn inspect_schema_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    let named: Vec<ObjectName> = match &statement {
        Statement::CreateTable(_) | Statement::AlterTable(_) | Statement::CreateIndex(_) => {
            Vec::new()
        }
        Statement::Drop { names, .. } => names.clone(),
        Statement::CreateView(view) => vec![view.name.clone()],
        _ => return Err(GuardRejection::WrongKind),
    };
    let mut guard = Guard::new(dialect, 1);
    let _ = statement.visit(&mut guard);
    let extra = named.iter().map(|name| guard.path(name)).collect();
    guard.finish(extra)
}

/// Why the editor asks before it runs a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Destructive {
    DeleteWithoutWhere,
    UpdateWithoutWhere,
    Drop,
    Truncate,
    AlterDrop,
    /// Neither parsed nor plainly a read, so what it does cannot be told.
    Unrecognized,
}

impl Destructive {
    pub fn describe(self) -> &'static str {
        match self {
            Self::DeleteWithoutWhere => "DELETE without WHERE removes every row",
            Self::UpdateWithoutWhere => "UPDATE without WHERE changes every row",
            Self::Drop => "DROP removes the object and what it holds",
            Self::Truncate => "TRUNCATE removes every row",
            Self::AlterDrop => "ALTER ... DROP removes part of the table",
            Self::Unrecognized => "Dexo could not read this statement",
        }
    }
}

/// Whether one statement only reads. SHOW and DESCRIBE count, and so does a plain
/// EXPLAIN, which never runs what it explains. A read that writes on the side --
/// `SELECT INTO`, `FOR UPDATE`, `set_config()`, a data-modifying CTE -- does not, nor
/// does anything sqlparser cannot parse.
pub fn is_read(sql: &str, dialect: Dialect) -> bool {
    // Dexo answers these from the catalog; nothing reaches the server.
    if crate::statement::is_backslash_command(sql) {
        return true;
    }
    let keyword = keyword_in(sql, dialect);
    if dialect == Dialect::Sqlite && keyword.as_deref() == Some("PRAGMA") {
        return pragma_reads(sql);
    }
    let shows = matches!(keyword.as_deref(), Some("SHOW" | "DESCRIBE" | "DESC"));
    match inspect_read(sql, dialect) {
        Ok(_) => true,
        // Parsed as exactly one statement, and EXPLAIN ANALYZE was already refused.
        Err(GuardRejection::WrongKind) => shows || keyword.as_deref() == Some("EXPLAIN"),
        // A SHOW, or a `TABLE t` (which sqlparser does not parse), is taken at its word
        // only when nothing follows it: the splitter ignores MySQL backslash escapes,
        // so `SHOW ... 'o\'%'; DELETE ...` used to arrive here as one span.
        Err(GuardRejection::Unparsed(_)) => {
            (shows || keyword.as_deref() == Some("TABLE"))
                && !sql.trim().trim_end_matches(';').contains(';')
        }
        Err(_) => false,
    }
}

/// What makes one statement destructive, if anything does. A statement that neither
/// parses nor is plainly a read is `Unrecognized`: unknown counts against it.
pub fn destructive(sql: &str, dialect: Dialect) -> Option<Destructive> {
    match keyword_in(sql, dialect).as_deref() {
        Some("DROP") => return Some(Destructive::Drop),
        Some("TRUNCATE") => return Some(Destructive::Truncate),
        // Maintenance sqlparser cannot parse: still a write, so production asks for
        // the name, but it destroys nothing and used to prompt as unreadable.
        Some(
            "REFRESH" | "CLUSTER" | "REINDEX" | "CHECKPOINT" | "VACUUM" | "ANALYZE" | "OPTIMIZE",
        ) => {
            return None;
        }
        // SQLite's session statements: writes, so production asks and read-only refuses,
        // but they destroy nothing and are not unreadable.
        Some("ATTACH" | "DETACH" | "PRAGMA") if dialect == Dialect::Sqlite => return None,
        _ => {}
    }
    let Ok(statement) = parse_one(sql, dialect) else {
        return (!is_read(sql, dialect)).then_some(Destructive::Unrecognized);
    };
    let mut finder = DestructiveFinder::default();
    let _ = statement.visit(&mut finder);
    finder.found
}

/// Whether the text of the workbench's WHERE and ORDER BY bars only reads, checked as
/// the statement they become: `SELECT * FROM t WHERE <where> ORDER BY <order>`. One
/// statement and no more, so a `;` is refused outright, and the side-effecting
/// functions `inspect_read` refuses are refused here too.
pub fn clauses_read(clauses: &dexo_driver_api::RawClauses, dialect: Dialect) -> Result<(), String> {
    let where_sql = clauses.where_sql.as_deref().map(str::trim).unwrap_or("");
    let order = clauses.order().unwrap_or("");
    if where_sql.contains(';') || order.contains(';') {
        return Err("a clause is one part of one statement: no `;`".into());
    }
    // What MySQL and MariaDB run from inside a comment has no place in a clause, and
    // `/*M! … */` read as a comment let a write past this check.
    if [where_sql, order]
        .iter()
        .any(|text| executable_comment(text))
    {
        return Err("a clause cannot hold an executable comment (`/*! … */`, `/*M! … */`)".into());
    }
    let mut probe = "SELECT * FROM _dexo_clauses".to_string();
    if !where_sql.is_empty() {
        probe.push_str(&format!(" WHERE ({where_sql})"));
    }
    if !order.is_empty() {
        probe.push_str(&format!(" ORDER BY {order}"));
    }
    match inspect_read(&probe, dialect) {
        Ok(_) => Ok(()),
        Err(GuardRejection::Unparsed(reason)) => Err(format!("not valid SQL: {reason}")),
        Err(rejection) => Err(format!("not a read: {rejection:?}")),
    }
}

/// Whether a SQLite PRAGMA only reads. `PRAGMA name = value` sets, and so does
/// `PRAGMA journal_mode(WAL)`: the parenthesised form reads only for the pragmas that
/// take an argument to look at (`table_info(t)`). Bare, a pragma reports its value --
/// except the few that act when named.
fn pragma_reads(sql: &str) -> bool {
    const LOOKS_AT: &[&str] = &[
        "table_info",
        "table_xinfo",
        "table_list",
        "index_list",
        "index_info",
        "index_xinfo",
        "foreign_key_list",
        "foreign_key_check",
        "integrity_check",
        "quick_check",
    ];
    const ACTS: &[&str] = &[
        "optimize",
        "incremental_vacuum",
        "wal_checkpoint",
        "shrink_memory",
    ];
    use sqlparser::tokenizer::{Token, Whitespace};
    // Comments read as part of the pragma: a trailing one hid `optimize`'s name, a
    // leading one the word PRAGMA.
    let Ok(tokens) =
        sqlparser::tokenizer::Tokenizer::new(&sqlparser::dialect::SQLiteDialect {}, sql).tokenize()
    else {
        return false;
    };
    let uncommented: String = tokens
        .iter()
        .filter(|token| {
            !matches!(
                token,
                Token::Whitespace(
                    Whitespace::SingleLineComment { .. } | Whitespace::MultiLineComment(_)
                )
            )
        })
        // A name means itself quoted too -- `"optimize"`, `[optimize]`, `'optimize'` --
        // and kept its quotes here, so it never matched.
        .map(|token| match token {
            Token::Word(word) => word.value.clone(),
            Token::SingleQuotedString(text) => text.clone(),
            other => other.to_string(),
        })
        .collect();
    let body = uncommented.trim().trim_end_matches(';').trim_end();
    let Some(rest) = body
        .get(6..)
        .filter(|_| body[..6].eq_ignore_ascii_case("pragma"))
    else {
        return false;
    };
    if rest.contains(['=', ';']) {
        return false;
    }
    let (name, argument) = match rest.split_once('(') {
        Some((name, argument)) => (name, Some(argument)),
        None => (rest, None),
    };
    let name = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match argument {
        Some(_) => LOOKS_AT.contains(&name.as_str()),
        None => !ACTS.contains(&name.as_str()),
    }
}

/// The first keyword past the comments `dialect` has: MySQL's `#` too.
fn keyword_in(sql: &str, dialect: Dialect) -> Option<String> {
    match dialect {
        Dialect::Postgres | Dialect::Sqlite => first_keyword(sql),
        Dialect::Mysql => first_keyword(&mysql_mask(sql)),
    }
}

/// Visits nested statements too, so a `DELETE` inside a CTE is found.
#[derive(Default)]
struct DestructiveFinder {
    found: Option<Destructive>,
}

impl Visitor for DestructiveFinder {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<()> {
        self.found = match statement {
            // Also found by their first keyword, but a MySQL `#` comment hides that.
            Statement::Drop { .. } => Some(Destructive::Drop),
            Statement::Truncate(_) => Some(Destructive::Truncate),
            Statement::Delete(delete) if delete.selection.is_none() => {
                Some(Destructive::DeleteWithoutWhere)
            }
            Statement::Update(update) if update.selection.is_none() => {
                Some(Destructive::UpdateWithoutWhere)
            }
            // Every drop operation displays as `DROP ...`; matching the text covers
            // columns, constraints, keys, indexes and partitions alike.
            Statement::AlterTable(alter)
                if alter
                    .operations
                    .iter()
                    .any(|operation| operation.to_string().starts_with("DROP")) =>
            {
                Some(Destructive::AlterDrop)
            }
            _ => return ControlFlow::Continue(()),
        };
        ControlFlow::Break(())
    }
}

/// Whether `text` holds a comment MySQL or MariaDB runs: `/*!`, `/*!50000`, `/*M!` or
/// `/*M!100100`, in any case.
fn executable_comment(text: &str) -> bool {
    text.contains("/*!") || text.to_ascii_lowercase().contains("/*m!")
}

fn parse_one(sql: &str, dialect: Dialect) -> Result<Statement, GuardRejection> {
    let mut statements = match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        // sqlparser reads MySQL's `/*! … */` as code, as the server runs it, but takes
        // MariaDB's `/*M! … */` for a comment. Written the MySQL way -- same length, so
        // positions in errors hold -- it is read as the code MariaDB runs.
        Dialect::Mysql if executable_comment(sql) => Parser::parse_sql(
            &MySqlDialect {},
            &sql.replace("/*M!", " /*!").replace("/*m!", " /*!"),
        ),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
    }
    .map_err(|error| GuardRejection::Unparsed(error.to_string()))?;
    if statements.len() != 1 {
        return Err(GuardRejection::NotSingleStatement);
    }
    Ok(statements.remove(0))
}

struct Guard {
    dialect: Dialect,
    statements_allowed: usize,
    statements_seen: usize,
    ctes: Vec<String>,
    relations: Vec<Vec<String>>,
    rejection: Option<GuardRejection>,
}

impl Guard {
    fn new(dialect: Dialect, statements_allowed: usize) -> Self {
        Self {
            dialect,
            statements_allowed,
            statements_seen: 0,
            ctes: Vec::new(),
            relations: Vec::new(),
            rejection: None,
        }
    }

    fn reject(&mut self, rejection: GuardRejection) -> ControlFlow<()> {
        self.rejection = Some(rejection);
        ControlFlow::Break(())
    }

    fn path(&self, name: &ObjectName) -> Vec<String> {
        name.0
            .iter()
            .map(|part| match part {
                ObjectNamePart::Identifier(ident)
                    if ident.quote_style.is_none() && self.dialect == Dialect::Postgres =>
                {
                    ident.value.to_lowercase()
                }
                ObjectNamePart::Identifier(ident) if self.dialect == Dialect::Sqlite => {
                    ident.value.to_ascii_lowercase()
                }
                ObjectNamePart::Identifier(ident) => ident.value.clone(),
                ObjectNamePart::Function(function) => function.name.value.clone(),
            })
            .collect()
    }

    fn finish(self, extra: Vec<Vec<String>>) -> Result<Inspection, GuardRejection> {
        if let Some(rejection) = self.rejection {
            return Err(rejection);
        }
        let ctes = self.ctes;
        let mut relations: Vec<Vec<String>> = self
            .relations
            .into_iter()
            .chain(extra)
            .filter(|path| !(path.len() == 1 && ctes.contains(&path[0])))
            .collect();
        relations.sort();
        relations.dedup();
        Ok(Inspection { relations })
    }
}

impl Visitor for Guard {
    type Break = ();

    fn pre_visit_statement(&mut self, _statement: &Statement) -> ControlFlow<()> {
        self.statements_seen += 1;
        if self.statements_seen > self.statements_allowed {
            return self.reject(GuardRejection::NestedStatement);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if !query.locks.is_empty() {
            return self.reject(GuardRejection::LockingClause);
        }
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                let name = self.path(&ObjectName::from(vec![cte.alias.name.clone()]));
                self.ctes.extend(name);
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        if select.into.is_some() {
            return self.reject(GuardRejection::SelectInto);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        let path = self.path(relation);
        self.relations.push(path);
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if let Expr::Function(function) = expr {
            let name = self
                .path(&function.name)
                .last()
                .map(|name| name.to_lowercase())
                .unwrap_or_default();
            if DENIED_FUNCTIONS.contains(&name.as_str()) {
                return self.reject(GuardRejection::Function(name));
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Destructive, GuardRejection, destructive, inspect_data_write, inspect_read,
        inspect_schema_write, is_read,
    };
    use crate::Dialect;

    /// The bars' text runs only as a read: a write, a second statement or a
    /// side-effecting function in it is refused before anything is sent.
    #[test]
    fn bar_clauses_only_read() {
        let check = |where_sql: &str, order: &str| {
            super::clauses_read(
                &dexo_driver_api::RawClauses {
                    where_sql: Some(where_sql.into()),
                    order_by: Some(order.into()),
                },
                Dialect::Postgres,
            )
        };
        assert!(check("total > 10 and name ilike 'a%'", "total desc, id").is_ok());
        assert!(check("id in (select id from t)", "").is_ok());
        assert!(check("1=1; delete from t", "").is_err());
        assert!(check("", "id; drop table t").is_err());
        assert!(check("pg_terminate_backend(42)", "").is_err());
        assert!(check("id = (", "").is_err());
    }

    /// MariaDB runs `/*M! … */` as MySQL runs `/*! … */`: in a bar's text either is
    /// refused, whatever its case, version or spacing, and on MySQL a statement holding
    /// one is read as the code inside it.
    #[test]
    fn executable_comments_are_code() {
        let check = |where_sql: &str, order: &str, dialect| {
            super::clauses_read(
                &dexo_driver_api::RawClauses {
                    where_sql: Some(where_sql.into()),
                    order_by: Some(order.into()),
                },
                dialect,
            )
        };
        for dialect in [Dialect::Mysql, Dialect::Postgres, Dialect::Sqlite] {
            for order in [
                "id /*M! LIMIT 1 INTO OUTFILE '/tmp/x' */ -- x",
                "id /*M!100100 LIMIT 1 INTO OUTFILE '/tmp/x' */",
                "id /*m! , sleep(5) */",
                "id /*M!\n  , sleep(5)\n*/",
                "id /*M!/* x */ , sleep(5) */",
                "id /*!50000 , sleep(5) */",
                "id /*! , sleep(5) */",
            ] {
                assert!(check("", order, dialect).is_err(), "{order}");
                assert!(check(order, "", dialect).is_err(), "{order}");
            }
            // A comment that only says something is still fine.
            assert!(check("id > 1 /* M! not code */", "id /* ! */", dialect).is_ok());
        }
        let mysql = |sql: &str| is_read(sql, Dialect::Mysql);
        assert!(!mysql("select 1 /*M! into outfile '/tmp/x' */"));
        assert!(!mysql("select 1 /*M!100100 , sleep(5) */"));
        assert!(!mysql("select 1 /*m!, sleep(5) */"));
        assert!(!mysql("/*M! delete from t */"));
        assert!(!mysql("/*M!100100 delete from t */"));
        assert!(mysql("select 1 /* M! a plain comment */"));
        assert_eq!(
            destructive("/*M! delete from t */", Dialect::Mysql),
            Some(Destructive::DeleteWithoutWhere)
        );
    }

    /// A PRAGMA that only reports is a read; one that sets or acts is a write, and
    /// none of them is destructive.
    #[test]
    fn sqlite_pragmas_read_unless_they_set_or_act() {
        let sqlite = |sql: &str| is_read(sql, Dialect::Sqlite);
        assert!(sqlite("pragma table_info(t)"));
        assert!(sqlite("PRAGMA main.foreign_key_list('orders');"));
        assert!(sqlite("pragma journal_mode"));
        assert!(!sqlite("pragma journal_mode = wal"));
        assert!(!sqlite("pragma journal_mode(WAL)"));
        assert!(!sqlite("pragma optimize"));
        assert!(!sqlite("pragma user_version; delete from t"));
        // Comments are not part of the pragma.
        assert!(!sqlite("pragma optimize -- tidy up"));
        assert!(!sqlite("pragma /* x */ optimize"));
        assert!(sqlite("-- columns\npragma table_info(t)"));
        assert!(sqlite("/* a */ pragma journal_mode -- b"));
        // A quoted name is the same name.
        for quoted in [
            "pragma \"incremental_vacuum\"",
            "pragma [optimize]",
            "pragma `optimize`",
            "pragma 'optimize'",
            "pragma main.\"optimize\"",
            "pragma \"journal_mode\" = wal",
            "pragma [journal_mode](wal)",
        ] {
            assert!(!sqlite(quoted), "{quoted}");
        }
        assert!(sqlite("pragma \"table_info\"('orders')"));
        assert!(sqlite("pragma [journal_mode]"));
        for sql in [
            "pragma user_version = 3",
            "detach database x",
            "attach 'a.db' as a",
        ] {
            assert_eq!(destructive(sql, Dialect::Sqlite), None, "{sql}");
            assert!(!sqlite(sql), "{sql}");
        }
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    fn reads(sql: &str) -> Vec<Vec<String>> {
        inspect_read(sql, Dialect::Postgres).unwrap().relations
    }

    fn refused(sql: &str) -> GuardRejection {
        inspect_read(sql, Dialect::Postgres).unwrap_err()
    }

    #[test]
    fn postgres_folds_unquoted_identifiers() {
        assert_eq!(reads("select id from Stores"), vec![path(&["stores"])]);
        assert_eq!(reads("select * from \"Mixed\""), vec![path(&["Mixed"])]);
    }

    #[test]
    fn every_joined_table_is_reported() {
        assert_eq!(
            reads("select count(*) from db1.public.stores s join db1.public.employees e on true"),
            vec![
                path(&["db1", "public", "employees"]),
                path(&["db1", "public", "stores"])
            ]
        );
        assert_eq!(reads("select * from stores s, employees e").len(), 2);
        assert_eq!(
            reads("select count(*) from\nemployees"),
            vec![path(&["employees"])]
        );
        assert_eq!(
            reads("select (select max(id) from employees) from stores").len(),
            2
        );
    }

    #[test]
    fn cte_names_are_not_tables_but_their_bodies_are() {
        assert_eq!(
            reads("with e as (select * from employees) select * from e"),
            vec![path(&["employees"])]
        );
        assert!(reads("select 1").is_empty());
        assert_eq!(
            reads("explain select * from stores"),
            vec![path(&["stores"])]
        );
    }

    #[test]
    fn reads_that_write_or_lock_are_refused() {
        assert_eq!(
            refused("select * into copy_of_stores from stores"),
            GuardRejection::SelectInto
        );
        assert_eq!(
            refused("select * from stores for update"),
            GuardRejection::LockingClause
        );
        assert_eq!(
            refused("select pg_terminate_backend(1) from stores"),
            GuardRejection::Function("pg_terminate_backend".into())
        );
        assert_eq!(
            refused("select query_to_xml('select * from employees', true, true, '')"),
            GuardRejection::Function("query_to_xml".into())
        );
        assert_eq!(
            refused("explain analyze delete from stores"),
            GuardRejection::ExplainAnalyze
        );
        assert_eq!(
            refused("explain (analyze) select 1"),
            GuardRejection::ExplainAnalyze
        );
        assert_eq!(
            refused("explain delete from stores"),
            GuardRejection::WrongKind
        );
        assert_eq!(
            refused("with x as (delete from stores returning *) select * from x"),
            GuardRejection::NestedStatement
        );
        assert_eq!(refused("delete from stores"), GuardRejection::WrongKind);
        assert_eq!(
            refused("select 1; select 2"),
            GuardRejection::NotSingleStatement
        );
        assert!(matches!(refused("selec 1"), GuardRejection::Unparsed(_)));
    }

    #[test]
    fn mysql_names_and_functions() {
        let read = |sql: &str| inspect_read(sql, Dialect::Mysql);
        assert_eq!(
            read("select * from shop.Items").unwrap().relations,
            vec![path(&["shop", "Items"])]
        );
        assert_eq!(
            read("select sleep(10)").unwrap_err(),
            GuardRejection::Function("sleep".into())
        );
        assert!(read("select * from items into outfile '/tmp/x'").is_err());
    }

    #[test]
    fn data_writes_report_every_table_they_touch() {
        let write = |sql: &str| inspect_data_write(sql, Dialect::Postgres);
        assert_eq!(
            write("insert into db1.public.stores (id) select id from employees")
                .unwrap()
                .relations,
            vec![path(&["db1", "public", "stores"]), path(&["employees"])]
        );
        assert_eq!(
            write("update stores set name = 'x' where id = 1")
                .unwrap()
                .relations,
            vec![path(&["stores"])]
        );
        assert_eq!(
            write("delete from stores using employees where stores.id = employees.id")
                .unwrap()
                .relations
                .len(),
            2
        );
        assert_eq!(
            write("with x as (select id from employees) delete from stores").unwrap_err(),
            GuardRejection::WrongKind
        );
        assert_eq!(
            write("drop table stores").unwrap_err(),
            GuardRejection::WrongKind
        );
        assert_eq!(
            write("update stores set id = nextval('s')").unwrap_err(),
            GuardRejection::Function("nextval".into())
        );
    }

    #[test]
    fn schema_writes_name_their_targets() {
        let ddl = |sql: &str| inspect_schema_write(sql, Dialect::Postgres);
        assert_eq!(
            ddl("drop table public.stores, public.employees")
                .unwrap()
                .relations,
            vec![path(&["public", "employees"]), path(&["public", "stores"])]
        );
        assert_eq!(
            ddl("create view v as select * from employees")
                .unwrap()
                .relations,
            vec![path(&["employees"]), path(&["v"])]
        );
        assert_eq!(
            ddl("create index idx on public.stores (name)")
                .unwrap()
                .relations,
            vec![path(&["public", "stores"])]
        );
        assert_eq!(
            ddl("create table t3 as select * from employees")
                .unwrap()
                .relations,
            vec![path(&["employees"]), path(&["t3"])]
        );
        assert_eq!(
            ddl("truncate public.stores").unwrap_err(),
            GuardRejection::WrongKind
        );
        assert_eq!(
            inspect_schema_write("drop table shop.items", Dialect::Mysql)
                .unwrap()
                .relations,
            vec![path(&["shop", "items"])]
        );
    }

    #[test]
    fn plain_reads_are_reads() {
        for sql in [
            "select 1",
            "with t as (select 1 as n) select n from t",
            "-- note\nselect * from items",
            "show tables",
            "SHOW search_path",
            "describe items",
            "explain select * from items",
        ] {
            assert!(is_read(sql, Dialect::Postgres), "{sql}");
        }
    }

    #[test]
    fn writes_hidden_in_reads_are_not_reads() {
        for sql in [
            "select * into backup from items",
            "select * from items for update",
            "select set_config('default_transaction_read_only', 'off', false)",
            "with d as (delete from items returning *) select * from d",
            "explain analyze delete from items",
            "set default_transaction_read_only = off",
            "begin",
            "insert into items values (1)",
            "not sql at all",
        ] {
            assert!(!is_read(sql, Dialect::Postgres), "{sql}");
        }
    }

    #[test]
    fn destructive_statements_say_why() {
        let pg = |sql: &str| destructive(sql, Dialect::Postgres);
        assert_eq!(
            pg("delete from items"),
            Some(Destructive::DeleteWithoutWhere)
        );
        assert_eq!(
            pg("DeLeTe from items"),
            Some(Destructive::DeleteWithoutWhere)
        );
        assert_eq!(
            pg("-- clean up\nDELETE FROM items"),
            Some(Destructive::DeleteWithoutWhere)
        );
        assert_eq!(
            pg("update items set n = 0"),
            Some(Destructive::UpdateWithoutWhere)
        );
        assert_eq!(
            pg("update items set n = (select max(n) from items where id = 1)"),
            Some(Destructive::UpdateWithoutWhere)
        );
        assert_eq!(pg("drop table items"), Some(Destructive::Drop));
        assert_eq!(pg("DROP INDEX items_n"), Some(Destructive::Drop));
        assert_eq!(pg("truncate items"), Some(Destructive::Truncate));
        assert_eq!(
            pg("alter table items drop column n"),
            Some(Destructive::AlterDrop)
        );
        assert_eq!(
            pg("alter table items drop constraint items_pkey"),
            Some(Destructive::AlterDrop)
        );
        assert!(pg("with d as (delete from items returning id) select * from d").is_some());
        assert_eq!(pg("frobnicate items"), Some(Destructive::Unrecognized));
        assert_eq!(
            destructive("delete from items limit 10", Dialect::Mysql),
            Some(Destructive::DeleteWithoutWhere)
        );
    }

    #[test]
    fn ordinary_writes_and_reads_are_not_destructive() {
        for sql in [
            "delete from items where id = 1",
            "update items set n = 0 where id in (select id from items)",
            "insert into items values (1, 2)",
            "alter table items add column m int",
            "alter table items alter column n drop not null",
            "create table t (id int)",
            "select 1",
            "show tables",
        ] {
            assert_eq!(destructive(sql, Dialect::Postgres), None, "{sql}");
        }
    }

    /// Postgres takes the British spelling in the option list and runs the statement.
    #[test]
    fn explain_analyse_is_not_a_read() {
        for sql in [
            "explain (analyse) delete from items",
            "explain analyse delete from items",
            "EXPLAIN (ANALYSE true) DELETE FROM items",
            "explain (analyze, format json) delete from items",
        ] {
            assert!(!is_read(sql, Dialect::Postgres), "{sql}");
            assert!(inspect_read(sql, Dialect::Postgres).is_err(), "{sql}");
        }
    }

    /// A MySQL `#` comment hides the first keyword from the splitter's view, so DROP
    /// and TRUNCATE are found in the parsed statement too.
    #[test]
    fn drop_and_truncate_are_found_after_a_mysql_hash_comment() {
        let mysql = |sql: &str| destructive(sql, Dialect::Mysql);
        assert_eq!(
            mysql("# drop the customer's old table\nDROP TABLE customers_old"),
            Some(Destructive::Drop)
        );
        assert_eq!(
            mysql("# empty it\nTRUNCATE TABLE customers_old"),
            Some(Destructive::Truncate)
        );
    }

    /// SHOW and DESCRIBE are reads, but not when the span carries a second statement:
    /// the splitter ignores MySQL backslash escapes, so this reaches the guard whole.
    #[test]
    fn a_show_that_carries_a_second_statement_is_not_a_read() {
        assert!(!is_read(
            "SHOW TABLES LIKE 'o\\'%'; DELETE FROM orders",
            Dialect::Mysql
        ));
        assert!(!is_read(
            "show tables; delete from orders",
            Dialect::Postgres
        ));
        assert!(is_read("SHOW TABLES LIKE 'o\\'%'", Dialect::Mysql));
    }

    /// A `#` comment before a MySQL statement is a comment, not a statement Dexo cannot
    /// read.
    #[test]
    fn a_mysql_hash_comment_before_a_read_is_still_a_read() {
        assert!(is_read("# list them\nSHOW TABLES", Dialect::Mysql));
        assert!(is_read(
            "# count them\nselect count(*) from orders",
            Dialect::Mysql
        ));
    }

    /// `TABLE t` is a read. Maintenance Dexo cannot parse still counts as a write, so
    /// production asks for the name, but it destroys nothing, so elsewhere it runs.
    #[test]
    fn table_reads_and_maintenance_is_not_destructive() {
        assert!(is_read("TABLE items", Dialect::Postgres));
        for sql in [
            "refresh materialized view sales",
            "cluster items",
            "reindex table items",
            "checkpoint",
            "vacuum items",
            "analyze items",
        ] {
            assert!(!is_read(sql, Dialect::Postgres), "{sql}");
            assert_eq!(destructive(sql, Dialect::Postgres), None, "{sql}");
        }
        assert_eq!(destructive("optimize table items", Dialect::Mysql), None);
        assert_eq!(destructive("analyze table items", Dialect::Mysql), None);
    }

    /// On a read-only SQLite file the server refuses writes too; these are what the
    /// editor must not send it as reads: a PRAGMA that lifts `query_only`, an ATTACH of
    /// another file, a VACUUM INTO that writes one.
    #[test]
    fn sqlite_reads_writes_and_destructive_statements() {
        let sqlite = |sql: &str| is_read(sql, Dialect::Sqlite);
        for sql in [
            "select * from items",
            "with t as (select 1 as n) select n from t",
            "explain query plan select * from items where id = 1",
        ] {
            assert!(sqlite(sql), "{sql}");
        }
        for sql in [
            "pragma query_only = 0",
            "attach database 'other.db' as other",
            "vacuum into '/tmp/copy.db'",
            "insert or replace into items values (1)",
            "begin immediate",
        ] {
            assert!(!sqlite(sql), "{sql}");
        }
        assert_eq!(
            destructive("delete from items", Dialect::Sqlite),
            Some(Destructive::DeleteWithoutWhere)
        );
        assert_eq!(
            destructive("drop table items", Dialect::Sqlite),
            Some(Destructive::Drop)
        );
        assert_eq!(
            destructive("delete from items where id = 1", Dialect::Sqlite),
            None
        );
        assert_eq!(
            inspect_read("select * from \"Items\"", Dialect::Sqlite)
                .unwrap()
                .relations,
            [["items"]]
        );
    }
}
