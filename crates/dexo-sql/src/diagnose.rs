//! Problems the editor underlines as you type: statements that do not parse, and tables
//! or columns the catalog does not have -- reported only when the catalog has loaded the
//! place they would be, never guessed.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, Ident, ObjectName, ObjectNamePart, Query, Statement, TableFactor, Visit, Visitor,
};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::statement::{first_keyword, split_statements_in};
use crate::{Diagnostic, Dialect};

/// What the catalog has loaded, lowercased: which schemas, the tables in each, and the
/// columns of the tables whose columns it knows.
#[derive(Clone, Debug, Default)]
pub struct KnownObjects {
    schemas: HashSet<String>,
    tables: HashMap<String, HashSet<String>>,
    columns: HashMap<(String, String), HashSet<String>>,
}

impl KnownObjects {
    pub fn add_table(&mut self, schema: &str, table: &str) {
        let schema = schema.to_lowercase();
        self.schemas.insert(schema.clone());
        self.tables
            .entry(table.to_lowercase())
            .or_default()
            .insert(schema);
    }

    pub fn add_column(&mut self, schema: &str, table: &str, column: &str) {
        self.columns
            .entry((schema.to_lowercase(), table.to_lowercase()))
            .or_default()
            .insert(column.to_lowercase());
    }

    fn has_table(&self, schema: Option<&str>, table: &str) -> bool {
        match (schema, self.tables.get(table)) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(schema), Some(schemas)) => schemas.contains(schema),
        }
    }

    /// The table's columns, when known: by its schema, or the only schema that has it.
    fn columns_of(&self, schema: Option<&str>, table: &str) -> Option<&HashSet<String>> {
        let schema = match schema {
            Some(schema) => schema.to_string(),
            None => {
                let schemas = self.tables.get(table)?;
                if schemas.len() != 1 {
                    return None;
                }
                schemas.iter().next()?.clone()
            }
        };
        self.columns
            .get(&(schema, table.to_string()))
            .filter(|columns| !columns.is_empty())
    }
}

/// The problems in `sql`. `known` is the catalog, when it has been loaded whole; without
/// it only parse errors are reported. A statement still being typed -- the one holding
/// `cursor`, a byte offset -- is not told it ends too soon.
pub fn diagnose(
    sql: &str,
    dialect: Dialect,
    known: Option<&KnownObjects>,
    cursor: usize,
) -> Vec<Diagnostic> {
    let spans = split_statements_in(sql, dialect);
    // Tables the document creates itself are known before the catalog hears of them.
    let mut created = HashSet::new();
    for span in &spans {
        let body = &sql[span.byte_range.clone()];
        if first_keyword(body).as_deref() == Some("CREATE")
            && let Ok(statements) = parse(body, dialect)
        {
            for statement in statements {
                if let Statement::CreateTable(create) = &statement {
                    created.extend(last_part(&create.name));
                } else if let Statement::CreateView(view) = &statement {
                    created.extend(last_part(&view.name));
                }
            }
        }
    }
    let mut found = Vec::new();
    for span in &spans {
        let body = &sql[span.byte_range.clone()];
        let checked = matches!(
            first_keyword(body).as_deref(),
            Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES")
        );
        if !checked {
            continue;
        }
        let start = span.byte_range.start;
        match parse(body, dialect) {
            Err(error) => {
                let typing = cursor >= start && cursor <= span.byte_range.end + 1;
                let message = error.to_string();
                if typing && message.contains("found: EOF") {
                    continue;
                }
                let at = location(&message)
                    .and_then(|(line, column)| offset(body, line, column))
                    .unwrap_or(0);
                let end = token_end(body, at);
                found.push(Diagnostic::local(clean(&message), start + at..start + end));
            }
            Ok(statements) => {
                let Some(known) = known else {
                    continue;
                };
                let mut refs = References::default();
                for statement in &statements {
                    let _ = statement.visit(&mut refs);
                }
                check(&refs, known, &created, body, start, &mut found);
            }
        }
    }
    found
}

fn parse(sql: &str, dialect: Dialect) -> Result<Vec<Statement>, sqlparser::parser::ParserError> {
    match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
    }
}

#[derive(Default)]
struct References {
    ctes: HashSet<String>,
    /// Each table read, with its alias.
    tables: Vec<(Vec<Ident>, Option<String>)>,
    /// `qualifier.column`.
    columns: Vec<(Ident, Ident)>,
}

impl Visitor for References {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                self.ctes.insert(cte.alias.name.value.to_lowercase());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        // A name with arguments is a table function (`generate_series(1, 3)`), not a
        // table the catalog would list.
        if let TableFactor::Table {
            name,
            alias,
            args: None,
            ..
        } = factor
        {
            let parts = name
                .0
                .iter()
                .filter_map(|part| match part {
                    ObjectNamePart::Identifier(ident) => Some(ident.clone()),
                    ObjectNamePart::Function(_) => None,
                })
                .collect();
            let alias = alias.as_ref().map(|alias| alias.name.value.to_lowercase());
            self.tables.push((parts, alias));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if let Expr::CompoundIdentifier(parts) = expr
            && let [qualifier, column] = parts.as_slice()
        {
            self.columns.push((qualifier.clone(), column.clone()));
        }
        ControlFlow::Continue(())
    }
}

fn check(
    refs: &References,
    known: &KnownObjects,
    created: &HashSet<String>,
    body: &str,
    start: usize,
    found: &mut Vec<Diagnostic>,
) {
    let mut aliases: HashMap<String, (Option<String>, String)> = HashMap::new();
    for (parts, alias) in &refs.tables {
        let names: Vec<String> = parts
            .iter()
            .map(|ident| ident.value.to_lowercase())
            .collect();
        let (schema, table) = match names.as_slice() {
            [table] => (None, table.clone()),
            [schema, table] | [_, schema, table] => (Some(schema.clone()), table.clone()),
            _ => continue,
        };
        aliases.insert(table.clone(), (schema.clone(), table.clone()));
        if let Some(alias) = alias {
            aliases.insert(alias.clone(), (schema.clone(), table.clone()));
        }
        let unknown = match &schema {
            None => {
                !refs.ctes.contains(&table)
                    && !created.contains(&table)
                    && !system_name(&table)
                    && !known.has_table(None, &table)
            }
            // A schema the catalog never listed may be a system one; nothing is said.
            Some(schema) => {
                known.schemas.contains(schema)
                    && !created.contains(&table)
                    && !known.has_table(Some(schema), &table)
            }
        };
        if unknown
            && let (Some(first), Some(last)) = (parts.first(), parts.last())
            && let Some(range) = span_of(body, first, last)
        {
            let shown: Vec<&str> = parts.iter().map(|ident| ident.value.as_str()).collect();
            found.push(Diagnostic::local(
                format!("unknown table {}", shown.join(".")),
                start + range.start..start + range.end,
            ));
        }
    }
    for (qualifier, column) in &refs.columns {
        let Some((schema, table)) = aliases.get(&qualifier.value.to_lowercase()) else {
            continue;
        };
        let Some(columns) = known.columns_of(schema.as_deref(), table) else {
            continue;
        };
        if !columns.contains(&column.value.to_lowercase())
            && let Some(range) = span_of(body, column, column)
        {
            found.push(Diagnostic::local(
                format!("unknown column {} in {table}", column.value),
                start + range.start..start + range.end,
            ));
        }
    }
}

/// Names a database answers for without listing them as the user's tables.
fn system_name(table: &str) -> bool {
    table.starts_with("pg_")
        || table.starts_with("sqlite_")
        || table == "dual"
        || table == "information_schema"
}

fn last_part(name: &ObjectName) -> Option<String> {
    match name.0.last()? {
        ObjectNamePart::Identifier(ident) => Some(ident.value.to_lowercase()),
        ObjectNamePart::Function(_) => None,
    }
}

/// Byte offsets of `first`'s start to `last`'s end in `body`, from sqlparser's spans.
fn span_of(body: &str, first: &Ident, last: &Ident) -> Option<std::ops::Range<usize>> {
    let start = offset(body, first.span.start.line, first.span.start.column)?;
    let end = offset(body, last.span.end.line, last.span.end.column)?;
    (end > start).then_some(start..end)
}

/// sqlparser's 1-based line and column (in chars) as a byte offset into `body`.
fn offset(body: &str, line: u64, column: u64) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut at = 0;
    for (index, text) in body.split('\n').enumerate() {
        if index as u64 + 1 == line {
            let within: usize = text
                .chars()
                .take(column as usize - 1)
                .map(char::len_utf8)
                .sum();
            return Some(at + within);
        }
        at += text.len() + 1;
    }
    None
}

/// `Line: 3, Column: 14` out of sqlparser's message.
fn location(message: &str) -> Option<(u64, u64)> {
    let rest = &message[message.rfind("Line: ")? + 6..];
    let (line, rest) = rest.split_once(", Column: ")?;
    let column: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((line.trim().parse().ok()?, column.parse().ok()?))
}

/// The message without sqlparser's prefix and position, which the underline shows.
fn clean(message: &str) -> String {
    let message = message
        .strip_prefix("sql parser error: ")
        .unwrap_or(message);
    match message.rfind(" at Line: ") {
        Some(at) => message[..at].to_string(),
        None => message.to_string(),
    }
}

/// From `at` to the end of the word or symbol there: what the underline covers.
fn token_end(body: &str, at: usize) -> usize {
    let rest = &body[at.min(body.len())..];
    let width = rest
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(rest.len(), |(index, _)| index);
    at + width.max(rest.chars().next().map_or(0, char::len_utf8))
}

#[cfg(test)]
mod tests {
    use super::{KnownObjects, diagnose};
    use crate::Dialect;

    fn known() -> KnownObjects {
        let mut known = KnownObjects::default();
        known.add_table("public", "orders");
        known.add_table("public", "customers");
        for column in ["id", "customer_id", "total"] {
            known.add_column("public", "orders", column);
        }
        known
    }

    fn messages(sql: &str, known: Option<&KnownObjects>) -> Vec<(String, String)> {
        diagnose(sql, Dialect::Postgres, known, usize::MAX)
            .into_iter()
            .map(|diagnostic| {
                let range = diagnostic.byte_range.unwrap();
                (diagnostic.message, sql[range].to_string())
            })
            .collect()
    }

    #[test]
    fn parse_errors_point_at_their_token() {
        let found = messages("select * from orders;\nselect * frm orders;", None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1, "orders");
        // Still being typed: no complaint that it ends too soon.
        assert!(diagnose("select * from", Dialect::Postgres, None, 13).is_empty());
        // Not a statement kind the parser is trusted with.
        assert!(
            messages(
                "create function f() returns int as $$ select 1 $$ language sql",
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn unknown_tables_and_columns_only_where_the_catalog_knows() {
        let known = known();
        let found = messages(
            "select o.total, o.nope from orders o join ghosts g on true",
            Some(&known),
        );
        let found: Vec<String> = found.into_iter().map(|(message, _)| message).collect();
        assert_eq!(
            found,
            ["unknown table ghosts", "unknown column nope in orders"]
        );
        for fine in [
            "with recent as (select 1) select * from recent",
            "select * from generate_series(1, 3)",
            "select * from pg_tables",
            "select * from other_schema.anything",
            "create table fresh (id int); select * from fresh",
            "select c.whatever from customers c",
        ] {
            assert!(messages(fine, Some(&known)).is_empty(), "{fine}");
        }
        assert!(messages("select * from ghosts", None).is_empty());
    }
}
