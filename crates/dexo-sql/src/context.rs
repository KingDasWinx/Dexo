use std::ops::Range;

use crate::dialect::Dialect;
use crate::lex::{Token, TokenKind, tokenize};
use crate::statement::statement_at;

/// How much the analysis trusts its own answer. `Low` means "I did not recognise this
/// shape" and callers should fall back to offering everything, which is what completion
/// did before there was any analysis at all -- so the worst case is never worse than it
/// used to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Inside a string or a comment: offer nothing.
    Suppressed,
    /// Columns of the row source the qualifier named (`u.` -> the columns of `u`).
    AliasColumn,
    /// Columns of everything this statement has in scope.
    Column,
    Table,
    Schema,
    Routine,
    InsertColumn,
    UpdateColumn,
    JoinCondition,
    /// Nothing recognised. Offer the lot, as before.
    Keyword,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
    Call,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowSourceKind {
    Table,
    Cte,
    Subquery,
    MutationTarget,
}

/// Something the statement can take columns from: a table in a FROM list, a CTE, a
/// subquery, or the target of an INSERT/UPDATE/DELETE.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowSource {
    pub kind: RowSourceKind,
    pub schema: Option<String>,
    pub name: String,
    pub alias: Option<String>,
    pub depth: u16,
}

impl RowSource {
    /// The name the user would write before a dot: the alias if there is one, else the
    /// table's own name.
    pub fn qualifier(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }

    pub fn qualified(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorContext {
    pub intent: Intent,
    pub confidence: Confidence,
    /// The identifier being typed, as written.
    pub prefix: String,
    /// The bytes a completion replaces. Empty range when the cursor follows a dot.
    pub replace: Range<usize>,
    /// The dotted parts before the cursor: `public.users.` -> ["public", "users"].
    pub qualifier: Vec<String>,
    /// Index into `row_sources` when the qualifier resolved to one.
    pub target_source: Option<usize>,
    pub row_sources: Vec<RowSource>,
    pub statement_kind: StatementKind,
}

impl CursorContext {
    pub fn target(&self) -> Option<&RowSource> {
        self.target_source
            .and_then(|index| self.row_sources.get(index))
    }
}

/// Words that introduce a table reference. After one of these, a name is a table.
const TABLE_INTRODUCERS: &[&str] = &[
    "from",
    "join",
    "straight_join",
    "update",
    "into",
    "using",
    "apply",
];

/// Words that end a table reference, so they can never be read as its alias.
const CLAUSE_WORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "cross",
    "outer",
    "natural",
    "on",
    "group",
    "order",
    "having",
    "limit",
    "offset",
    "union",
    "intersect",
    "except",
    "set",
    "values",
    "returning",
    "window",
    "fetch",
    "for",
    "into",
    "as",
    "and",
    "or",
    "not",
    "using",
    "lateral",
    "with",
    "insert",
    "update",
    "delete",
    "by",
    "asc",
    "desc",
];

/// Words after which a bare name is a column.
const COLUMN_WORDS: &[&str] = &[
    "where", "on", "and", "or", "having", "by", "select", "when", "then", "else", "case",
];

pub fn analyze(sql: &str, cursor: usize, dialect: Dialect) -> CursorContext {
    let cursor = cursor.min(sql.len());
    let tokens = tokenize(sql, dialect);

    if let Some(token) = tokens.iter().find(|token| {
        matches!(token.kind, TokenKind::String | TokenKind::Comment)
            && cursor > token.span.start
            && (cursor < token.span.end || !token.closed)
    }) {
        let _ = token;
        return empty(Intent::Suppressed, Confidence::High, cursor);
    }

    // Only the statement under the cursor is in scope. An alias declared in a previous
    // statement is not a thing this one can select from.
    let span = statement_at(sql, cursor).map(|span| span.byte_range);
    let (start, end) = span.map(|s| (s.start, s.end)).unwrap_or((0, sql.len()));
    let scope: Vec<&Token> = tokens
        .iter()
        .filter(|token| token.span.start >= start && token.span.end <= end)
        .collect();

    let (prefix, replace, qualifier) = trailing_identifier(sql, &scope, cursor);
    let statement_kind = statement_kind(sql, &scope);
    let row_sources = row_sources(sql, &scope);
    let previous = previous_word(sql, &scope, replace.start, &qualifier);

    let target_source = qualifier.last().and_then(|last| {
        row_sources
            .iter()
            .position(|source| source.qualifier().eq_ignore_ascii_case(last))
    });

    let (intent, confidence) = intent(
        statement_kind,
        previous.as_deref(),
        &qualifier,
        target_source,
        &row_sources,
    );

    CursorContext {
        intent,
        confidence,
        prefix,
        replace,
        qualifier,
        target_source,
        row_sources,
        statement_kind,
    }
}

fn intent(
    statement_kind: StatementKind,
    previous: Option<&str>,
    qualifier: &[String],
    target_source: Option<usize>,
    row_sources: &[RowSource],
) -> (Intent, Confidence) {
    let previous = previous.unwrap_or("");
    let after_introducer = TABLE_INTRODUCERS
        .iter()
        .any(|word| previous.eq_ignore_ascii_case(word));

    if statement_kind == StatementKind::Call || previous.eq_ignore_ascii_case("call") {
        return (Intent::Routine, Confidence::High);
    }
    if !qualifier.is_empty() {
        // `public.` right after FROM names a schema, not an alias -- even when some
        // table happens to be called `public`.
        if after_introducer {
            return (Intent::Table, Confidence::Medium);
        }
        if target_source.is_some() {
            return (Intent::AliasColumn, Confidence::High);
        }
        return (Intent::Schema, Confidence::Medium);
    }
    if after_introducer {
        return (Intent::Table, Confidence::High);
    }
    if previous.eq_ignore_ascii_case("set") && statement_kind == StatementKind::Update {
        return (Intent::UpdateColumn, Confidence::Medium);
    }
    if COLUMN_WORDS
        .iter()
        .any(|word| previous.eq_ignore_ascii_case(word))
        && !row_sources.is_empty()
    {
        if previous.eq_ignore_ascii_case("on") {
            return (Intent::JoinCondition, Confidence::Medium);
        }
        return (Intent::Column, Confidence::Medium);
    }
    (Intent::Keyword, Confidence::Low)
}

fn empty(intent: Intent, confidence: Confidence, cursor: usize) -> CursorContext {
    CursorContext {
        intent,
        confidence,
        prefix: String::new(),
        replace: cursor..cursor,
        qualifier: Vec::new(),
        target_source: None,
        row_sources: Vec::new(),
        statement_kind: StatementKind::Other,
    }
}

/// The identifier under the cursor and the dotted parts before it. The old code only
/// looked for a bare trailing dot, so one character past it (`u.e`) lost the alias
/// entirely and started matching table names instead.
fn trailing_identifier(
    sql: &str,
    scope: &[&Token],
    cursor: usize,
) -> (String, Range<usize>, Vec<String>) {
    let mut prefix = String::new();
    let mut replace = cursor..cursor;

    // A word the cursor sits at the end of (or inside) is the prefix being typed.
    let index = match scope
        .iter()
        .position(|token| token.span.start < cursor && token.span.end >= cursor)
    {
        Some(position) => {
            let token = scope[position];
            if matches!(token.kind, TokenKind::Word | TokenKind::QuotedIdent) {
                prefix = token.ident(sql).unwrap_or("").to_string();
                replace = token.span.start..cursor.max(token.span.start);
                position
            } else if token.kind == TokenKind::Punct && token.text(sql) == "." {
                position + 1
            } else {
                return (prefix, replace, Vec::new());
            }
        }
        None => scope
            .iter()
            .position(|token| token.span.start >= cursor)
            .unwrap_or(scope.len()),
    };

    let mut qualifier = Vec::new();
    let mut at = index;
    while at >= 1 {
        let dot = scope[at - 1];
        if !(dot.kind == TokenKind::Punct && dot.text(sql) == ".") {
            break;
        }
        let Some(part) = at.checked_sub(2).and_then(|i| scope.get(i)) else {
            break;
        };
        let Some(ident) = part.ident(sql) else {
            break;
        };
        qualifier.push(ident.to_string());
        at -= 2;
    }
    qualifier.reverse();
    (prefix, replace, qualifier)
}

/// The last word before the identifier being typed, skipping the dotted qualifier.
fn previous_word(sql: &str, scope: &[&Token], at: usize, qualifier: &[String]) -> Option<String> {
    let skip = qualifier.len() * 2;
    scope
        .iter()
        .filter(|token| token.span.end <= at && token.kind != TokenKind::Comment)
        .rev()
        .nth(skip)
        .filter(|token| token.kind == TokenKind::Word)
        .map(|token| token.text(sql).to_string())
}

fn statement_kind(sql: &str, scope: &[&Token]) -> StatementKind {
    let first = scope
        .iter()
        .find(|token| token.kind == TokenKind::Word)
        .map(|token| token.text(sql).to_ascii_lowercase());
    match first.as_deref() {
        Some("select") | Some("with") | Some("table") | Some("values") => StatementKind::Select,
        Some("insert") | Some("replace") => StatementKind::Insert,
        Some("update") => StatementKind::Update,
        Some("delete") => StatementKind::Delete,
        Some("call") | Some("exec") | Some("execute") => StatementKind::Call,
        _ => StatementKind::Other,
    }
}

/// Every table the statement can take columns from. Reading this off the tokens is what
/// replaces lowercasing the whole buffer and asking whether it contains "users u" --
/// which matched `users_archive ua`, and matched across statements.
fn row_sources(sql: &str, scope: &[&Token]) -> Vec<RowSource> {
    let mut sources = Vec::new();
    let mut index = 0;
    // `with name as (` declares a name the outer query can select from.
    while index + 2 < scope.len() {
        let token = scope[index];
        if (token.is_keyword(sql, "with") || token.text(sql) == ",")
            && let Some(name) = scope.get(index + 1).and_then(|t| t.ident(sql))
            && scope
                .get(index + 2)
                .is_some_and(|t| t.is_keyword(sql, "as"))
            && scope.get(index + 3).is_some_and(|t| t.text(sql) == "(")
        {
            sources.push(RowSource {
                kind: RowSourceKind::Cte,
                schema: None,
                name: name.to_string(),
                alias: None,
                depth: token.depth,
            });
        }
        index += 1;
    }

    let mut index = 0;
    while index < scope.len() {
        let token = scope[index];
        let introducer = TABLE_INTRODUCERS
            .iter()
            .any(|word| token.is_keyword(sql, word));
        if !introducer {
            index += 1;
            continue;
        }
        let kind = if token.is_keyword(sql, "update") || token.is_keyword(sql, "into") {
            RowSourceKind::MutationTarget
        } else {
            RowSourceKind::Table
        };
        index += 1;
        // A FROM list continues over commas: `from a x, b y`.
        loop {
            let Some(source) = take_row_source(sql, scope, &mut index, kind) else {
                break;
            };
            sources.push(source);
            if scope.get(index).is_some_and(|t| t.text(sql) == ",") {
                index += 1;
                continue;
            }
            break;
        }
    }
    sources
}

fn take_row_source(
    sql: &str,
    scope: &[&Token],
    index: &mut usize,
    kind: RowSourceKind,
) -> Option<RowSource> {
    let token = scope.get(*index)?;
    if token.text(sql) == "(" {
        // A subquery. Skip to its close; the name it can be referred to is the alias.
        let depth = token.depth;
        *index += 1;
        while scope
            .get(*index)
            .is_some_and(|t| !(t.text(sql) == ")" && t.depth == depth))
        {
            *index += 1;
        }
        *index += 1;
        let alias = take_alias(sql, scope, index);
        return Some(RowSource {
            kind: RowSourceKind::Subquery,
            schema: None,
            name: alias.clone().unwrap_or_default(),
            alias,
            depth,
        });
    }
    let mut parts = vec![token.ident(sql)?.to_string()];
    let depth = token.depth;
    *index += 1;
    while scope.get(*index).is_some_and(|t| t.text(sql) == ".") {
        let Some(part) = scope.get(*index + 1).and_then(|t| t.ident(sql)) else {
            break;
        };
        parts.push(part.to_string());
        *index += 2;
    }
    let name = parts.pop()?;
    let schema = parts.pop();
    let alias = take_alias(sql, scope, index);
    Some(RowSource {
        kind,
        schema,
        name,
        alias,
        depth,
    })
}

fn take_alias(sql: &str, scope: &[&Token], index: &mut usize) -> Option<String> {
    let mut at = *index;
    if scope.get(at).is_some_and(|t| t.is_keyword(sql, "as")) {
        at += 1;
    }
    let token = scope.get(at)?;
    let ident = token.ident(sql)?;
    if token.kind == TokenKind::Word
        && CLAUSE_WORDS
            .iter()
            .any(|word| ident.eq_ignore_ascii_case(word))
    {
        return None;
    }
    *index = at + 1;
    Some(ident.to_string())
}
