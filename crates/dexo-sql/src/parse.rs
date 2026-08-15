use std::ops::Range;

use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
use tree_sitter::{InputEdit, Parser, Query, QueryCursor, StreamingIterator, Tree};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDiagnostic {
    pub message: String,
    pub byte_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct StatementRegion {
    pub byte_range: Range<usize>,
}

#[derive(Debug)]
pub struct ParsedSql {
    pub highlights: Vec<HighlightSpan>,
    pub regions: Vec<StatementRegion>,
    pub ast: Option<Vec<sqlparser::ast::Statement>>,
    pub diagnostics: Vec<LocalDiagnostic>,
}

pub struct ParserService {
    dialect: Dialect,
    parser: Parser,
    query: Option<Query>,
    tree: Option<Tree>,
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
            tree: None,
        }
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub fn parse(&mut self, sql: &str) -> ParsedSql {
        let tree = self
            .parser
            .parse(sql, self.tree.as_ref())
            .expect("parser language is set");
        let parsed = self.analyze(sql, &tree);
        self.tree = Some(tree);
        parsed
    }

    pub fn apply_edit(&mut self, edit: InputEdit) {
        if let Some(tree) = &mut self.tree {
            tree.edit(&edit);
        }
    }

    pub fn parse_edited(&mut self, old: &str, new: &str) -> ParsedSql {
        self.apply_edit(InputEdit {
            start_byte: 0,
            old_end_byte: old.len(),
            new_end_byte: new.len(),
            start_position: tree_sitter::Point::new(0, 0),
            old_end_position: end_point(old),
            new_end_position: end_point(new),
        });
        self.parse(new)
    }

    fn analyze(&self, sql: &str, tree: &Tree) -> ParsedSql {
        let root = tree.root_node();
        let highlights = if let Some(query) = &self.query {
            highlights_from_query(query, root, sql)
        } else {
            highlights_from_walk(root, sql)
        };
        let mut regions = Vec::new();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            if child.kind() == "comment" || child.kind() == "marginalia" {
                continue;
            }
            regions.push(StatementRegion {
                byte_range: child.start_byte()..child.end_byte(),
            });
        }
        if regions.is_empty() && !sql.trim().is_empty() {
            regions.push(StatementRegion {
                byte_range: 0..sql.len(),
            });
        }
        let mut diagnostics = Vec::new();
        collect_errors(root, &mut diagnostics);
        let ast = parse_ast(self.dialect, sql);
        ParsedSql {
            highlights,
            regions,
            ast,
            diagnostics,
        }
    }
}

fn parse_ast(dialect: Dialect, sql: &str) -> Option<Vec<sqlparser::ast::Statement>> {
    match dialect {
        Dialect::Postgres => sqlparser::parser::Parser::parse_sql(&PostgreSqlDialect {}, sql).ok(),
        Dialect::Mysql => sqlparser::parser::Parser::parse_sql(&MySqlDialect {}, sql).ok(),
    }
}

fn highlights_from_query(query: &Query, root: tree_sitter::Node, sql: &str) -> Vec<HighlightSpan> {
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, sql.as_bytes());
    let mut out = Vec::new();
    while let Some((m, cap_ix)) = captures.next() {
        let capture = m.captures[*cap_ix];
        let name = query.capture_names()[capture.index as usize];
        let kind = highlight_kind(name);
        let text = capture
            .node
            .utf8_text(sql.as_bytes())
            .unwrap_or("")
            .to_string();
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
    } else if name == "variable" || name == "field" || name == "type" {
        Highlight::Identifier
    } else {
        Highlight::Other
    }
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

fn collect_errors(node: tree_sitter::Node, out: &mut Vec<LocalDiagnostic>) {
    if node.is_error() || node.is_missing() {
        out.push(LocalDiagnostic {
            message: "local parse error".into(),
            byte_range: node.start_byte()..node.end_byte().max(node.start_byte()),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, out);
    }
}

fn end_point(text: &str) -> tree_sitter::Point {
    let mut row = 0;
    let mut last = 0;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            row += 1;
            last = index + 1;
        }
    }
    tree_sitter::Point::new(row, text.len() - last)
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
