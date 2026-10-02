//! Problems the editor underlines as you type: statements that do not parse, and tables
//! or columns the catalog does not have -- reported only when the catalog has loaded the
//! place they would be, never guessed.

use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Range};
use std::sync::atomic::{AtomicU64, Ordering};

use sqlparser::ast::{Expr, Ident, ObjectNamePart, Query, Statement, TableFactor, Visit, Visitor};
use sqlparser::dialect::{DuckDbDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::statement::{first_keyword, split_statements_in};
use crate::{Diagnostic, Dialect};

/// What the catalog has loaded, lowercased: which schemas, the tables in each, and the
/// columns of the tables whose columns it knows.
#[derive(Clone, Debug)]
pub struct KnownObjects {
    /// Changes with every change to what is known, so a [`Diagnoser`] can tell.
    stamp: u64,
    schemas: HashSet<String>,
    tables: HashMap<String, HashSet<String>>,
    columns: HashMap<(String, String), HashSet<String>>,
}

static STAMPS: AtomicU64 = AtomicU64::new(0);

impl Default for KnownObjects {
    fn default() -> Self {
        Self {
            stamp: STAMPS.fetch_add(1, Ordering::Relaxed),
            schemas: HashSet::new(),
            tables: HashMap::new(),
            columns: HashMap::new(),
        }
    }
}

impl KnownObjects {
    pub fn add_table(&mut self, schema: &str, table: &str) {
        self.stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
        let schema = schema.to_lowercase();
        self.schemas.insert(schema.clone());
        self.tables
            .entry(table.to_lowercase())
            .or_default()
            .insert(schema);
    }

    pub fn add_column(&mut self, schema: &str, table: &str, column: &str) {
        self.stamp = STAMPS.fetch_add(1, Ordering::Relaxed);
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
/// `cursor`, a byte offset -- is not told off for an error at the cursor: that part is
/// not written yet.
pub fn diagnose(
    sql: &str,
    dialect: Dialect,
    known: Option<&KnownObjects>,
    cursor: usize,
) -> Vec<Diagnostic> {
    Diagnoser::default().diagnose(sql, dialect, known, cursor)
}

/// [`diagnose`] for a document checked on every key: each statement's answer is kept
/// until its text, the catalog or the dialect change, so a keystroke parses the
/// statement it lands in and not the whole script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnoser {
    /// The dialect and catalog stamp the kept answers were found with.
    against: Option<(Dialect, Option<u64>)>,
    /// Each statement's problems.
    answers: HashMap<String, Vec<Problem>>,
    /// The table each statement creates.
    creates: HashMap<String, Option<String>>,
    /// Where the cursor may go while the parse errors left out for it at the last look
    /// stay left out: each error and the blank before it.
    hidden: Vec<Range<usize>>,
}

/// One problem in a statement, offsets within it.
#[derive(Clone, Debug, PartialEq)]
struct Problem {
    message: String,
    range: Range<usize>,
    /// A parse error, which a statement still being typed may just not have finished.
    parse: bool,
    /// The table an `unknown table` names, which the document may create. Read when the
    /// answers are put together, not kept in them: the created tables were part of what
    /// the kept answers depended on, and each key typed in a CREATE TABLE's name threw
    /// them all away.
    table: Option<String>,
}

impl Diagnoser {
    pub fn diagnose(
        &mut self,
        sql: &str,
        dialect: Dialect,
        known: Option<&KnownObjects>,
        cursor: usize,
    ) -> Vec<Diagnostic> {
        let spans = split_statements_in(sql, dialect);
        let bodies: Vec<&str> = spans
            .iter()
            .map(|span| &sql[span.byte_range.clone()])
            .collect();
        if self
            .against
            .as_ref()
            .is_some_and(|(kept, _)| *kept != dialect)
        {
            self.creates.clear();
        }
        // Tables the document creates itself are known before the catalog hears of them.
        let created: HashSet<String> = bodies
            .iter()
            .filter_map(|body| match self.creates.get(*body) {
                Some(kept) => kept.clone(),
                None => {
                    let creates = created_table(body, dialect);
                    self.creates.insert(body.to_string(), creates.clone());
                    creates
                }
            })
            .collect();
        let against = (dialect, known.map(|known| known.stamp));
        if self.against.as_ref() != Some(&against) {
            self.answers.clear();
        }
        let present: HashSet<&str> = bodies.iter().copied().collect();
        self.answers
            .retain(|body, _| present.contains(body.as_str()));
        self.creates
            .retain(|body, _| present.contains(body.as_str()));
        self.against = Some(against);
        self.hidden.clear();
        let mut found = Vec::new();
        for (span, body) in spans.iter().zip(&bodies) {
            let start = span.byte_range.start;
            let typing =
                (cursor >= start && cursor <= span.byte_range.end + 1).then(|| cursor - start);
            let answer = match self.answers.get(*body) {
                Some(kept) => kept.clone(),
                None => {
                    let answer = statement_problems(body, dialect, known);
                    self.answers.insert(body.to_string(), answer.clone());
                    answer
                }
            };
            for problem in answer {
                if problem
                    .table
                    .as_ref()
                    .is_some_and(|table| created.contains(table))
                {
                    continue;
                }
                // Only an error near the cursor is left out: past it on its line or the
                // next -- what is being typed throws the parser off a few words on, as
                // `o.| from orders o` errs at the last `o`. Anything past the cursor was,
                // so an error lines further down stayed hidden while the cursor sat
                // above it.
                if let Some(cursor) = typing.filter(|_| problem.parse) {
                    let line = body[..problem.range.start]
                        .rfind('\n')
                        .map_or(0, |at| at + 1);
                    let from = body[..line.saturating_sub(1)]
                        .rfind('\n')
                        .map_or(0, |at| at + 1);
                    let to = if body[problem.range.end..].trim().is_empty() {
                        body.len() + 1
                    } else {
                        problem.range.end
                    };
                    if (from..=to).contains(&cursor) {
                        self.hidden.push(start + from..start + to + 1);
                        continue;
                    }
                }
                found.push(Diagnostic::local(
                    problem.message,
                    start + problem.range.start..start + problem.range.end,
                ));
            }
        }
        found
    }

    /// Whether an error left out for the cursor at the last look is away from `cursor`
    /// now: the document is to be looked at again, or the error stays hidden until the
    /// next edit.
    pub fn hidden_away_from(&self, cursor: usize) -> bool {
        self.hidden.iter().any(|near| !near.contains(&cursor))
    }
}

/// One statement's problems, offsets within it.
fn statement_problems(body: &str, dialect: Dialect, known: Option<&KnownObjects>) -> Vec<Problem> {
    let checked = matches!(
        first_keyword(body).as_deref(),
        Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES")
    );
    if !checked {
        return Vec::new();
    }
    let mut found = Vec::new();
    match parse(body, dialect) {
        Err(error) => {
            let message = error.to_string();
            let located = location(&message).and_then(|(line, column)| offset(body, line, column));
            // Ended too soon: the last word is where something more was wanted.
            let at = located.unwrap_or_else(|| last_word_start(body));
            let end = token_end(body, at);
            if located.is_some() && !beyond_doubt(body, at, dialect) {
                return found;
            }
            found.push(Problem {
                message: clean(&message),
                range: at..end,
                parse: true,
                table: None,
            });
        }
        Ok(statements) => {
            if let Some(known) = known {
                let mut refs = References::default();
                for statement in &statements {
                    let _ = statement.visit(&mut refs);
                }
                check(&refs, known, body, dialect, &mut found);
            }
        }
    }
    found
}

/// Whether a parse error at `at` is the SQL's and not the parser's. sqlparser knows only
/// part of each dialect, so an error is believed only when the highlighting grammar
/// fails the statement too and the statement uses none of the constructs sqlparser is
/// known to lack.
fn beyond_doubt(body: &str, at: usize, dialect: Dialect) -> bool {
    // ponytail: a list of what sqlparser fails on in each dialect; extend it as more
    // valid SQL turns up underlined.
    let unparsed: &[&[&str]] = match dialect {
        Dialect::Postgres => &[&["rows", "from"], &["symmetric"], &["asymmetric"]],
        Dialect::Mysql => &[
            &["lock", "in"],
            &["share", "mode"],
            &["with", "rollup"],
            &["outfile"],
            &["dumpfile"],
        ],
        Dialect::Sqlite => &[&["indexed", "by"], &["not", "indexed"], &["glob"]],
        Dialect::Duckdb => &[&["summarize"], &["pivot"], &["unpivot"], &["qualify"]],
    };
    // The statement's own words, read as the dialect reads them: one in a string, a
    // comment or a quoted name, or another dialect's construct, hid a real error.
    let tokens = match dialect {
        Dialect::Postgres => Tokenizer::new(&PostgreSqlDialect {}, body).tokenize(),
        Dialect::Mysql => Tokenizer::new(&MySqlDialect {}, body).tokenize(),
        Dialect::Sqlite => Tokenizer::new(&SQLiteDialect {}, body).tokenize(),
        Dialect::Duckdb => Tokenizer::new(&DuckDbDialect {}, body).tokenize(),
    };
    let words: Vec<String> = tokens
        .unwrap_or_default()
        .into_iter()
        .filter_map(|token| match token {
            Token::Word(word) if word.quote_style.is_none() => Some(word.value.to_lowercase()),
            _ => None,
        })
        .collect();
    let lacking = unparsed.iter().any(|construct| {
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
    let mut grammar = tree_sitter::Parser::new();
    grammar
        .set_language(&tree_sitter_sequel::LANGUAGE.into())
        .expect("tree-sitter-sequel language");
    grammar
        .parse(body, None)
        .is_none_or(|tree| tree.root_node().has_error())
}

/// Where the last word of `body` starts.
fn last_word_start(body: &str) -> usize {
    let trimmed = body.trim_end();
    trimmed.rfind(char::is_whitespace).map_or(0, |at| {
        at + trimmed[at..].chars().next().map_or(1, char::len_utf8)
    })
}

fn parse(sql: &str, dialect: Dialect) -> Result<Vec<Statement>, sqlparser::parser::ParserError> {
    let sql = &*crate::statement::line_ends(sql, dialect);
    match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
        Dialect::Duckdb => Parser::parse_sql(&DuckDbDialect {}, sql),
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

/// The tables and columns `refs` names that the catalog does not have. A table the
/// document itself creates is left for the caller to let pass.
fn check(
    refs: &References,
    known: &KnownObjects,
    body: &str,
    dialect: Dialect,
    found: &mut Vec<Problem>,
) {
    // One map for the whole statement, scopes and all: a name that stands for two
    // different tables somewhere in it -- `o` in a query and in its subquery -- stands
    // for neither, and its columns go unchecked.
    let mut aliases: HashMap<String, Option<(Option<String>, String)>> = HashMap::new();
    let mut name = |key: String, target: Option<(Option<String>, String)>| {
        aliases
            .entry(key)
            .and_modify(|known| {
                if *known != target {
                    *known = None;
                }
            })
            .or_insert(target);
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
        // A CTE named like a table stands for the CTE, under an alias too: its columns
        // are the CTE's, not the table's.
        let target = (schema.is_some() || !refs.ctes.contains(&table))
            .then(|| (schema.clone(), table.clone()));
        name(table.clone(), target.clone());
        if let Some(alias) = alias {
            name(alias.clone(), target);
        }
        // `FROM ONLY t`, `UPDATE IGNORE t`: the parser took a modifier it does not know
        // there for the table. Any keyword passed here once, and a missing table named
        // `status`, `data` or `session` was never reported.
        let modifier = parts.len() == 1
            && parts[0].quote_style.is_none()
            && TABLE_MODIFIERS.contains(&table.as_str());
        let unknown = match &schema {
            None if modifier => false,
            None => {
                !refs.ctes.contains(&table)
                    && !system_name(&table)
                    && !known.has_table(None, &table)
            }
            // A schema the catalog never listed may be a system one; nothing is said.
            Some(schema) => {
                known.schemas.contains(schema) && !known.has_table(Some(schema), &table)
            }
        };
        if unknown
            && let (Some(first), Some(last)) = (parts.first(), parts.last())
            && let Some(range) = span_of(body, first, last)
        {
            let shown: Vec<&str> = parts.iter().map(|ident| ident.value.as_str()).collect();
            found.push(Problem {
                message: format!("unknown table {}", shown.join(".")),
                range,
                parse: false,
                table: Some(table),
            });
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
            && !system_column(&name, dialect)
            && let Some(range) = span_of(body, column, column)
        {
            found.push(Problem {
                message: format!("unknown column {} in {table}", column.value),
                range,
                parse: false,
                table: None,
            });
        }
    }
}

/// Words that may stand before a table's name, which sqlparser reads as the name.
const TABLE_MODIFIERS: &[&str] = &[
    "only",
    "ignore",
    "low_priority",
    "high_priority",
    "delayed",
    "quick",
    "lateral",
];

/// Whether every row of `dialect` has the column `name` without the table declaring
/// it: SQLite's rowid and its aliases, Postgres's system columns (`oid` left Postgres
/// 12's tables), MySQL's `_rowid` for a one-column integer key. One list for all let
/// `o.rowid` pass on MySQL and `o.oid` on Postgres.
fn system_column(name: &str, dialect: Dialect) -> bool {
    let names: &[&str] = match dialect {
        Dialect::Postgres => &["ctid", "xmin", "xmax", "cmin", "cmax", "tableoid"],
        Dialect::Sqlite => &["rowid", "oid", "_rowid_"],
        Dialect::Mysql => &["_rowid"],
        Dialect::Duckdb => &["rowid"],
    };
    names.contains(&name)
}

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
pub fn created_table(body: &str, dialect: Dialect) -> Option<String> {
    let tokens = match dialect {
        Dialect::Postgres => Tokenizer::new(&PostgreSqlDialect {}, body).tokenize(),
        Dialect::Mysql => Tokenizer::new(&MySqlDialect {}, body).tokenize(),
        Dialect::Sqlite => Tokenizer::new(&SQLiteDialect {}, body).tokenize(),
        Dialect::Duckdb => Tokenizer::new(&DuckDbDialect {}, body).tokenize(),
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
        // `SELECT .. INTO name`, a CTE before it or not: the words after the INTO of the
        // main SELECT. Only Postgres makes a table of it -- MySQL's INTO fills variables
        // or a file, and the word after it was learned as a table -- and an INTO inside
        // parentheses, or an INSERT's, is not that one.
        "SELECT" | "WITH" if dialect == Dialect::Postgres => {
            let mut depth = 0_usize;
            let mut main = (first == "SELECT").then_some(first.clone());
            let mut after_into = None;
            while let Some(token) = tokens.next() {
                match token {
                    Token::LParen => depth += 1,
                    Token::RParen => depth = depth.saturating_sub(1),
                    _ if depth > 0 => {}
                    _ => match word(&token) {
                        Some((upper, _))
                            if main.is_none()
                                && matches!(
                                    upper.as_str(),
                                    "SELECT"
                                        | "INSERT"
                                        | "UPDATE"
                                        | "DELETE"
                                        | "MERGE"
                                        | "VALUES"
                                        | "TABLE"
                                ) =>
                        {
                            main = Some(upper);
                        }
                        Some((upper, _))
                            if upper == "INTO" && main.as_deref() == Some("SELECT") =>
                        {
                            after_into =
                                Some(tokens.by_ref().map_while(|token| word(&token)).collect());
                            break;
                        }
                        _ => {}
                    },
                }
            }
            after_into?
        }
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
        // A construct sqlparser lacks counts only as code of its own dialect: not in a
        // string, a comment or a quoted name, nor another dialect's.
        for (dialect, wrong) in [
            (Dialect::Postgres, "select * frm orders -- rows from"),
            (
                Dialect::Postgres,
                "select * frm orders where note = 'symmetric'",
            ),
            (Dialect::Postgres, "select \"glob\" frm orders"),
            (Dialect::Postgres, "select * frm orders where a glob b"),
            (
                Dialect::Mysql,
                "select * frm orders where note = 'with rollup'",
            ),
            (Dialect::Mysql, "select * frm orders # into outfile"),
            (Dialect::Sqlite, "select * frm orders /* not indexed */"),
        ] {
            assert_eq!(in_dialect(wrong, dialect).len(), 1, "{dialect:?}: {wrong}");
        }
        // Typing `o.` in the middle: what follows the cursor is not written yet.
        let sql = "select o. from orders o";
        assert!(diagnose(sql, Dialect::Postgres, None, 9).is_empty());
        assert!(!diagnose(sql, Dialect::Postgres, None, usize::MAX).is_empty());
    }

    /// Only an error at the cursor is left out of the statement being typed, and moving
    /// the cursor away says the document is to be looked at again.
    #[test]
    fn only_an_error_at_the_cursor_waits() {
        // The error lines below the cursor: not what is being typed.
        let later = "select id,\n  total,\n  customer_id\nfrom orders where id = = 1";
        assert_eq!(diagnose(later, Dialect::Postgres, None, 10).len(), 1);
        assert!(diagnose(later, Dialect::Postgres, None, 30).is_empty());
        let mut diagnoser = super::Diagnoser::default();
        // Typing `o.` on the line above throws the parser off on the next one.
        let typing = "select o.\nfrom orders o;\nselect 1";
        assert!(
            diagnoser
                .diagnose(typing, Dialect::Postgres, None, 9)
                .is_empty()
        );
        assert!(!diagnoser.hidden_away_from(9) && !diagnoser.hidden_away_from(3));
        // The cursor gone to another statement: the error is to be shown, and is.
        let elsewhere = typing.len();
        assert!(diagnoser.hidden_away_from(elsewhere));
        assert_eq!(
            diagnoser
                .diagnose(typing, Dialect::Postgres, None, elsewhere)
                .len(),
            1
        );
        assert!(!diagnoser.hidden_away_from(elsewhere));
        // Ending too soon at the end, a blank typed after it or not.
        let open = "select * from orders where ";
        assert!(
            diagnoser
                .diagnose(open, Dialect::Postgres, None, open.len())
                .is_empty()
        );
        assert!(!diagnoser.hidden_away_from(open.len()));
    }

    /// A kept answer is the one a fresh look gives: after the statement, the catalog or
    /// the created tables change, and wherever the statement moves in the document.
    #[test]
    fn kept_answers_follow_what_they_depend_on() {
        let mut diagnoser = super::Diagnoser::default();
        let mut known = known();
        let mut run = |sql: &str, known: &KnownObjects| {
            diagnoser
                .diagnose(sql, Dialect::Postgres, Some(known), usize::MAX)
                .into_iter()
                .map(|found| (found.message, found.byte_range.unwrap()))
                .collect::<Vec<_>>()
        };
        let sql = "select * from ghosts;\nselect * frm orders";
        assert_eq!(run(sql, &known), diagnose_all(sql, &known));
        let moved = "select 1;\nselect * from ghosts;\nselect * frm orders";
        assert_eq!(run(moved, &known), diagnose_all(moved, &known));
        known.add_table("public", "ghosts");
        assert_eq!(run(moved, &known), diagnose_all(moved, &known));
        assert_eq!(run(moved, &known).len(), 1);
        let creates = "create table fresh (id int);\nselect * from fresh";
        assert_eq!(run(creates, &known), diagnose_all(creates, &known));
        assert!(run(creates, &known).is_empty());
    }

    /// Typing a CREATE TABLE's name changes what the document creates, and that alone
    /// keeps every other statement's answer: it used to throw them all away.
    #[test]
    fn a_created_table_keeps_the_other_answers() {
        let known = known();
        let mut diagnoser = super::Diagnoser::default();
        let messages = |found: Vec<crate::Diagnostic>| {
            found
                .into_iter()
                .map(|found| found.message)
                .collect::<Vec<_>>()
        };
        let first = "select * from ghosts;\ncreate table gh (id int)";
        assert_eq!(
            messages(diagnoser.diagnose(first, Dialect::Postgres, Some(&known), usize::MAX)),
            ["unknown table ghosts"]
        );
        // Marked, so an answer worked out again would show.
        diagnoser
            .answers
            .get_mut("select * from ghosts")
            .unwrap()
            .push(super::Problem {
                message: "kept".into(),
                range: 0..1,
                parse: false,
                table: None,
            });
        let typed = "select * from ghosts;\ncreate table gho (id int)";
        assert!(
            messages(diagnoser.diagnose(typed, Dialect::Postgres, Some(&known), usize::MAX))
                .contains(&"kept".to_string())
        );
        // Once the document creates the table, it is not unknown.
        let created = "select * from ghosts;\ncreate table ghosts (id int)";
        assert_eq!(
            messages(diagnoser.diagnose(created, Dialect::Postgres, Some(&known), usize::MAX)),
            ["kept"]
        );
    }

    fn diagnose_all(sql: &str, known: &KnownObjects) -> Vec<(String, std::ops::Range<usize>)> {
        diagnose(sql, Dialect::Postgres, Some(known), usize::MAX)
            .into_iter()
            .map(|found| (found.message, found.byte_range.unwrap()))
            .collect()
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
            "select o.ctid, o.xmin, o.xmax, o.cmin, o.cmax, o.tableoid from orders o",
            "select o.total from orders o where exists (select 1 from customers o where o.id = 1)",
            "with orders as (select id, 1 as extra from orders) select orders.extra from orders",
            "with orders as (select id, 1 as extra from orders) select o.extra from orders o",
            "create unlogged table ul (id int); select * from ul",
            "create table t2 (like orders including all); select * from t2",
            "select * into newtab from orders; select * from newtab",
            "create virtual table ft using fts5(body); select * from ft",
            "create temp table if not exists public.tmp1 as select 1; select * from tmp1",
        ] {
            assert!(messages(fine, Some(&known)).is_empty(), "{fine}");
        }
        assert!(messages("select * from ghosts", None).is_empty());
        // Each dialect's columns every row has, and no other dialect's.
        let columns = |sql: &str, dialect| {
            diagnose(sql, dialect, Some(&known), usize::MAX)
                .into_iter()
                .map(|found| found.message)
                .collect::<Vec<_>>()
        };
        assert!(
            columns(
                "select o.rowid, o.oid, o._rowid_ from orders o",
                Dialect::Sqlite
            )
            .is_empty()
        );
        assert!(columns("select o._rowid from orders o", Dialect::Mysql).is_empty());
        assert_eq!(
            columns("select o.oid, o.rowid from orders o", Dialect::Postgres),
            [
                "unknown column oid in orders",
                "unknown column rowid in orders"
            ]
        );
        assert_eq!(
            columns("select o.rowid, o.ctid from orders o", Dialect::Mysql),
            [
                "unknown column rowid in orders",
                "unknown column ctid in orders"
            ]
        );
        // A missing table named like a keyword is missing too.
        for word in ["status", "data", "name", "session"] {
            let sql = format!("select * from {word}");
            assert_eq!(
                messages(&sql, Some(&known)),
                [(format!("unknown table {word}"), word.to_string())],
                "{sql}"
            );
        }
    }

    /// Only Postgres makes a table of `SELECT … INTO`, at the top level, a CTE before
    /// it or not; MySQL's INTO fills variables or a file.
    #[test]
    fn select_into_creates_a_table_only_where_it_does() {
        use super::created_table;
        let postgres = |sql: &str| created_table(sql, Dialect::Postgres);
        assert_eq!(
            postgres("select * into fresh from orders").as_deref(),
            Some("fresh")
        );
        assert_eq!(
            postgres("select * into unlogged table ul from orders").as_deref(),
            Some("ul")
        );
        assert_eq!(
            postgres("with recent as (select 1) select * into fresh from recent").as_deref(),
            Some("fresh")
        );
        assert_eq!(
            postgres("with recent as (select 1) insert into orders select * from recent"),
            None
        );
        assert_eq!(postgres("select (select 1) from orders"), None);
        for sql in [
            "select * from orders into outfile '/tmp/x'",
            "select total into @v from orders",
            "select id into v_id from orders",
        ] {
            assert_eq!(created_table(sql, Dialect::Mysql), None, "{sql}");
        }
        assert_eq!(created_table("select 1 into x", Dialect::Sqlite), None);
    }
}
