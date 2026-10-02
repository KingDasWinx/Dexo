use std::ops::Range;

use crate::Dialect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatementEffect {
    ReadOnly,
    DataWrite,
    SchemaWrite,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatementSpan {
    pub byte_range: Range<usize>,
    pub effect: StatementEffect,
    pub understood: bool,
}

/// Splits a buffer into statements. A `;` always ends one; so does a line that starts
/// with a statement keyword (SELECT, INSERT, WITH, ...) outside parentheses, unless the
/// text before it is still expecting it -- `UNION`, `AS`, `INSERT INTO t (...)`,
/// `WITH x AS (...)`. Without that second rule a statement missing its `;` swallowed
/// the next one: Ctrl+Enter sent both, and completion offered the neighbour's columns.
pub fn split_statements(sql: &str) -> Vec<StatementSpan> {
    let bytes = sql.as_bytes();
    let mut spans = Vec::new();
    let mut start = skip_ws(sql, 0);
    let mut i = start;
    let mut scan = Scan::default();
    while i < bytes.len() {
        // A psql backslash command -- `\dt`, `\d orders` -- is a line, not SQL: it ends
        // where the line does, `;` or not.
        if i == start && bytes[i] == b'\\' {
            let line_end = sql[i..].find('\n').map_or(bytes.len(), |at| i + at);
            spans.push(classify_span(sql, start..trim_end(sql, start, line_end)));
            start = skip_ws(sql, line_end);
            i = start;
            scan = Scan::default();
            continue;
        }
        match bytes[i] {
            // Inside a trigger's or routine's BEGIN ... END, a `;` ends a statement of
            // the body.
            b';' if scan.block > 0 => {
                scan.last = Last::Other;
                i += 1;
            }
            b';' => {
                if start < i {
                    spans.push(classify_span(sql, start..i));
                }
                i += 1;
                start = skip_ws(sql, i);
                i = start;
                scan = Scan::default();
            }
            b' ' | b'\t' | b'\r' | b'\n' => {
                let next = skip_ws(sql, i);
                let new_line = sql[i..next].contains('\n');
                if new_line && bytes.get(next) == Some(&b'\\') && scan.depth == 0 && scan.block == 0
                {
                    spans.push(classify_span(sql, start..trim_end(sql, start, i)));
                    start = next;
                    scan = Scan::default();
                } else if new_line
                    && let Some(word) = take_ident(&sql[next..])
                    && scan.starts_new(word)
                {
                    spans.push(classify_span(sql, start..trim_end(sql, start, i)));
                    start = next;
                    scan = Scan::default();
                }
                i = next;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line_comment(sql, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_comment(sql, i).unwrap_or(i + 2),
            b'(' => {
                scan.depth += 1;
                scan.last = Last::Open;
                i += 1;
            }
            b')' => {
                scan.depth = scan.depth.saturating_sub(1);
                scan.last = Last::Close;
                i += 1;
            }
            b',' => {
                scan.last = Last::Comma;
                i += 1;
            }
            b'=' => {
                scan.last = Last::Operator;
                i += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let word = take_ident(&sql[i..]).unwrap_or("");
                scan.word(word);
                i += word.len().max(1);
            }
            _ => {
                // A Postgres routine's body is one dollar-quoted string, with no END to
                // close it: past it, a new line may start the next statement.
                if scan.routine && scan.depth == 0 && skip_dollar(sql, i).is_some() {
                    scan.body_closed = true;
                }
                scan.last = Last::Other;
                i = skip_atom(sql, i).max(i + 1);
            }
        }
    }
    if start < bytes.len() && !sql[start..].trim().is_empty() {
        spans.push(classify_span(sql, start..bytes.len()));
    }
    spans
}

/// Words between a CREATE and the kind of object it makes. AGGREGATE is MariaDB's
/// stored aggregate function, whose body is a block too.
const CREATE_MODIFIERS: &[&str] = &[
    "OR",
    "REPLACE",
    "DEFINER",
    "CURRENT_USER",
    "TEMP",
    "TEMPORARY",
    "CONSTRAINT",
    "AGGREGATE",
];

/// Words that start a statement when they open a line.
const STARTERS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "WITH", "CREATE", "ALTER", "DROP", "TRUNCATE", "MERGE",
    "EXPLAIN", "SHOW", "CALL", "GRANT", "REVOKE",
];

/// Words after which a statement keyword on the next line still belongs to the same
/// statement: `UNION\nSELECT`, `AS\nSELECT`, `THEN\nUPDATE`, `FOR\nUPDATE`.
const CONTINUATIONS: &[&str] = &[
    "AS",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "MINUS",
    "ALL",
    "DISTINCT",
    "THEN",
    "ELSE",
    "DO",
    "INSTEAD",
    "ALSO",
    "FOR",
    "ON",
    "KEY",
    "OR",
    "AFTER",
    "BEFORE",
    "OF",
    "EXPLAIN",
    "ANALYZE",
    "VERBOSE",
    "GRANT",
    "REVOKE",
    "IN",
    "EXISTS",
    "NOT",
    "ANY",
    "SOME",
    "RETURNS",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Last {
    #[default]
    None,
    Word,
    Open,
    Close,
    Comma,
    Operator,
    Other,
}

/// What the scan knows about the statement it is inside.
#[derive(Default)]
struct Scan {
    first: Option<String>,
    depth: u32,
    last: Last,
    last_word: String,
    /// An INSERT whose rows have not started: a SELECT or WITH on the next line is its
    /// source, not a new statement.
    insert_awaits_rows: bool,
    /// A WITH at the top level: after its `)` the main query follows on its own line.
    cte: bool,
    /// A CREATE whose object is not yet named: only modifiers have followed it.
    create_header: bool,
    /// A CREATE of a trigger, function, procedure or event: its body is a block.
    routine: bool,
    /// `BEGIN`/`CASE` blocks open inside a routine's body. While one is open, `;` ends
    /// a statement of the body, not the CREATE.
    block: u32,
    /// The routine's BEGIN ... END body has closed: what follows on a new line may be
    /// the next statement.
    body_closed: bool,
    /// The last END closed a counted block, so an `IF` after it gives the count back.
    end_counted: bool,
}

impl Scan {
    fn word(&mut self, word: &str) {
        let upper = word.to_ascii_uppercase();
        if self.first.is_none() {
            self.insert_awaits_rows = matches!(upper.as_str(), "INSERT" | "REPLACE");
            self.create_header = upper == "CREATE";
            self.first = Some(upper.clone());
        } else if self.create_header && self.depth == 0 {
            // A routine is one only when it is what the CREATE makes, named past its
            // modifiers -- `OR REPLACE`, `TEMP`, MySQL's `DEFINER = user@host`. Found
            // anywhere, TRIGGER, FUNCTION, PROCEDURE or EVENT held the split: `CREATE
            // TABLE event (…)` swallowed the statement on the next line.
            if matches!(
                upper.as_str(),
                "TRIGGER" | "FUNCTION" | "PROCEDURE" | "EVENT"
            ) {
                self.routine = true;
                self.create_header = false;
            } else if !(CREATE_MODIFIERS.contains(&upper.as_str())
                || matches!(self.last, Last::Operator | Last::Other))
            {
                self.create_header = false;
            }
        } else if self.depth == 0
            && self.insert_awaits_rows
            && matches!(
                upper.as_str(),
                "SELECT" | "VALUES" | "VALUE" | "SET" | "DEFAULT"
            )
        {
            self.insert_awaits_rows = false;
        }
        if self.depth == 0 && upper == "WITH" {
            self.cte = true;
        }
        if self.routine {
            // `END IF`, `END LOOP`, ... close what was never counted as opening, so the
            // END before them gives its count back; `END CASE` closes a counted CASE.
            let after_end = self.last == Last::Word && self.last_word == "END";
            match upper.as_str() {
                "BEGIN" | "CASE" if !after_end => self.block += 1,
                "END" => {
                    self.end_counted = self.block > 0;
                    self.block = self.block.saturating_sub(1);
                    self.body_closed = self.end_counted && self.block == 0;
                }
                "IF" | "LOOP" | "WHILE" | "REPEAT" if after_end && self.end_counted => {
                    self.block += 1;
                    self.body_closed = false;
                }
                _ => {}
            }
        }
        self.last = Last::Word;
        self.last_word = upper;
    }

    fn starts_new(&self, word: &str) -> bool {
        let upper = word.to_ascii_uppercase();
        // A routine's body may be one statement without BEGIN -- `FOR EACH ROW UPDATE
        // ...` -- and is the routine's, not a statement of its own.
        if self.first.is_none()
            || self.depth > 0
            || self.block > 0
            || (self.routine && !self.body_closed)
            || !STARTERS.contains(&upper.as_str())
        {
            return false;
        }
        match self.last {
            Last::Comma | Last::Open | Last::Operator => return false,
            Last::Word if CONTINUATIONS.contains(&self.last_word.as_str()) => return false,
            _ => {}
        }
        if self.insert_awaits_rows && matches!(upper.as_str(), "SELECT" | "WITH") {
            return false;
        }
        !(self.cte && self.last == Last::Close)
    }
}

fn skip_line_comment(sql: &str, i: usize) -> usize {
    sql[i..].find('\n').map_or(sql.len(), |offset| i + offset)
}

fn trim_end(sql: &str, start: usize, end: usize) -> usize {
    start + sql[start..end].trim_end().len()
}

pub fn statement_at(sql: &str, byte_index: usize) -> Option<StatementSpan> {
    pick_statement(sql, split_statements(sql), byte_index)
}

/// [`split_statements`] read the way `dialect` reads comments and strings. MySQL takes
/// `#` as a line comment and a backslash as an escape inside strings; the plain splitter
/// knows neither, so an apostrophe in a `#` comment, or a `\'` in a string, swallowed
/// every statement after it.
pub fn split_statements_in(sql: &str, dialect: Dialect) -> Vec<StatementSpan> {
    match dialect {
        Dialect::Postgres | Dialect::Duckdb => split_statements(&line_ends(sql, dialect)),
        Dialect::Sqlite => split_statements(&sqlite_mask(sql)),
        Dialect::Mysql => split_statements(&mysql_mask(sql)),
    }
}

/// `sql` with every `;` inside a SQLite `[bracketed identifier]` replaced, and each
/// `/*` inside a comment -- SQLite's do not nest -- byte for byte, so `[a;b]` stays one
/// name and offsets into the mask are offsets into `sql`.
fn sqlite_mask(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = mask_comment(bytes, &mut out, i),
            b'[' => {
                // A name never spans lines; a `[` with no `]` on its line masks nothing.
                let close = bytes[i..]
                    .iter()
                    .position(|byte| *byte == b']' || *byte == b'\n')
                    .map(|at| i + at)
                    .filter(|at| bytes[*at] == b']');
                if let Some(close) = close {
                    for byte in &mut out[i..close] {
                        if *byte == b';' {
                            *byte = b'_';
                        }
                    }
                    i = close;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    // Only ASCII bytes were written, over ASCII ones.
    String::from_utf8(out).unwrap_or_else(|_| sql.to_string())
}

/// [`statement_at`] for `dialect`, as [`split_statements_in`] splits it.
pub fn statement_at_in(sql: &str, byte_index: usize, dialect: Dialect) -> Option<StatementSpan> {
    pick_statement(sql, split_statements_in(sql, dialect), byte_index)
}

/// `sql` as `dialect` ends its lines. Postgres and DuckDB end a `--` comment at a bare
/// carriage return as at a line feed: read to the next line feed instead, `select 1
/// --\r; drop table t` was one statement here and two on the server. Byte for byte, so
/// offsets into it are offsets into `sql`.
pub fn line_ends(sql: &str, dialect: Dialect) -> std::borrow::Cow<'_, str> {
    if matches!(dialect, Dialect::Postgres | Dialect::Duckdb) && sql.contains('\r') {
        std::borrow::Cow::Owned(sql.replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(sql)
    }
}

/// `sql` with MySQL's `#` comments blanked and every backslash-escaped character in a
/// string replaced, byte for byte, so offsets into it are offsets into `sql`.
pub(crate) fn mysql_mask(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\'
                        && quote != b'`'
                        && bytes.get(i + 1).is_some_and(u8::is_ascii)
                    {
                        out[i + 1] = b'_';
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = mask_comment(bytes, &mut out, i),
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    // Only ASCII bytes were written, over whole characters or over ASCII ones.
    String::from_utf8(out).unwrap_or_else(|_| sql.to_string())
}

fn pick_statement(
    sql: &str,
    statements: Vec<StatementSpan>,
    byte_index: usize,
) -> Option<StatementSpan> {
    statements
        .iter()
        .find(|span| byte_index >= span.byte_range.start && byte_index <= span.byte_range.end)
        .cloned()
        .or_else(|| {
            statements
                .iter()
                .find(|span| byte_index < span.byte_range.start)
                .cloned()
        })
        .or_else(|| {
            if byte_index <= sql.len() {
                statements.last().cloned()
            } else {
                None
            }
        })
}

fn classify_span(sql: &str, range: Range<usize>) -> StatementSpan {
    let body = sql[range.clone()].trim();
    let (effect, understood) = classify(body);
    StatementSpan {
        byte_range: range,
        effect,
        understood,
    }
}

fn classify(sql: &str) -> (StatementEffect, bool) {
    // Answered by Dexo from the catalog, never sent.
    if is_backslash_command(sql) {
        return (StatementEffect::ReadOnly, true);
    }
    let first = first_keyword(sql);
    match first.as_deref() {
        Some("SELECT" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE") => {
            (StatementEffect::ReadOnly, true)
        }
        Some("WITH") => classify_with(sql),
        Some("INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE") => {
            (StatementEffect::DataWrite, true)
        }
        Some("CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "GRANT" | "REVOKE" | "COMMENT") => {
            (StatementEffect::SchemaWrite, true)
        }
        Some("BEGIN" | "START" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE") => {
            (StatementEffect::DataWrite, true)
        }
        Some(_) | None => (StatementEffect::Unknown, false),
    }
}

/// A psql backslash command on its own line, a `;` at its end allowed out of psql habit.
/// One with a `;` anywhere else is not taken for one: `\dt ; DELETE …` on one line is
/// what a server would see as SQL.
pub fn is_backslash_command(sql: &str) -> bool {
    let sql = sql.trim();
    let sql = sql.strip_suffix(';').unwrap_or(sql);
    sql.starts_with('\\') && !sql.contains(';') && !sql.contains('\n')
}

fn classify_with(sql: &str) -> (StatementEffect, bool) {
    if let Some(rest) = skip_cte_prefix(sql) {
        let (effect, understood) = classify(rest);
        if effect == StatementEffect::ReadOnly && !understood {
            (StatementEffect::Unknown, false)
        } else {
            (effect, understood)
        }
    } else {
        (StatementEffect::Unknown, false)
    }
}

fn skip_cte_prefix(sql: &str) -> Option<&str> {
    let mut i = skip_ws(sql, 0);
    let with = first_keyword(&sql[i..])?;
    if with != "WITH" {
        return None;
    }
    i = skip_ws(sql, i + 4);
    if sql[i..].starts_with("RECURSIVE") || sql[i..].starts_with("recursive") {
        i = skip_ws(sql, i + 9);
    }
    loop {
        i = skip_ident(sql, i)?;
        i = skip_ws(sql, i);
        if eq_ignore(&sql[i..], "AS") {
            i = skip_ws(sql, i + 2);
        } else {
            return None;
        }
        if sql.as_bytes().get(i) != Some(&b'(') {
            return None;
        }
        i = skip_balanced_paren(sql, i)?;
        i = skip_ws(sql, i);
        if sql.as_bytes().get(i) == Some(&b',') {
            i = skip_ws(sql, i + 1);
            continue;
        }
        return Some(&sql[i..]);
    }
}

pub(crate) fn first_keyword(sql: &str) -> Option<String> {
    let i = skip_ws(sql, 0);
    let rest = &sql[i..];
    let ident = take_ident(rest)?;
    Some(ident.to_ascii_uppercase())
}

fn take_ident(sql: &str) -> Option<&str> {
    let mut chars = sql.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut end = first.len_utf8();
    for (idx, ch) in chars {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    Some(&sql[..end])
}

fn skip_ident(sql: &str, start: usize) -> Option<usize> {
    take_ident(&sql[start..]).map(|ident| start + ident.len())
}

fn eq_ignore(sql: &str, keyword: &str) -> bool {
    sql.len() >= keyword.len() && sql[..keyword.len()].eq_ignore_ascii_case(keyword)
}

fn skip_ws(sql: &str, mut i: usize) -> usize {
    let bytes = sql.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            _ => match skip_comment(sql, i) {
                Some(next) => i = next,
                None => break,
            },
        }
    }
    i
}

/// The end of the comment starting at `i`, or `None` if one does not start there. A
/// block comment holds the ones opened inside it, as in Postgres: ending at the first
/// `*/` split a nested comment at the `;` after it. MySQL and SQLite do not nest, so
/// their masks take the inner `/*` out first.
pub(crate) fn skip_comment(sql: &str, i: usize) -> Option<usize> {
    skip_comment_nesting(sql, i, true)
}

/// [`skip_comment`], with block comments nesting or not. Shared with the lexer, which
/// needs comments as tokens rather than as whitespace.
pub(crate) fn skip_comment_nesting(sql: &str, i: usize, nested: bool) -> Option<usize> {
    let bytes = sql.as_bytes();
    match bytes.get(i)? {
        b'-' if bytes.get(i + 1) == Some(&b'-') => {
            let mut end = i + 2;
            while end < bytes.len() && bytes[end] != b'\n' {
                end += 1;
            }
            Some(end)
        }
        b'/' if bytes.get(i + 1) == Some(&b'*') => {
            let mut end = i + 2;
            let mut depth = 1;
            while end + 1 < bytes.len() {
                if bytes[end] == b'*' && bytes[end + 1] == b'/' {
                    depth -= 1;
                    end += 2;
                    if depth == 0 {
                        return Some(end);
                    }
                } else if nested && bytes[end] == b'/' && bytes[end + 1] == b'*' {
                    depth += 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            Some(bytes.len())
        }
        _ => None,
    }
}

/// Blanks the `*` of each `/*` inside the block comment starting at `i`, in `out`, and
/// says where it ends: for MySQL and SQLite, whose comments end at the first `*/`.
fn mask_comment(bytes: &[u8], out: &mut [u8], mut i: usize) -> usize {
    i += 2;
    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            out[i + 1] = b' ';
        }
        i += 1;
    }
    i + 2
}

fn skip_atom(sql: &str, i: usize) -> usize {
    let bytes = sql.as_bytes();
    if i >= bytes.len() {
        return i;
    }
    match bytes[i] {
        b'\'' => skip_quote(sql, i, b'\'', false),
        b'"' => skip_quote(sql, i, b'"', false),
        b'`' => skip_quote(sql, i, b'`', false),
        b'$' => skip_dollar(sql, i).unwrap_or(i + 1),
        b'-' if bytes.get(i + 1) == Some(&b'-') => skip_ws(sql, i),
        b'/' if bytes.get(i + 1) == Some(&b'*') => skip_ws(sql, i),
        b';' => i,
        _ => i + 1,
    }
}

/// The end of the quoted run starting at `start`. A doubled quote is an escaped
/// quote everywhere; a backslash only in MySQL, so `escapes` says which.
pub(crate) fn skip_quote(sql: &str, start: usize, quote: u8, escapes: bool) -> usize {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if escapes && bytes[i] == b'\\' && quote == b'\'' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

pub(crate) fn skip_dollar(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'$') {
        return None;
    }
    let tag = &sql[start..=i];
    i += 1;
    if let Some(rel) = sql[i..].find(tag) {
        Some(i + rel + tag.len())
    } else {
        Some(bytes.len())
    }
}

fn skip_balanced_paren(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if bytes.get(start) != Some(&b'(') {
        return None;
    }
    let mut depth = 1;
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b';' && depth == 0 {
            break;
        }
        match bytes[i] {
            b'\'' => i = skip_quote(sql, i, b'\'', false),
            b'"' => i = skip_quote(sql, i, b'"', false),
            b'`' => i = skip_quote(sql, i, b'`', false),
            b'$' => i = skip_dollar(sql, i).unwrap_or(i + 1),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// The buffer cut at each statement's start, so every piece holds one statement with
/// the `;` and comments that follow it -- nothing in the buffer is left unparsed.
pub(crate) fn segments(sql: &str, statements: &[StatementSpan]) -> Vec<Range<usize>> {
    let mut cuts: Vec<usize> = statements
        .iter()
        .skip(1)
        .map(|statement| statement.byte_range.start)
        .collect();
    cuts.push(sql.len());
    let mut start = 0;
    cuts.into_iter()
        .map(|end| {
            let segment = start..end;
            start = end;
            segment
        })
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{StatementEffect, split_statements, split_statements_in, statement_at};

    /// A trigger's or routine's body is one statement with the CREATE, however many
    /// `;` it holds -- and nothing in it runs on its own.
    #[test]
    fn routine_bodies_stay_with_their_create() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON x BEGIN\n  DELETE FROM u;\n  UPDATE v SET x = CASE WHEN 1 THEN 2 END;\nEND;\nselect 1;";
        let texts: Vec<&str> = split_statements(sql)
            .iter()
            .map(|span| &sql[span.byte_range.clone()])
            .collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[0].ends_with("END"), "{texts:?}");
        assert_eq!(texts[1], "select 1");

        let mysql = "CREATE PROCEDURE p() BEGIN\n  IF 1 THEN SELECT 1; END IF;\n  WHILE 0 DO SELECT 2; END WHILE;\nEND;\nselect 3;";
        assert_eq!(
            split_statements_in(mysql, crate::Dialect::Mysql).len(),
            2,
            "{mysql}"
        );

        // A body of one statement, without BEGIN, is still the trigger's: run alone it
        // was an UPDATE with no WHERE.
        let bare = "CREATE TRIGGER t AFTER DELETE ON x FOR EACH ROW\nUPDATE stats SET n = n - 1;\nselect 6;";
        let texts: Vec<&str> = split_statements_in(bare, crate::Dialect::Mysql)
            .iter()
            .map(|span| &bare[span.byte_range.clone()])
            .collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[0].ends_with("n - 1"), "{texts:?}");
        // After the body's END, a statement on its own line is the next one.
        let closed = "CREATE TRIGGER t AFTER INSERT ON x BEGIN\n  DELETE FROM u;\nEND\nselect 7";
        assert_eq!(split_statements(closed).len(), 2);

        // A table may have a column called begin; that is not a block.
        let table = "CREATE TABLE log (\"begin\" int, note text); select 4;";
        assert_eq!(split_statements(table).len(), 2);

        let brackets = "select [a;b] from t; select 5";
        assert_eq!(
            split_statements_in(brackets, crate::Dialect::Sqlite).len(),
            2
        );
    }

    /// Postgres nests block comments: a `;` after an inner `*/` is still in the outer
    /// one. MySQL and SQLite end a comment at its first `*/`.
    #[test]
    fn nested_comments_hold_their_semicolons_where_they_nest() {
        let sql = "/* outer /* inner */ ; still comment */ select 1;\nselect 2";
        let texts: Vec<&str> = split_statements_in(sql, crate::Dialect::Postgres)
            .iter()
            .map(|span| &sql[span.byte_range.clone()])
            .collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts[0].ends_with("select 1"), "{texts:?}");
        let flat = "/* a /* b */ select 1; select 2";
        for dialect in [crate::Dialect::Mysql, crate::Dialect::Sqlite] {
            let texts: Vec<&str> = split_statements_in(flat, dialect)
                .iter()
                .map(|span| &flat[span.byte_range.clone()])
                .collect();
            assert_eq!(texts.len(), 2, "{dialect:?}: {texts:?}");
            assert!(texts[0].ends_with("select 1"), "{dialect:?}: {texts:?}");
        }
        let tokens = crate::tokenize("/* a /* b */ c */ x", crate::Dialect::Postgres);
        assert_eq!(tokens.len(), 2);
        let tokens = crate::tokenize("/* a /* b */ c */ x", crate::Dialect::Mysql);
        assert!(tokens.len() > 2);
    }

    /// Only what a CREATE makes says whether it is a routine: a table or a column
    /// named `event` or `trigger` is not one, and a Postgres routine's dollar-quoted
    /// body is closed at its end.
    #[test]
    fn a_routine_is_what_the_create_makes() {
        let count = |sql: &str, dialect| split_statements_in(sql, dialect).len();
        for sql in [
            "CREATE TABLE event (id int)\nSELECT 1",
            "CREATE TABLE logs (trigger_name text, function text)\nSELECT 1",
            "CREATE INDEX procedure ON t (x)\nSELECT 1",
            "CREATE VIEW event AS SELECT 1\nSELECT 2",
            "CREATE FUNCTION f() RETURNS int AS $$ select 1 $$ LANGUAGE sql\nSELECT 2",
            "CREATE OR REPLACE FUNCTION f() RETURNS trigger AS $body$\nBEGIN\n  UPDATE t SET x = 1;\n  RETURN NEW;\nEND\n$body$ LANGUAGE plpgsql\nSELECT 3",
        ] {
            assert_eq!(count(sql, crate::Dialect::Postgres), 2, "{sql}");
        }
        for sql in [
            "CREATE TABLE event (id int)\nSELECT 1",
            "CREATE DEFINER=root@localhost PROCEDURE p() BEGIN\n  SELECT 1;\n  SELECT 2;\nEND\nselect 3",
            "CREATE DEFINER = `root`@`%` TRIGGER t BEFORE INSERT ON x FOR EACH ROW BEGIN\n  SET NEW.a = 1;\nEND\nselect 3",
            "CREATE AGGREGATE FUNCTION agg(x INT) RETURNS INT BEGIN\n  DECLARE s INT;\n  RETURN s;\nEND\nselect 3",
        ] {
            assert_eq!(count(sql, crate::Dialect::Mysql), 2, "{sql}");
        }
    }

    /// A backslash command is its own statement, ending at its line, and a read; with
    /// a `;` on the line it is not one, and counts against the run like unknown SQL.
    #[test]
    fn backslash_commands_end_at_their_line() {
        let sql = "\\dt public.*\nselect 1;\n\\d orders\nselect 2";
        let spans = super::split_statements(sql);
        let texts: Vec<&str> = spans
            .iter()
            .map(|span| &sql[span.byte_range.clone()])
            .collect();
        assert_eq!(
            texts,
            ["\\dt public.*", "select 1", "\\d orders", "select 2"]
        );
        assert_eq!(spans[0].effect, super::StatementEffect::ReadOnly);
        assert!(super::is_backslash_command("\\x"));
        assert!(super::is_backslash_command("\\dt;"));
        assert!(!super::is_backslash_command("\\dt;;"));
        // A line of a statement that starts with a backslash, inside parentheses, is
        // still that statement's.
        let nested = "select (\n\\N\n)";
        assert_eq!(super::split_statements(nested).len(), 1);
        assert!(!super::is_backslash_command("\\dt ; delete from t"));
        assert!(!crate::is_read(
            "\\dt ; delete from t",
            crate::Dialect::Postgres
        ));
        assert!(crate::is_read("\\dt", crate::Dialect::Mysql));
        assert_eq!(crate::destructive("\\l", crate::Dialect::Sqlite), None);
    }
    use crate::Dialect;

    /// MySQL reads `#` as a comment and `\'` as a quote inside a string; the splitter
    /// read neither, so an apostrophe in a comment, or an escaped quote, swallowed
    /// every statement after it.
    #[test]
    fn mysql_hash_comments_and_backslash_escapes_split_like_mysql() {
        let texts = |sql: &str, dialect: Dialect| -> Vec<String> {
            split_statements_in(sql, dialect)
                .into_iter()
                .map(|span| sql[span.byte_range].trim().to_string())
                .collect()
        };
        assert_eq!(
            texts(
                "# drop the customer's old table\nDROP TABLE customers_old;\nselect 1",
                Dialect::Mysql
            ),
            ["DROP TABLE customers_old", "select 1"]
        );
        assert_eq!(
            texts(
                "SHOW TABLES LIKE 'o\\'%'; DELETE FROM orders",
                Dialect::Mysql
            ),
            ["SHOW TABLES LIKE 'o\\'%'", "DELETE FROM orders"]
        );
        assert_eq!(
            texts("select '#not a comment'", Dialect::Mysql),
            ["select '#not a comment'"]
        );
        assert_eq!(texts("select 1 # 2", Dialect::Postgres), ["select 1 # 2"]);
    }

    #[test]
    fn cte_delete_is_mutating() {
        let s = statement_at("WITH x AS (SELECT 1) DELETE FROM t WHERE id=1", 10).unwrap();
        assert_eq!(s.effect, StatementEffect::DataWrite);
    }

    #[test]
    fn semicolon_inside_string_does_not_split() {
        let spans = split_statements("select 'a;b'; select 2");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].effect, StatementEffect::ReadOnly);
    }

    #[test]
    fn dollar_quote_keeps_inner_semicolon() {
        let spans = split_statements("select $tag$ a;b $tag$; select 2");
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn caret_after_the_final_semicolon_uses_the_last_statement() {
        let sql = "select 1; select 2;";
        let span = statement_at(sql, sql.len()).unwrap();
        assert_eq!(&sql[span.byte_range], "select 2");
    }

    #[test]
    fn cursor_past_the_document_is_invalid() {
        assert!(statement_at("select 1;", 100).is_none());
    }

    fn texts(sql: &str) -> Vec<&str> {
        split_statements(sql)
            .into_iter()
            .map(|span| &sql[span.byte_range])
            .collect()
    }

    /// A statement missing its `;` used to swallow the next one, so Ctrl+Enter sent
    /// both and completion saw the neighbour's tables.
    #[test]
    fn a_statement_keyword_opening_a_line_starts_a_new_statement() {
        assert_eq!(
            texts("select * from\nselect id, name from orders where id = 1;"),
            ["select * from", "select id, name from orders where id = 1"]
        );
        assert_eq!(
            texts("select id from orders where\n\nselect name from customers;"),
            ["select id from orders where", "select name from customers"]
        );
        assert_eq!(
            texts("select id, name from orders where id = 1\nselect * from"),
            ["select id, name from orders where id = 1", "select * from"]
        );
        assert_eq!(
            texts("select * from customers -- all\nupdate orders set paid = true"),
            ["select * from customers", "update orders set paid = true"]
        );
        let sql = "select * from\nselect id from orders;";
        let span = statement_at(sql, sql.len() - 3).unwrap();
        assert_eq!(&sql[span.byte_range], "select id from orders");
    }

    /// Multi-line statements whose later lines open with a statement keyword that
    /// still belongs to them.
    #[test]
    fn statements_that_continue_across_lines_stay_whole() {
        let whole = [
            "insert into archive\nselect * from orders",
            "insert into archive (id, total)\nselect id, total from orders",
            "insert into t (a)\nwith x as (select 1)\nselect * from x",
            "with recent as (\n  select * from orders\n)\nselect * from recent",
            "with a as (select 1),\nb as (select 2)\nselect * from a, b",
            "create view v as\nselect * from orders",
            "create table t as\nselect * from orders",
            "select 1\nunion all\nselect 2",
            "select 1 union\nselect 2",
            "explain analyze\nselect * from orders",
            "insert into t values (1)\non duplicate key\nupdate total = 1",
            "merge into t using s on t.id = s.id\nwhen matched then\nupdate set total = s.total",
            "create trigger audit after\ninsert on orders for each row execute function log()",
            "create policy p on orders for\nselect using (true)",
            "select *\nfrom orders\nwhere id in (\n  select order_id from items\n)",
            "select id,\n  (select max(total) from orders) as top\nfrom customers",
            "grant\nselect on orders to reader",
            "select * from orders for\nupdate",
        ];
        for sql in whole {
            assert_eq!(texts(sql), [sql], "split: {sql:?}");
        }
    }

    #[test]
    fn unknown_syntax_is_never_readonly() {
        let s = statement_at("blargle frobnicate", 0).unwrap();
        assert_eq!(s.effect, StatementEffect::Unknown);
        assert!(!s.understood);
    }
}

#[cfg(test)]
mod proptests {
    use super::split_statements;

    proptest::proptest! {
        #[test]
        fn spans_cover_non_whitespace_without_overlap(sql in "[a-zA-Z0-9_ ;,'\"]{0,80}") {
            let spans = split_statements(&sql);
            let mut covered = vec![false; sql.len()];
            for span in &spans {
                for i in span.byte_range.clone() {
                    assert!(!covered[i], "overlap");
                    covered[i] = true;
                }
            }
            for (i, ch) in sql.char_indices() {
                if !ch.is_whitespace() && ch != ';' {
                    let byte_end = i + ch.len_utf8();
                    assert!(
                        covered[i..byte_end].iter().any(|c| *c) || sql[i..].trim().is_empty(),
                        "uncovered non-whitespace at {i}"
                    );
                }
            }
        }
    }
}
