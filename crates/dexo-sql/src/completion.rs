use crate::context::{Confidence, CursorContext, Intent, RowSource, RowSourceKind, analyze};
use crate::dialect::Dialect;
use crate::rank;

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

/// A foreign key as declared on one table, pointing at another. This is what turns
/// "these two tables are in the same query" into an actual join condition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignKey {
    pub local_columns: Vec<String>,
    /// Qualified name of the table being referenced.
    pub referenced: String,
    pub referenced_columns: Vec<String>,
}

pub trait Catalog {
    fn tables(&self) -> Vec<TableInfo>;
    fn functions(&self) -> Vec<FunctionInfo>;

    /// Foreign keys declared on `qualified`. Defaulted: a catalog that does not know
    /// about constraints simply offers no join conditions.
    fn foreign_keys(&self, qualified: &str) -> Vec<ForeignKey> {
        let _ = qualified;
        Vec::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeCatalog {
    tables: Vec<TableInfo>,
    functions: Vec<FunctionInfo>,
    foreign_keys: Vec<(String, ForeignKey)>,
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

impl FakeCatalog {
    pub fn add_foreign_key(
        &mut self,
        table: &str,
        local: impl IntoIterator<Item = impl Into<String>>,
        referenced: &str,
        referenced_columns: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.foreign_keys.push((
            table.to_string(),
            ForeignKey {
                local_columns: local.into_iter().map(Into::into).collect(),
                referenced: referenced.to_string(),
                referenced_columns: referenced_columns.into_iter().map(Into::into).collect(),
            },
        ));
    }
}

impl Catalog for FakeCatalog {
    fn tables(&self) -> Vec<TableInfo> {
        self.tables.clone()
    }

    fn functions(&self) -> Vec<FunctionInfo> {
        self.functions.clone()
    }

    fn foreign_keys(&self, qualified: &str) -> Vec<ForeignKey> {
        self.foreign_keys
            .iter()
            .filter(|(table, _)| table.eq_ignore_ascii_case(qualified))
            .map(|(_, key)| key.clone())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
    /// How well this answers what was typed, and how much the position wanted it. Higher
    /// sorts first. It was a private rank of 1 to 5 that only knew whether a table was
    /// starred.
    pub score: i32,
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
    let prefix = &context.prefix;
    let mut items = Vec::new();
    match context.intent {
        Intent::Suppressed => return Vec::new(),
        Intent::AliasColumn => {
            if let Some(source) = context.target()
                && let Some(table) = resolve_source(source, catalog)
            {
                push_columns(&mut items, &table, prefix);
            }
        }
        Intent::JoinCondition => {
            push_join_conditions(&mut items, context, catalog, prefix);
            push_scope_columns(&mut items, context, catalog, prefix);
        }
        Intent::Column => {
            push_scope_columns(&mut items, context, catalog, prefix);
        }
        Intent::InsertColumn | Intent::UpdateColumn => {
            let target = context
                .row_sources
                .iter()
                .find(|source| source.kind == RowSourceKind::MutationTarget)
                .or_else(|| context.row_sources.first());
            if let Some(table) = target.and_then(|source| resolve_source(source, catalog)) {
                push_columns(&mut items, &table, prefix);
            }
        }
        Intent::Table | Intent::Schema => {
            // A qualifier here names a schema: `public.` narrows the list to that
            // schema's tables. If nothing matches it was not a schema after all, so
            // offer the whole list rather than an empty popup.
            let schema = context.qualifier.last().cloned();
            push_tables(&mut items, catalog, prefix, schema.as_deref());
            if items.is_empty() {
                push_tables(&mut items, catalog, prefix, None);
            }
        }
        Intent::Routine => push_functions(&mut items, catalog, prefix),
        Intent::Keyword => {
            // Nothing recognised, so nothing is ruled out.
            push_tables(&mut items, catalog, prefix, None);
            push_functions(&mut items, catalog, prefix);
            push_keywords(&mut items, prefix);
        }
    }
    // A recognised position that turned up nothing at all would leave the user staring at
    // an empty box; keywords are always a legitimate answer.
    if items.is_empty() && context.confidence != Confidence::High {
        push_tables(&mut items, catalog, prefix, None);
        push_keywords(&mut items, prefix);
    }
    rank::finish(items)
}

fn push_columns(items: &mut Vec<CompletionItem>, table: &TableInfo, prefix: &str) {
    for column in &table.columns {
        let Some(score) = rank::match_score(column, prefix) else {
            continue;
        };
        let boosts = rank::Boosts {
            key_column: rank::is_key_column(column),
            ..Default::default()
        };
        items.push(CompletionItem {
            label: column.clone(),
            kind: CompletionKind::Column,
            detail: Some(table.qualified.clone()),
            target_id: Some(format!("{}.{}", table.qualified, column)),
            signature: None,
            score: score + boosts.total(),
        });
    }
}

/// Whole join conditions, read off the foreign keys between the tables already in the
/// statement: after `join orders o on`, `o.user_id = u.id` is almost always the answer,
/// and it is the one thing here the database knows and the user would have to remember.
fn push_join_conditions(
    items: &mut Vec<CompletionItem>,
    context: &CursorContext,
    catalog: &dyn Catalog,
    prefix: &str,
) {
    for (left_index, left) in context.row_sources.iter().enumerate() {
        let Some(left_table) = resolve_source(left, catalog) else {
            continue;
        };
        for key in catalog.foreign_keys(&left_table.qualified) {
            for (right_index, right) in context.row_sources.iter().enumerate() {
                if right_index == left_index {
                    continue;
                }
                let Some(right_table) = resolve_source(right, catalog) else {
                    continue;
                };
                if !references(&key.referenced, &right_table) {
                    continue;
                }
                let condition = key
                    .local_columns
                    .iter()
                    .zip(key.referenced_columns.iter())
                    .map(|(local, referenced)| {
                        format!(
                            "{}.{local} = {}.{referenced}",
                            left.qualifier(),
                            right.qualifier()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" AND ");
                if condition.is_empty() {
                    continue;
                }
                // Matched against the whole condition, so typing either side finds it.
                let Some(score) = rank::match_score(&condition, prefix) else {
                    continue;
                };
                items.push(CompletionItem {
                    label: condition,
                    kind: CompletionKind::Snippet,
                    detail: Some(format!("foreign key · {}", left_table.qualified)),
                    target_id: Some(left_table.qualified.clone()),
                    signature: None,
                    score: score + rank::FOREIGN_KEY,
                });
            }
        }
    }
}

/// Whether a foreign key's referenced name is this table. The key names the table the
/// way the database does -- `public.brands` -- while the catalog qualifies it with the
/// database too, so the two are compared from the right.
fn references(referenced: &str, table: &TableInfo) -> bool {
    let mut parts = referenced.rsplit('.');
    let Some(name) = parts.next() else {
        return false;
    };
    if !name.eq_ignore_ascii_case(&table.name) {
        return false;
    }
    match parts.next() {
        Some(schema) => schema.eq_ignore_ascii_case(&table.schema),
        None => true,
    }
}

/// Columns of every table this statement has in scope, each labelled with where it came
/// from -- two tables in a join often share a column name.
fn push_scope_columns(
    items: &mut Vec<CompletionItem>,
    context: &CursorContext,
    catalog: &dyn Catalog,
    prefix: &str,
) {
    for source in &context.row_sources {
        let Some(table) = resolve_source(source, catalog) else {
            continue;
        };
        let before = items.len();
        push_columns(items, &table, prefix);
        if context.row_sources.len() > 1 {
            let qualifier = source.qualifier().to_string();
            for item in &mut items[before..] {
                item.detail = Some(format!("{qualifier} · {}", table.qualified));
            }
        }
    }
}

fn push_tables(
    items: &mut Vec<CompletionItem>,
    catalog: &dyn Catalog,
    prefix: &str,
    schema: Option<&str>,
) {
    for table in catalog.tables() {
        if let Some(schema) = schema
            && !table.schema.eq_ignore_ascii_case(schema)
        {
            continue;
        }
        let Some(score) = rank::match_score(&table.name, prefix) else {
            continue;
        };
        let boosts = rank::Boosts {
            favorite: table.favorite,
            recent: table.recency > 0,
            ..Default::default()
        };
        items.push(CompletionItem {
            label: table.name.clone(),
            kind: CompletionKind::Table,
            detail: Some(table.qualified.clone()),
            target_id: Some(table.qualified.clone()),
            signature: None,
            score: score + boosts.total(),
        });
    }
}

fn push_functions(items: &mut Vec<CompletionItem>, catalog: &dyn Catalog, prefix: &str) {
    for function in catalog.functions() {
        let Some(score) = rank::match_score(&function.name, prefix) else {
            continue;
        };
        items.push(CompletionItem {
            label: function.name.clone(),
            kind: CompletionKind::Function,
            detail: None,
            target_id: Some(function.name.clone()),
            signature: Some(function.signature.clone()),
            score,
        });
    }
}

fn push_keywords(items: &mut Vec<CompletionItem>, prefix: &str) {
    for keyword in KEYWORDS {
        let Some(score) = rank::match_score(keyword, prefix) else {
            continue;
        };
        items.push(CompletionItem {
            label: (*keyword).into(),
            kind: CompletionKind::Keyword,
            detail: None,
            target_id: None,
            signature: None,
            score,
        });
    }
}

pub fn labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|item| item.label).collect()
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

    /// `id` leads because a key column is what a statement usually reaches for, not
    /// because of the alphabet -- the order used to be alphabetical.
    #[test]
    fn completes_columns_for_alias() {
        let catalog = FakeCatalog::table("public.users", ["id", "email"]);
        let items = complete(
            "select u. from public.users u",
            9,
            &catalog,
            Dialect::Postgres,
        );
        assert_eq!(labels(items), ["id", "email"]);
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
        assert_eq!(a, ["id", "email"]);
    }
}
