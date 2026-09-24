use std::ops::Range;

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::dialect::Dialect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Highlight {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Identifier,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightSpan {
    pub kind: Highlight,
    pub text: String,
    pub byte_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct StatementRegion {
    pub byte_range: Range<usize>,
}

/// What one parse of the buffer yields. It carried a full `sqlparser` AST and a list of
/// error nodes too; nothing ever read either, and both were rebuilt for every character
/// typed -- the AST by a second, non-incremental parser.
#[derive(Debug)]
pub struct ParsedSql {
    pub highlights: Vec<HighlightSpan>,
    pub regions: Vec<StatementRegion>,
}

pub struct ParserService {
    dialect: Dialect,
    parser: Parser,
    query: Option<Query>,
}

impl ParserService {
    pub fn postgres() -> Self {
        Self::new(Dialect::Postgres)
    }

    pub fn mysql() -> Self {
        Self::new(Dialect::Mysql)
    }

    pub fn new(dialect: Dialect) -> Self {
        let language = tree_sitter_sequel::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("tree-sitter-sequel language");
        let query = Query::new(&language, tree_sitter_sequel::HIGHLIGHTS_QUERY).ok();
        Self {
            dialect,
            parser,
            query,
        }
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Parses each statement on its own. One tree for the whole buffer let an unfinished
    /// statement take over the next -- the grammar reserves no words, so after `from`
    /// the next statement's `select` and `from` read as table names and lost their
    /// colour, and recovery picked a different reading on every keystroke.
    pub fn parse(&mut self, sql: &str) -> ParsedSql {
        let statements = crate::statement::split_statements(sql);
        let mut highlights = Vec::new();
        for segment in crate::statement::segments(sql, &statements) {
            let text = &sql[segment.clone()];
            let tree = self
                .parser
                .parse(text, None)
                .expect("parser language is set");
            highlights.extend(self.highlights(text, &tree).into_iter().map(|mut span| {
                span.byte_range =
                    span.byte_range.start + segment.start..span.byte_range.end + segment.start;
                span
            }));
        }
        let highlights = mark_reserved_words(sql, self.dialect, highlights);
        let mut regions: Vec<StatementRegion> = statements
            .into_iter()
            .map(|statement| StatementRegion {
                byte_range: statement.byte_range,
            })
            .collect();
        if regions.is_empty() && !sql.trim().is_empty() {
            regions.push(StatementRegion {
                byte_range: 0..sql.len(),
            });
        }
        ParsedSql {
            highlights,
            regions,
        }
    }

    /// Kept for callers that pass the previous text; every parse is fresh now, one
    /// statement at a time.
    pub fn parse_edited(&mut self, _old: &str, new: &str) -> ParsedSql {
        self.parse(new)
    }

    fn highlights(&self, sql: &str, tree: &Tree) -> Vec<HighlightSpan> {
        let root = tree.root_node();
        match &self.query {
            Some(query) => highlights_from_query(query, root, sql),
            None => highlights_from_walk(root, sql),
        }
    }
}

/// The grammar gives up on constructs it does not know -- `distinct on`, `for update`,
/// `cast(x as t)` -- and colours nothing after them, reserved words included; it also
/// calls `null` a string. A reserved word outside a string or comment is a keyword
/// wherever it stands, so the lexer's reading wins over the grammar's for those. After a
/// dot it is a column name (`t.order`) and is left alone.
fn mark_reserved_words(
    sql: &str,
    dialect: Dialect,
    mut highlights: Vec<HighlightSpan>,
) -> Vec<HighlightSpan> {
    let tokens = crate::lex::tokenize(sql, dialect);
    let mut words: Vec<(Range<usize>, Highlight)> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let after_dot = index
            .checked_sub(1)
            .and_then(|previous| tokens.get(previous))
            .is_some_and(|previous| previous.text(sql) == ".");
        let text = token.text(sql);
        if token.kind == crate::lex::TokenKind::Word && !after_dot && crate::lex::is_reserved(text)
        {
            // `true` and `false` keep the boolean colour.
            let kind = if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
                Highlight::Number
            } else {
                Highlight::Keyword
            };
            words.push((token.span.clone(), kind));
        }
    }
    if words.is_empty() {
        return highlights;
    }
    // Every span on a reserved word gives way; the grammar sometimes puts two there.
    highlights.sort_by_key(|span| (span.byte_range.start, span.byte_range.end));
    let mut out = Vec::with_capacity(highlights.len() + words.len());
    let mut first = 0;
    for span in highlights {
        while first < words.len() && words[first].0.end <= span.byte_range.start {
            first += 1;
        }
        let overlaps = words[first..]
            .iter()
            .take_while(|(word, _)| word.start < span.byte_range.end)
            .any(|(word, _)| word.end > span.byte_range.start);
        if !overlaps {
            out.push(span);
        }
    }
    out.extend(words.into_iter().map(|(range, kind)| HighlightSpan {
        kind,
        text: sql[range.clone()].to_string(),
        byte_range: range,
    }));
    out.sort_by_key(|span| span.byte_range.start);
    out
}

fn highlights_from_query(query: &Query, root: tree_sitter::Node, sql: &str) -> Vec<HighlightSpan> {
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, sql.as_bytes());
    let mut out = Vec::new();
    while let Some((m, cap_ix)) = captures.next() {
        let capture = m.captures[*cap_ix];
        let name = query.capture_names()[capture.index as usize];
        let text = capture
            .node
            .utf8_text(sql.as_bytes())
            .unwrap_or("")
            .to_string();
        // The grammar marks numbers with a Lua pattern (`%d`) that Rust's regex never
        // matches, so every literal came through as a string and numbers were green.
        let kind = match highlight_kind(name) {
            Highlight::String if is_number(&text) => Highlight::Number,
            kind => kind,
        };
        out.push(HighlightSpan {
            kind,
            text,
            byte_range: capture.node.start_byte()..capture.node.end_byte(),
        });
    }
    out
}

fn highlights_from_walk(node: tree_sitter::Node, sql: &str) -> Vec<HighlightSpan> {
    let mut out = Vec::new();
    walk_highlights(node, sql, &mut out);
    out
}

fn walk_highlights(node: tree_sitter::Node, sql: &str, out: &mut Vec<HighlightSpan>) {
    let kind = highlight_from_node_kind(node.kind());
    if kind != Highlight::Other || node.kind().starts_with("keyword_") {
        let text = node.utf8_text(sql.as_bytes()).unwrap_or("").to_string();
        out.push(HighlightSpan {
            kind: if node.kind().starts_with("keyword_") {
                Highlight::Keyword
            } else {
                kind
            },
            text,
            byte_range: node.start_byte()..node.end_byte(),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_highlights(child, sql, out);
    }
}

fn highlight_kind(name: &str) -> Highlight {
    if name.starts_with("keyword") {
        Highlight::Keyword
    } else if name.starts_with("string") {
        Highlight::String
    } else if name.starts_with("comment") {
        Highlight::Comment
    } else if name.starts_with("number") || name.starts_with("float") {
        Highlight::Number
    } else if name.starts_with("function") {
        Highlight::Function
    } else if name == "conditional"
        || name == "storageclass"
        || name == "attribute"
        || name.starts_with("type.")
    {
        // CASE/WHEN, ASC/DESC, and built-in types are keywords to the reader.
        Highlight::Keyword
    } else if name == "boolean" {
        Highlight::Number
    } else if name == "variable" || name == "field" || name == "type" {
        Highlight::Identifier
    } else {
        Highlight::Other
    }
}

fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    !digits.is_empty()
        && digits.chars().any(|ch| ch.is_ascii_digit())
        && digits.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
}

fn highlight_from_node_kind(kind: &str) -> Highlight {
    if kind.starts_with("keyword_") {
        Highlight::Keyword
    } else if kind == "comment" || kind == "marginalia" {
        Highlight::Comment
    } else if kind == "literal" {
        Highlight::String
    } else {
        Highlight::Other
    }
}

#[cfg(test)]
mod tests {
    use super::{Highlight, ParserService};

    #[test]
    fn incomplete_select_still_highlights_keywords() {
        let parsed = ParserService::postgres().parse("select * fro");
        assert!(
            parsed
                .highlights
                .iter()
                .any(|h| h.kind == Highlight::Keyword && h.text.eq_ignore_ascii_case("select"))
        );
        assert!(!parsed.regions.is_empty());
    }

    /// The colour the editor paints at `byte`: the first span that covers it.
    fn kind_at(sql: &str, byte: usize) -> Option<Highlight> {
        ParserService::postgres()
            .parse(sql)
            .highlights
            .into_iter()
            .find(|span| span.byte_range.contains(&byte))
            .map(|span| span.kind)
    }

    /// An unfinished statement took the next one's keywords for table names, and they
    /// lost their colour.
    #[test]
    fn an_unfinished_statement_leaves_the_next_one_coloured() {
        for sql in [
            "select * from\nselect id, name from orders where id = 1;",
            "select id from orders where\n\nselect name from customers;",
            "select * from ;\nselect id from orders;",
            "select 1\nselect * from",
        ] {
            let complete = if sql.starts_with("select 1") {
                0
            } else {
                sql.rfind("select").unwrap()
            };
            let from = complete + sql[complete..].find("from").unwrap();
            assert_eq!(kind_at(sql, complete), Some(Highlight::Keyword), "{sql:?}");
            assert_eq!(kind_at(sql, from), Some(Highlight::Keyword), "{sql:?}");
        }
    }

    #[test]
    fn numbers_and_case_words_get_their_own_colours() {
        let sql = "select 42, 'x', case when true then 1 end from t order by id desc";
        assert_eq!(
            kind_at(sql, sql.find("42").unwrap()),
            Some(Highlight::Number)
        );
        assert_eq!(
            kind_at(sql, sql.find("'x'").unwrap()),
            Some(Highlight::String)
        );
        assert_eq!(
            kind_at(sql, sql.find("case").unwrap()),
            Some(Highlight::Keyword)
        );
        assert_eq!(
            kind_at(sql, sql.find("when").unwrap()),
            Some(Highlight::Keyword)
        );
        assert_eq!(
            kind_at(sql, sql.find("desc").unwrap()),
            Some(Highlight::Keyword)
        );
    }

    #[test]
    fn comments_between_statements_keep_their_colour() {
        let sql = "select 1;\n-- totals\nselect 2;";
        assert_eq!(
            kind_at(sql, sql.find("--").unwrap()),
            Some(Highlight::Comment)
        );
    }

    /// The grammar knows no `distinct on`, `for update` or `cast(x as t)`, and coloured
    /// nothing from there on; it also called `null` a string.
    #[test]
    fn reserved_words_are_keywords_where_the_grammar_gives_up() {
        for (sql, word) in [
            ("select distinct on (a) * from t order by a desc", "from"),
            ("select distinct on (a) * from t order by a desc", "order"),
            ("select distinct on (a) * from t order by a desc", "desc"),
            ("select * from t for update", "update"),
            ("select cast(a as int) from t", "cast"),
            ("select * from t where a is not null", "null"),
            ("select * from t having count(*) > 1 order by a", "having"),
        ] {
            let at = sql.find(&format!(" {word}")).unwrap() + 1;
            assert_eq!(
                kind_at(sql, at),
                Some(Highlight::Keyword),
                "{word} in {sql}"
            );
        }
        // A column that happens to be called `order` keeps its own colour after a dot,
        // and a boolean stays a boolean.
        let sql = "select t.order from t where ok = true";
        assert_ne!(
            kind_at(sql, sql.find("order").unwrap()),
            Some(Highlight::Keyword)
        );
        assert_eq!(
            kind_at(sql, sql.find("true").unwrap()),
            Some(Highlight::Number)
        );
    }

    #[test]
    fn parser_corpus_never_panics() {
        let fixtures = [
            "select 1",
            "select * from t",
            "select * fro",
            "insert into t values (1)",
            "$$$",
            "/* comment",
            "select 'ação'",
            "WITH x AS (SELECT 1) DELETE FROM t",
            "",
            "select `id` from `users`",
        ];
        for sql in fixtures {
            let _ = ParserService::postgres().parse(sql);
            let _ = ParserService::mysql().parse(sql);
        }
    }
}
