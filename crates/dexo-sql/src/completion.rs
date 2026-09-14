use crate::context::{CursorContext, Intent, RowSource, analyze};
use crate::dialect::Dialect;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableInfo {
    pub qualified: String,
    pub schema: String,
    pub name: String,
    pub favorite: bool,
    pub recency: u64,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionInfo {
    pub name: String,
    pub signature: String,
}

pub trait Catalog {
    fn tables(&self) -> Vec<TableInfo>;
    fn functions(&self) -> Vec<FunctionInfo>;
}

#[derive(Clone, Debug, Default)]
pub struct FakeCatalog {
    tables: Vec<TableInfo>,
    functions: Vec<FunctionInfo>,
}

impl FakeCatalog {
    pub fn table(qualified: &str, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut catalog = Self::default();
        catalog.add_table(qualified, columns, false, 0);
        catalog
    }

    pub fn add_table(
        &mut self,
        qualified: &str,
        columns: impl IntoIterator<Item = impl Into<String>>,
        favorite: bool,
        recency: u64,
    ) {
        let (schema, name) = split_qualified(qualified);
        self.tables.push(TableInfo {
            qualified: qualified.to_string(),
            schema,
            name,
            favorite,
            recency,
            columns: columns.into_iter().map(Into::into).collect(),
        });
    }
}

impl Catalog for FakeCatalog {
    fn tables(&self) -> Vec<TableInfo> {
        self.tables.clone()
    }

    fn functions(&self) -> Vec<FunctionInfo> {
        self.functions.clone()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionKind {
    Keyword,
    Table,
    Column,
    Alias,
    Function,
    Snippet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub target_id: Option<String>,
    pub signature: Option<String>,
    rank: u8,
}

pub fn complete(
    sql: &str,
    cursor: usize,
    catalog: &dyn Catalog,
    dialect: Dialect,
) -> Vec<CompletionItem> {
    let cursor = cursor.min(sql.len());
    complete_with(&analyze(sql, cursor, dialect), catalog)
}

/// The same, for a caller that has already analysed the cursor. The editor needs the
/// context anyway -- to decide whether to open the popup at all -- and the two must not
/// arrive at different answers about what is being typed.
pub fn complete_with(context: &CursorContext, catalog: &dyn Catalog) -> Vec<CompletionItem> {
    if context.intent == Intent::Suppressed {
        return Vec::new();
    }
    if let Some(source) = context.target()
        && let Some(table) = resolve_source(source, catalog)
    {
        let token = context.prefix.to_ascii_lowercase();
        let mut items: Vec<_> = table
            .columns
            .iter()
            .filter(|column| token.is_empty() || column.to_ascii_lowercase().starts_with(&token))
            .map(|column| CompletionItem {
                label: column.clone(),
                kind: CompletionKind::Column,
                detail: Some(table.qualified.clone()),
                target_id: Some(format!("{}.{}", table.qualified, column)),
                signature: None,
                rank: 0,
            })
            .collect();
        items.sort_by(|a, b| a.label.cmp(&b.label));
        return items;
    }
    let token = context.prefix.to_ascii_lowercase();
    let mut items = Vec::new();
    for table in catalog.tables() {
        let rank = table_rank(&table);
        items.push(CompletionItem {
            label: table.name.clone(),
            kind: CompletionKind::Table,
            detail: Some(table.qualified.clone()),
            target_id: Some(table.qualified.clone()),
            signature: None,
            rank,
        });
    }
    for function in catalog.functions() {
        items.push(CompletionItem {
            label: function.name.clone(),
            kind: CompletionKind::Function,
            detail: None,
            target_id: Some(function.name.clone()),
            signature: Some(function.signature.clone()),
            rank: 4,
        });
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem {
            label: (*keyword).into(),
            kind: CompletionKind::Keyword,
            detail: None,
            target_id: None,
            signature: None,
            rank: 5,
        });
    }
    if !token.is_empty() {
        items.retain(|item| item.label.to_ascii_lowercase().starts_with(&token));
    }
    items.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.label.cmp(&b.label)));
    items
}

pub fn labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|item| item.label).collect()
}

fn table_rank(table: &TableInfo) -> u8 {
    if table.favorite {
        1
    } else if table.recency > 0 {
        2
    } else if table.schema == "public" {
        3
    } else {
        4
    }
}

pub fn current_token(prefix: &str) -> String {
    prefix
        .rsplit(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The catalog entry a row source names. Matching used to be `sql.contains("users u")`
/// over the whole lowercased buffer, which found `users_archive ua` and reached into
/// other statements; the name now comes from the parsed FROM list instead.
pub fn resolve_source(source: &RowSource, catalog: &dyn Catalog) -> Option<TableInfo> {
    let wanted = source.name.to_ascii_lowercase();
    let schema = source.schema.as_ref().map(|s| s.to_ascii_lowercase());
    catalog
        .tables()
        .into_iter()
        .filter(|table| table.name.to_ascii_lowercase() == wanted)
        .find(|table| match &schema {
            Some(schema) => table.schema.to_ascii_lowercase() == *schema,
            None => true,
        })
}

fn split_qualified(qualified: &str) -> (String, String) {
    let parts: Vec<&str> = qualified.split('.').collect();
    match parts.as_slice() {
        [_, schema, name] => ((*schema).into(), (*name).into()),
        [schema, name] => ((*schema).into(), (*name).into()),
        _ => (String::new(), qualified.to_string()),
    }
}

const KEYWORDS: &[&str] = &[
    "select", "from", "where", "join", "inner", "left", "right", "on", "group", "order", "limit",
    "insert", "update", "delete", "with", "values",
];

#[cfg(test)]
mod tests {
    use super::{FakeCatalog, complete, labels};
    use crate::dialect::Dialect;

    #[test]
    fn completes_columns_for_alias() {
        let catalog = FakeCatalog::table("public.users", ["id", "email"]);
        let items = complete(
            "select u. from public.users u",
            9,
            &catalog,
            Dialect::Postgres,
        );
        assert_eq!(labels(items), ["email", "id"]);
    }

    #[test]
    fn offline_catalog_is_deterministic() {
        let catalog = FakeCatalog::table("public.users", ["id", "email"]);
        let a = labels(complete(
            "select u. from public.users u",
            9,
            &catalog,
            Dialect::Postgres,
        ));
        let b = labels(complete(
            "select u. from public.users u",
            9,
            &catalog,
            Dialect::Mysql,
        ));
        assert_eq!(a, b);
        assert_eq!(a, ["email", "id"]);
    }
}
