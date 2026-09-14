use std::ops::Range;

use crate::dialect::Dialect;
use crate::statement::{skip_comment, skip_dollar, skip_quote};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    /// A bare word: an identifier or a keyword. Which one it is depends on where it
    /// sits, so the lexer does not decide.
    Word,
    /// An identifier the user quoted. It is never a keyword, however it is spelled.
    QuotedIdent,
    String,
    Number,
    Comment,
    /// `:name`, `$1`, `?`, `@name` — a placeholder, not an identifier.
    Param,
    Punct,
    Operator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
    /// Parenthesis nesting the token sits in. `(` and its matching `)` both carry the
    /// outer level. This is how a subquery gets scoped without a grammar.
    pub depth: u16,
    /// False for a quote or block comment that ran to the end of the buffer. While
    /// someone is typing, the last one usually did.
    pub closed: bool,
}

impl Token {
    pub fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.span.clone()]
    }

    /// The identifier this token names, with quoting removed. `None` for anything that
    /// cannot be one.
    pub fn ident<'a>(&self, sql: &'a str) -> Option<&'a str> {
        match self.kind {
            TokenKind::Word => Some(self.text(sql)),
            TokenKind::QuotedIdent => {
                let text = self.text(sql);
                let mut chars = text.chars();
                chars.next();
                if self.closed {
                    chars.next_back();
                }
                Some(chars.as_str())
            }
            _ => None,
        }
    }

    /// Whether this is the bare word `keyword`, case-insensitively. A quoted identifier
    /// never matches: a table the user wrote as `"from"` is not the FROM keyword.
    pub fn is_keyword(&self, sql: &str, keyword: &str) -> bool {
        self.kind == TokenKind::Word && self.text(sql).eq_ignore_ascii_case(keyword)
    }
}

/// Splits `sql` into tokens, keeping comments and strings rather than skipping them —
/// the cursor can be inside one, and that is the difference between offering a
/// completion and staying out of the way.
pub fn tokenize(sql: &str, dialect: Dialect) -> Vec<Token> {
    let escapes = dialect == Dialect::Mysql;
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut depth: u16 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if let Some(end) = skip_comment(sql, i) {
            let closed = !sql[i..end].starts_with("/*") || sql[i..end].ends_with("*/");
            tokens.push(Token {
                kind: TokenKind::Comment,
                span: i..end,
                depth,
                closed,
            });
            i = end;
            continue;
        }
        let (kind, end, closed) = match byte {
            b'\'' => {
                let end = skip_quote(sql, i, b'\'', escapes);
                (
                    TokenKind::String,
                    end,
                    sql[i..end].len() > 1 && bytes[end - 1] == b'\'',
                )
            }
            b'"' => {
                let end = skip_quote(sql, i, b'"', false);
                (
                    TokenKind::QuotedIdent,
                    end,
                    sql[i..end].len() > 1 && bytes[end - 1] == b'"',
                )
            }
            b'`' => {
                let end = skip_quote(sql, i, b'`', false);
                (
                    TokenKind::QuotedIdent,
                    end,
                    sql[i..end].len() > 1 && bytes[end - 1] == b'`',
                )
            }
            b'[' if dialect == Dialect::Mysql => (TokenKind::Punct, i + 1, true),
            b'$' => match skip_dollar(sql, i) {
                Some(end) => (TokenKind::String, end, sql[i..end].ends_with('$')),
                None => {
                    let end = take_while(bytes, i + 1, |b| b.is_ascii_digit());
                    (TokenKind::Param, end, true)
                }
            },
            b':' | b'@' | b'?' | b'#' => {
                let end = take_while(bytes, i + 1, is_ident_byte);
                (TokenKind::Param, end.max(i + 1), true)
            }
            b'(' => {
                let token = Token {
                    kind: TokenKind::Punct,
                    span: i..i + 1,
                    depth,
                    closed: true,
                };
                tokens.push(token);
                depth = depth.saturating_add(1);
                i += 1;
                continue;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                (TokenKind::Punct, i + 1, true)
            }
            b',' | b';' | b'.' => (TokenKind::Punct, i + 1, true),
            b'0'..=b'9' => {
                let end = take_while(bytes, i, |b| b.is_ascii_digit() || b == b'.');
                (TokenKind::Number, end, true)
            }
            b if is_ident_start(b) => {
                let end = take_while(bytes, i, is_ident_byte);
                (TokenKind::Word, end, true)
            }
            _ => (TokenKind::Operator, i + 1, true),
        };
        tokens.push(Token {
            kind,
            span: i..end,
            depth,
            closed,
        });
        i = end.max(i + 1);
    }
    tokens
}

/// Whether `cursor` sits inside a string literal or a comment, where a completion popup
/// has no business opening.
pub fn suppressed_at(sql: &str, cursor: usize, dialect: Dialect) -> bool {
    let cursor = cursor.min(sql.len());
    tokenize(sql, dialect).iter().any(|token| {
        matches!(token.kind, TokenKind::String | TokenKind::Comment)
            && cursor > token.span.start
            // An unterminated run swallows the rest of the buffer, so the cursor is
            // inside it even when it sits at the very end.
            && (cursor < token.span.end || !token.closed)
    })
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn take_while(bytes: &[u8], from: usize, predicate: impl Fn(u8) -> bool) -> usize {
    let mut end = from;
    while end < bytes.len() && predicate(bytes[end]) {
        end += 1;
    }
    end
}
