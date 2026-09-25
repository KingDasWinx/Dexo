use std::ops::ControlFlow;

use sqlparser::ast::{Expr, ObjectName, ObjectNamePart, Query, Select, Statement, Visit, Visitor};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;

use crate::Dialect;

/// What a statement touches, as identifier paths such as `["db", "public", "orders"]`.
/// Unquoted Postgres identifiers are folded to lowercase, as the server folds them, so
/// the allowlist compares the name the server will actually resolve.
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
            let analyze_option = options.iter().flatten().any(|option| {
                option.name.value.eq_ignore_ascii_case("analyze")
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

fn parse_one(sql: &str, dialect: Dialect) -> Result<Statement, GuardRejection> {
    let mut statements = match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
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
    use super::{GuardRejection, inspect_data_write, inspect_read, inspect_schema_write};
    use crate::Dialect;

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
}
