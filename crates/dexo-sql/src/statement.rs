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

pub fn split_statements(sql: &str) -> Vec<StatementSpan> {
    let mut spans = Vec::new();
    let mut start = skip_ws(sql, 0);
    let mut i = start;
    let bytes = sql.as_bytes();
    while i < bytes.len() {
        i = skip_atom(sql, i);
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b';' {
            if start < i {
                spans.push(classify_span(sql, start..i));
            }
            i += 1;
            start = skip_ws(sql, i);
            i = start;
        }
    }
    if start < bytes.len() && !sql[start..].trim().is_empty() {
        spans.push(classify_span(sql, start..bytes.len()));
    }
    spans
}

pub fn statement_at(sql: &str, byte_index: usize) -> Option<StatementSpan> {
    split_statements(sql)
        .into_iter()
        .find(|span| byte_index >= span.byte_range.start && byte_index <= span.byte_range.end)
        .or_else(|| {
            split_statements(sql)
                .into_iter()
                .find(|span| byte_index < span.byte_range.start)
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
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => break,
        }
    }
    i
}

fn skip_atom(sql: &str, i: usize) -> usize {
    let bytes = sql.as_bytes();
    if i >= bytes.len() {
        return i;
    }
    match bytes[i] {
        b'\'' => skip_quote(sql, i, b'\''),
        b'"' => skip_quote(sql, i, b'"'),
        b'`' => skip_quote(sql, i, b'`'),
        b'$' => skip_dollar(sql, i).unwrap_or(i + 1),
        b'-' if bytes.get(i + 1) == Some(&b'-') => skip_ws(sql, i),
        b'/' if bytes.get(i + 1) == Some(&b'*') => skip_ws(sql, i),
        b';' => i,
        _ => i + 1,
    }
}

fn skip_quote(sql: &str, start: usize, quote: u8) -> usize {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
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

fn skip_dollar(sql: &str, start: usize) -> Option<usize> {
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
            b'\'' => i = skip_quote(sql, i, b'\''),
            b'"' => i = skip_quote(sql, i, b'"'),
            b'`' => i = skip_quote(sql, i, b'`'),
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
