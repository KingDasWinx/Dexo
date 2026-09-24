use std::ops::Range;

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
        match bytes[i] {
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
                if sql[i..next].contains('\n')
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
}

impl Scan {
    fn word(&mut self, word: &str) {
        let upper = word.to_ascii_uppercase();
        if self.first.is_none() {
            self.insert_awaits_rows = matches!(upper.as_str(), "INSERT" | "REPLACE");
            self.first = Some(upper.clone());
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
        self.last = Last::Word;
        self.last_word = upper;
    }

    fn starts_new(&self, word: &str) -> bool {
        let upper = word.to_ascii_uppercase();
        if self.first.is_none() || self.depth > 0 || !STARTERS.contains(&upper.as_str()) {
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
    let statements = split_statements(sql);
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

fn first_keyword(sql: &str) -> Option<String> {
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

/// The end of the comment starting at `i`, or `None` if one does not start there.
/// Shared with the lexer, which needs comments as tokens rather than as whitespace.
pub(crate) fn skip_comment(sql: &str, i: usize) -> Option<usize> {
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
            while end + 1 < bytes.len() && !(bytes[end] == b'*' && bytes[end + 1] == b'/') {
                end += 1;
            }
            Some((end + 2).min(bytes.len()))
        }
        _ => None,
    }
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
    use super::{StatementEffect, split_statements, statement_at};

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
