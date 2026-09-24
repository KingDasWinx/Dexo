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
    /// A name being declared rather than looked up: the alias after a table in a FROM
    /// list, or after `AS`. Offering names here turned `from users u` + Enter into
    /// `from users users`, so only the clauses that can follow are offered.
    Alias,
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
    /// A name would start here: whitespace before the cursor, and before that something
    /// still waiting for an operand. `where |` is such a place; `count(|` is still
    /// punctuation being typed, and `where id = 1 |` wants an operator or a keyword.
    pub awaiting_name: bool,
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
    "apply",
    "table",
    "truncate",
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

/// When typing is allowed to open the popup on its own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerMode {
    /// Only when asked for, with Ctrl+Space.
    Manual,
    /// Once an identifier has been started, or after a dot.
    RequirePrefix,
    /// Also where the position itself says what belongs there: after FROM, after ON,
    /// inside a WHERE clause.
    #[default]
    Positional,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerOrigin {
    Typing,
    Explicit,
}

/// Whether the popup should be on screen. Asking for it explicitly always works, except
/// inside a string or a comment, where there is nothing to offer either way.
pub fn should_open(mode: TriggerMode, context: &CursorContext, origin: TriggerOrigin) -> bool {
    if context.intent == Intent::Suppressed {
        return false;
    }
    if origin == TriggerOrigin::Explicit {
        return true;
    }
    // After a table only a clause keyword is on offer, and only from two letters on: a
    // one-letter alias followed by Enter must stay an alias.
    if context.intent == Intent::Alias {
        return mode != TriggerMode::Manual && context.prefix.chars().count() >= 2;
    }
    let started = !context.prefix.is_empty() || !context.qualifier.is_empty();
    match mode {
        TriggerMode::Manual => false,
        TriggerMode::RequirePrefix => started,
        // Before anything is typed, only where the position alone says what goes there,
        // and only once the punctuation is done: `where |` opens, `count(|` does not.
        TriggerMode::Positional => {
            started
                || match context.intent {
                    Intent::InsertColumn => true,
                    Intent::Table | Intent::UpdateColumn | Intent::Routine | Intent::Schema => {
                        context.awaiting_name
                    }
                    Intent::Column | Intent::JoinCondition => {
                        context.awaiting_name && !context.row_sources.is_empty()
                    }
                    _ => false,
                }
        }
    }
}

pub fn analyze(sql: &str, cursor: usize, dialect: Dialect) -> CursorContext {
    let cursor = cursor.min(sql.len());
    let tokens = tokenize(sql, dialect);

    if tokens.iter().any(|token| token.holds(sql, cursor)) {
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
    let place = place(sql, &scope, replace.start, &qualifier);

    let target_source = qualifier.last().and_then(|last| {
        row_sources
            .iter()
            .position(|source| source.qualifier().eq_ignore_ascii_case(last))
    });

    let (intent, confidence) = intent(
        statement_kind,
        place,
        &qualifier,
        target_source,
        &row_sources,
    );

    CursorContext {
        intent,
        confidence,
        awaiting_name: sql[..replace.start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
            && !ends_operand(sql, &scope, replace.start),
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
    place: Place,
    qualifier: &[String],
    target_source: Option<usize>,
    row_sources: &[RowSource],
) -> (Intent, Confidence) {
    if statement_kind == StatementKind::Call {
        return (Intent::Routine, Confidence::High);
    }
    if !qualifier.is_empty() {
        // `public.` right after FROM names a schema, not an alias -- even when some
        // table happens to be called `public`.
        if place == Place::TableName {
            return (Intent::Table, Confidence::Medium);
        }
        if target_source.is_some() {
            return (Intent::AliasColumn, Confidence::High);
        }
        return (Intent::Schema, Confidence::Medium);
    }
    match place {
        Place::TableName => (Intent::Table, Confidence::High),
        Place::AfterTable | Place::AliasName => (Intent::Alias, Confidence::High),
        Place::InsertColumns => (Intent::InsertColumn, Confidence::High),
        Place::SetTarget => (Intent::UpdateColumn, Confidence::High),
        Place::Expression { on: true } if !row_sources.is_empty() => {
            (Intent::JoinCondition, Confidence::Medium)
        }
        Place::Expression { .. } if !row_sources.is_empty() => (Intent::Column, Confidence::Medium),
        // Typing the SELECT list before a FROM exists: nothing to take columns from yet,
        // but functions and keywords still belong here.
        Place::Expression { .. } => (Intent::Column, Confidence::Low),
        Place::Other => (Intent::Keyword, Confidence::Low),
    }
}

/// Where the cursor sits in the statement's grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Place {
    /// Right after FROM/JOIN/INTO/UPDATE/TABLE, or after a comma in a FROM list.
    TableName,
    /// After a table in a FROM list: its alias, or the next clause's keyword.
    AfterTable,
    /// `select total as |`.
    AliasName,
    /// An expression: the SELECT list, WHERE, ON, HAVING, GROUP/ORDER BY, CASE, a SET
    /// value, a function's arguments.
    Expression { on: bool },
    /// `insert into t (|`.
    InsertColumns,
    /// `update t set |`, and after each comma in that list.
    SetTarget,
    /// VALUES rows, LIMIT, a statement's first word: nowhere a name is looked up.
    Other,
}

/// Walks back from the cursor to the clause that governs it, at the cursor's own
/// parenthesis level. Deciding from the single word before the cursor, as this used
/// to, missed every column after a comma, an operator, `(`, DISTINCT, IN, NOT...: most
/// of the places a column is typed.
fn place(sql: &str, scope: &[&Token], at: usize, qualifier: &[String]) -> Place {
    let before: Vec<&Token> = scope
        .iter()
        .copied()
        .filter(|token| token.span.end <= at && token.kind != TokenKind::Comment)
        .collect();
    let before = &before[..before.len().saturating_sub(qualifier.len() * 2)];
    // Tokens between the governing clause and the cursor, nearest first. A parenthesised
    // group before the cursor counts as its `)`; the one the cursor is inside, as `(`.
    let mut since: Vec<&Token> = Vec::new();
    let mut level = 0usize;
    let mut i = before.len();
    while i > 0 {
        i -= 1;
        let token = before[i];
        let text = token.text(sql);
        if token.kind == TokenKind::Punct && text == ")" {
            if level == 0 {
                since.push(token);
            }
            level += 1;
            continue;
        }
        if token.kind == TokenKind::Punct && text == "(" {
            if level > 0 {
                level -= 1;
                continue;
            }
            // The parenthesis the cursor is inside. Its opener decides what it holds;
            // anything else -- a function call, IN (...) -- belongs to the clause outside.
            let opener = &before[..i];
            if is_insert_target(sql, opener) {
                return Place::InsertColumns;
            }
            match opener.last() {
                Some(word) if word.is_keyword(sql, "values") => return Place::Other,
                Some(word) if word.is_keyword(sql, "using") => {
                    return Place::Expression { on: false };
                }
                _ => {}
            }
            since.clear();
            since.push(token);
            continue;
        }
        if level > 0 {
            continue;
        }
        if token.kind == TokenKind::Word
            && let Some(place) = clause_place(sql, &text.to_ascii_lowercase(), &before[..i], &since)
        {
            return place;
        }
        since.push(token);
    }
    Place::Other
}

/// The place a clause keyword gives the cursor, or `None` when `word` is not one and
/// the walk has to keep going back.
fn clause_place(sql: &str, word: &str, earlier: &[&Token], since: &[&Token]) -> Option<Place> {
    let nearest = since
        .first()
        .map(|token| token.text(sql).to_ascii_lowercase());
    let nearest = nearest.as_deref();
    Some(match word {
        "select" | "where" | "having" | "when" | "then" | "else" | "case" | "returning" | "on" => {
            if nearest == Some("as") {
                Place::AliasName
            } else {
                Place::Expression { on: word == "on" }
            }
        }
        "by" => {
            let owner = earlier.last()?;
            if !(owner.is_keyword(sql, "group")
                || owner.is_keyword(sql, "order")
                || owner.is_keyword(sql, "partition"))
            {
                return None;
            }
            if nearest == Some("as") {
                Place::AliasName
            } else {
                Place::Expression { on: false }
            }
        }
        "from" | "join" | "straight_join" | "into" | "update" | "table" | "truncate" => {
            match nearest {
                None | Some(",") => Place::TableName,
                // `from (|`: a subquery starts here, or a column list is being declared.
                Some("(") => Place::Other,
                _ => Place::AfterTable,
            }
        }
        "set" => match nearest {
            None | Some(",") => Place::SetTarget,
            _ => Place::Expression { on: false },
        },
        "values" | "limit" | "offset" | "fetch" | "insert" | "delete" | "create" | "alter"
        | "drop" | "grant" | "revoke" | "with" | "explain" | "show" => Place::Other,
        _ => return None,
    })
}

/// `insert into [schema.]name` right before a `(`: the parenthesis lists its columns.
fn is_insert_target(sql: &str, opener: &[&Token]) -> bool {
    let mut at = opener.len();
    let Some(last) = at.checked_sub(1).and_then(|i| opener.get(i)) else {
        return false;
    };
    if last.ident(sql).is_none() {
        return false;
    }
    at -= 1;
    while at >= 2 && opener[at - 1].text(sql) == "." && opener[at - 2].ident(sql).is_some() {
        at -= 2;
    }
    at >= 2 && opener[at - 1].is_keyword(sql, "into") && opener[at - 2].is_keyword(sql, "insert")
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
        awaiting_name: false,
    }
}

/// Words after which an operand still has to come.
const EXPECTS_OPERAND: &[&str] = &[
    "and",
    "or",
    "not",
    "like",
    "ilike",
    "in",
    "is",
    "between",
    "distinct",
    "all",
    "any",
    "some",
    "exists",
    "case",
    "when",
    "then",
    "else",
    "select",
    "where",
    "having",
    "on",
    "by",
    "set",
    "from",
    "join",
    "into",
    "update",
    "table",
    "truncate",
    "returning",
    "with",
    "union",
    "intersect",
    "except",
    "using",
    "over",
];

/// Whether the token before `at` completes an operand -- a name, a literal, a closed
/// parenthesis -- so what follows is an operator or a keyword rather than a name.
fn ends_operand(sql: &str, scope: &[&Token], at: usize) -> bool {
    let Some(last) = scope
        .iter()
        .rev()
        .find(|token| token.span.end <= at && token.kind != TokenKind::Comment)
    else {
        return false;
    };
    match last.kind {
        TokenKind::Number | TokenKind::String | TokenKind::QuotedIdent | TokenKind::Param => true,
        TokenKind::Punct => last.text(sql) == ")",
        TokenKind::Word => !EXPECTS_OPERAND
            .iter()
            .any(|word| last.text(sql).eq_ignore_ascii_case(word)),
        TokenKind::Operator | TokenKind::Comment => false,
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
