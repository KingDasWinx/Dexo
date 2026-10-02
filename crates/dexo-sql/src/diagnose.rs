//! Problems the editor underlines as you type: statements that do not parse, and tables
//! or columns the catalog does not have -- reported only when the catalog has loaded the
//! place they would be, never guessed.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, Ident, ObjectNamePart, Query, Statement, TableFactor, Visit, Visitor};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

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
    let created: HashSet<String> = spans
        .iter()
        .filter_map(|span| created_name(&sql[span.byte_range.clone()], dialect))
        .collect();
    let mut found = Vec::new();
    let mut grammar = None;
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
                let message = error.to_string();
                let located =
                    location(&message).and_then(|(line, column)| offset(body, line, column));
                // Ended too soon: the last word is where something more was wanted.
                let at = located.unwrap_or_else(|| last_word_start(body));
                let end = token_end(body, at);
                // The statement being typed is not told off for what is at or past the
                // cursor: that part is not written yet.
                let typing = cursor >= start && cursor <= span.byte_range.end + 1;
                if typing && start + end >= cursor {
                    continue;
                }
                if located.is_some() && !beyond_doubt(body, at, &mut grammar) {
                    continue;
                }
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

/// Whether a parse error at `at` is the SQL's and not the parser's. sqlparser knows only
/// part of each dialect, so an error is believed only when the highlighting grammar
/// fails the statement too and the statement uses none of the constructs sqlparser is
/// known to lack.
fn beyond_doubt(body: &str, at: usize, grammar: &mut Option<tree_sitter::Parser>) -> bool {
    // ponytail: a list of what sqlparser fails on in each dialect; extend it as more
    // valid SQL turns up underlined.
    const UNPARSED: &[&[&str]] = &[
        &["rows", "from"],
        &["symmetric"],
        &["asymmetric"],
        &["lock", "in"],
        &["share", "mode"],
        &["with", "rollup"],
        &["outfile"],
        &["dumpfile"],
        &["indexed", "by"],
        &["not", "indexed"],
        &["glob"],
    ];
    let words: Vec<String> = body
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let lacking = UNPARSED.iter().any(|construct| {
        words.windows(construct.len()).any(|run| {
            run.iter()
                .zip(construct.iter())
                .all(|(word, want)| word == want)
        })
    });
    // `ORDER BY id USING <`, `CHAR(65 USING utf8mb4)`.
    let at_using = body[at..]
        .get(..5)
        .is_some_and(|word| word.eq_ignore_ascii_case("using"));
    if lacking || at_using {
        return false;
    }
    let grammar = grammar.get_or_insert_with(|| {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_sequel::LANGUAGE.into())
            .expect("tree-sitter-sequel language");
        parser
    });
    grammar
        .parse(body, None)
        .is_none_or(|tree| tree.root_node().has_error())
}

fn is_keyword(word: &str) -> bool {
    !word.is_empty()
        && sqlparser::keywords::ALL_KEYWORDS
            .binary_search(&word.to_ascii_uppercase().as_str())
            .is_ok()
}

/// Where the last word of `body` starts.
fn last_word_start(body: &str) -> usize {
    let trimmed = body.trim_end();
    trimmed.rfind(char::is_whitespace).map_or(0, |at| {
        at + trimmed[at..].chars().next().map_or(1, char::len_utf8)
    })
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
    // One map for the whole statement, scopes and all: a name that stands for two
    // different tables somewhere in it -- `o` in a query and in its subquery -- stands
    // for neither, and its columns go unchecked.
    let mut aliases: HashMap<String, Option<(Option<String>, String)>> = HashMap::new();
    let mut name = |key: String, target: (Option<String>, String)| {
        aliases
            .entry(key)
            .and_modify(|known| {
                if known.as_ref() != Some(&target) {
                    *known = None;
                }
            })
            .or_insert(Some(target));
    };
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
        name(table.clone(), (schema.clone(), table.clone()));
        if let Some(alias) = alias {
            name(alias.clone(), (schema.clone(), table.clone()));
        }
        // `FROM ONLY t`, `UPDATE IGNORE t`: the parser took a keyword it does not know
        // there for the table.
        let keyword = parts.len() == 1 && parts[0].quote_style.is_none() && is_keyword(&table);
        let unknown = match &schema {
            None if keyword => false,
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
        let qualifier = qualifier.value.to_lowercase();
        // A CTE named like a table has the CTE's columns.
        if refs.ctes.contains(&qualifier) {
            continue;
        }
        let Some(Some((schema, table))) = aliases.get(&qualifier) else {
            continue;
        };
        let Some(columns) = known.columns_of(schema.as_deref(), table) else {
            continue;
        };
        let name = column.value.to_lowercase();
        if !columns.contains(&name)
            && !SYSTEM_COLUMNS.contains(&name.as_str())
            && let Some(range) = span_of(body, column, column)
        {
            found.push(Diagnostic::local(
                format!("unknown column {} in {table}", column.value),
                start + range.start..start + range.end,
            ));
        }
    }
}

/// Columns every row has without the table declaring them: SQLite's rowid and its
/// aliases, Postgres's system columns.
const SYSTEM_COLUMNS: &[&str] = &[
    "rowid", "oid", "_rowid_", "ctid", "xmin", "xmax", "cmin", "cmax", "tableoid",
];

/// Names a database answers for without listing them as the user's tables.
fn system_name(table: &str) -> bool {
    table.starts_with("pg_")
        || table.starts_with("sqlite_")
        || table == "dual"
        || table == "information_schema"
}

/// The table, view or sequence `body` creates, read from its words rather than parsed:
/// the parser misses `CREATE UNLOGGED TABLE`, `(LIKE ..)`, SQLite's virtual tables and
/// `SELECT .. INTO new_table`, and each left the new name underlined further down.
fn created_name(body: &str, dialect: Dialect) -> Option<String> {
    let tokens = match dialect {
        Dialect::Postgres => Tokenizer::new(&PostgreSqlDialect {}, body).tokenize(),
        Dialect::Mysql => Tokenizer::new(&MySqlDialect {}, body).tokenize(),
        Dialect::Sqlite => Tokenizer::new(&SQLiteDialect {}, body).tokenize(),
    }
    .ok()?;
    let mut tokens = tokens
        .into_iter()
        .filter(|token| !matches!(token, Token::Whitespace(_) | Token::EOF | Token::Period));
    let word = |token: &Token| match token {
        Token::Word(word) => Some((word.value.to_uppercase(), word.value.to_lowercase())),
        _ => None,
    };
    let (first, _) = word(&tokens.next()?)?;
    let names: Vec<(String, String)> = match first.as_str() {
        "CREATE" => tokens.map_while(|token| word(&token)).collect(),
        // `SELECT .. INTO name`: the words after the first INTO.
        "SELECT" => tokens
            .skip_while(|token| word(token).is_none_or(|(upper, _)| upper != "INTO"))
            .skip(1)
            .map_while(|token| word(&token))
            .collect(),
        _ => return None,
    };
    const MODIFIERS: &[&str] = &[
        "OR",
        "REPLACE",
        "GLOBAL",
        "LOCAL",
        "TEMP",
        "TEMPORARY",
        "UNLOGGED",
        "VIRTUAL",
        "MATERIALIZED",
        "RECURSIVE",
        "FOREIGN",
        "TABLE",
        "VIEW",
        "SEQUENCE",
        "IF",
        "NOT",
        "EXISTS",
    ];
    let mut words = names.into_iter().peekable();
    if first == "CREATE" {
        let mut creates = false;
        while let Some((upper, _)) = words.peek() {
            if !MODIFIERS.contains(&upper.as_str()) {
                break;
            }
            creates |= matches!(upper.as_str(), "TABLE" | "VIEW" | "SEQUENCE");
            words.next();
        }
        if !creates {
            return None;
        }
    }
    // A qualified name's dots were dropped: its words run on, and the table is the last
    // one before the next keyword -- `public.t (` stops at the `(`, `x.t AS` at AS.
    let mut name = None;
    for (upper, lower) in words {
        if name.is_some() && matches!(upper.as_str(), "AS" | "USING" | "FROM" | "LIKE") {
            break;
        }
        name = Some(lower);
    }
    name
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

    /// What sqlparser cannot parse but the dialect can is not underlined; typos are.
    #[test]
    fn parse_errors_are_the_sql_s_not_the_parser_s() {
        let in_dialect =
            |sql: &str, dialect: Dialect| diagnose(sql, dialect, Some(&known()), usize::MAX);
        for (dialect, fine) in [
            (Dialect::Postgres, "select * from only orders"),
            (
                Dialect::Postgres,
                "select * from rows from (generate_series(1,3), generate_series(1,4))",
            ),
            (
                Dialect::Postgres,
                "select * from orders where id between symmetric 1 and 2",
            ),
            (
                Dialect::Postgres,
                "select * from orders order by id using <",
            ),
            (Dialect::Mysql, "select * from orders lock in share mode"),
            (
                Dialect::Mysql,
                "select id, count(*) from orders group by id with rollup",
            ),
            (Dialect::Mysql, "select 5 div 2"),
            (Dialect::Mysql, "select * from orders into outfile '/tmp/x'"),
            (Dialect::Mysql, "select char(65 using utf8mb4)"),
            (Dialect::Mysql, "update ignore orders set total = 1"),
            (Dialect::Mysql, "update low_priority orders set total = 1"),
            (Dialect::Sqlite, "select * from orders indexed by i"),
            (Dialect::Sqlite, "select * from orders not indexed"),
            (Dialect::Sqlite, "select * from orders where id glob 'a*'"),
            (Dialect::Sqlite, "select * from orders where id is 0"),
            (Dialect::Sqlite, "select id << 2 from orders"),
        ] {
            assert!(
                in_dialect(fine, dialect).is_empty(),
                "{fine}: {:?}",
                in_dialect(fine, dialect)
            );
        }
        for wrong in [
            "select * frm orders",
            "select * form orders",
            "select * from orders wher id = 1",
            "insert into orders (id, total) valus (1, 2)",
            "delete form orders",
            "select * from orders where id = = 1",
        ] {
            assert_eq!(in_dialect(wrong, Dialect::Postgres).len(), 1, "{wrong}");
        }
        // Typing `o.` in the middle: what follows the cursor is not written yet.
        let sql = "select o. from orders o";
        assert!(diagnose(sql, Dialect::Postgres, None, 9).is_empty());
        assert!(!diagnose(sql, Dialect::Postgres, None, usize::MAX).is_empty());
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
            "select o.rowid, o.oid, o._rowid_, o.ctid, o.xmin, o.tableoid from orders o",
            "select o.total from orders o where exists (select 1 from customers o where o.id = 1)",
            "with orders as (select id, 1 as extra from orders) select orders.extra from orders",
            "create unlogged table ul (id int); select * from ul",
            "create table t2 (like orders including all); select * from t2",
            "select * into newtab from orders; select * from newtab",
            "create virtual table ft using fts5(body); select * from ft",
            "create temp table if not exists public.tmp1 as select 1; select * from tmp1",
        ] {
            assert!(messages(fine, Some(&known)).is_empty(), "{fine}");
        }
        assert!(messages("select * from ghosts", None).is_empty());
    }
}
