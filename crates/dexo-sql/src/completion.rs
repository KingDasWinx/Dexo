use std::collections::HashSet;

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

    /// The table a statement names, matched without regard to case. Defaulted to a scan;
    /// a catalog that holds a whole database should answer from an index, because this
    /// runs for every table in the statement on every character typed.
    fn table(&self, schema: Option<&str>, name: &str) -> Option<TableInfo> {
        self.tables().into_iter().find(|table| {
            table.name.eq_ignore_ascii_case(name)
                && schema.is_none_or(|schema| table.schema.eq_ignore_ascii_case(schema))
        })
    }

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
    /// What goes before the name when the item is accepted: the schema of a table the
    /// search path may not reach (`reporting.daily`), the alias of a column two tables of
    /// the statement have (`c.id`). Written out, the bare name ran into "relation does
    /// not exist" and "column reference is ambiguous".
    pub qualifier: Option<String>,
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
            push_aliases(&mut items, context, prefix);
        }
        // An expression can be any of these; the ranking, not the list, says which the
        // position most likely wants.
        Intent::Column => {
            push_scope_columns(&mut items, context, catalog, prefix);
            push_aliases(&mut items, context, prefix);
            // No FROM yet: `select users.` is as likely as a function.
            if context.row_sources.is_empty() {
                push_tables(&mut items, catalog, prefix, None, context, &HashSet::new());
            }
            push_functions(&mut items, catalog, prefix);
            push_builtins(&mut items, prefix);
            push_keywords(&mut items, KEYWORDS, prefix);
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
        Intent::Table => {
            // A qualifier here names a schema: `public.` narrows the list to that
            // schema's tables. If nothing matches it was not a schema after all, so
            // offer the whole list rather than an empty popup.
            let schema = context.qualifier.last().cloned();
            let related = related_tables(context, catalog);
            push_tables(
                &mut items,
                catalog,
                prefix,
                schema.as_deref(),
                context,
                &related,
            );
            if items.is_empty() {
                push_tables(&mut items, catalog, prefix, None, context, &related);
            }
        }
        // A qualifier the statement does not declare: `select venda.` typed before the
        // FROM, or a table's own name where it goes by an alias. It is a table's
        // columns if a table goes by that name, and a schema's tables if a schema does.
        Intent::Schema => {
            if let Some(table) = named_table(&context.qualifier, catalog) {
                push_columns(&mut items, &table, prefix);
            }
            push_tables(
                &mut items,
                catalog,
                prefix,
                context.qualifier.last().map(String::as_str),
                context,
                &HashSet::new(),
            );
        }
        Intent::Routine => push_functions(&mut items, catalog, prefix),
        // A name being made up, or the next clause: `from venda or` is ORDER BY on its
        // way. Only clauses the letters start, so an alias rarely brings any up.
        Intent::Alias => {
            push_keywords(&mut items, AFTER_TABLE, prefix);
            let typed = prefix.to_ascii_lowercase();
            items.retain(|item| item.label.to_ascii_lowercase().starts_with(&typed));
        }
        Intent::Keyword => {
            // Nothing recognised, so nothing is ruled out.
            push_tables(&mut items, catalog, prefix, None, context, &HashSet::new());
            push_functions(&mut items, catalog, prefix);
            push_builtins(&mut items, prefix);
            push_keywords(&mut items, KEYWORDS, prefix);
        }
    }
    // A recognised position that turned up nothing at all would leave the user staring at
    // an empty box; keywords are always a legitimate answer. Not after a dot, though:
    // only a member of what precedes it can go there.
    if items.is_empty() && context.confidence != Confidence::High && context.qualifier.is_empty() {
        push_tables(&mut items, catalog, prefix, None, context, &HashSet::new());
        push_keywords(&mut items, KEYWORDS, prefix);
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
            qualifier: None,
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
                    qualifier: None,
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
    let first = items.len();
    let mut owners = Vec::new();
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
            owners.extend(std::iter::repeat_n(qualifier, items.len() - before));
        }
    }
    // A column two of the tables have is accepted with its table's name or alias: bare,
    // the server answers that the reference is ambiguous.
    for (index, qualifier) in owners.into_iter().enumerate() {
        let name = items[first + index].label.to_ascii_lowercase();
        let shared = items[first..]
            .iter()
            .filter(|other| other.label.to_ascii_lowercase() == name)
            .count()
            > 1;
        if shared {
            items[first + index].qualifier = Some(qualifier);
        }
    }
}

/// The names the statement's tables go by, so `u` is on offer before `u.` is typed.
fn push_aliases(items: &mut Vec<CompletionItem>, context: &CursorContext, prefix: &str) {
    for source in &context.row_sources {
        let label = source.qualifier();
        if label.is_empty() {
            continue;
        }
        let Some(score) = rank::match_score(label, prefix) else {
            continue;
        };
        items.push(CompletionItem {
            label: label.to_string(),
            kind: CompletionKind::Alias,
            detail: Some(source.qualified()),
            target_id: None,
            signature: None,
            qualifier: None,
            score,
        });
    }
}

/// Schemas a name reaches without being qualified: Postgres's `public`, SQLite's and
/// DuckDB's `main`, SQL Server's `dbo`, and MySQL's tables, which have no schema of
/// their own. A table in any other is written with its schema.
fn is_default_schema(schema: &str) -> bool {
    schema.is_empty()
        || ["public", "main", "dbo"]
            .iter()
            .any(|default| schema.eq_ignore_ascii_case(default))
}

/// The tables a foreign key ties to one the statement already reads, either way round,
/// by lowercased qualified name: after `from orders o join`, `customers` and
/// `order_items` are the likely ones.
fn related_tables(context: &CursorContext, catalog: &dyn Catalog) -> HashSet<String> {
    let mut related = HashSet::new();
    let sources: Vec<TableInfo> = context
        .row_sources
        .iter()
        .filter_map(|source| resolve_source(source, catalog))
        .collect();
    if sources.is_empty() {
        return related;
    }
    let tables = catalog.tables();
    for table in &tables {
        let is_source = sources
            .iter()
            .any(|source| source.qualified.eq_ignore_ascii_case(&table.qualified));
        for key in catalog.foreign_keys(&table.qualified) {
            if is_source {
                // The statement's table points at these.
                for target in tables.iter().filter(|t| references(&key.referenced, t)) {
                    related.insert(target.qualified.to_ascii_lowercase());
                }
            } else if sources
                .iter()
                .any(|source| references(&key.referenced, source))
            {
                // This one points at a table the statement reads.
                related.insert(table.qualified.to_ascii_lowercase());
            }
        }
    }
    related
}

fn push_tables(
    items: &mut Vec<CompletionItem>,
    catalog: &dyn Catalog,
    prefix: &str,
    schema: Option<&str>,
    context: &CursorContext,
    related: &HashSet<String>,
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
            related: related.contains(&table.qualified.to_ascii_lowercase()),
            other_schema: !is_default_schema(&table.schema),
            ..Default::default()
        };
        // Typed after `reporting.`, the schema is already there.
        let qualifier = (context.qualifier.is_empty() && !is_default_schema(&table.schema))
            .then(|| table.schema.clone());
        items.push(CompletionItem {
            label: table.name.clone(),
            kind: CompletionKind::Table,
            detail: Some(table.qualified.clone()),
            target_id: Some(table.qualified.clone()),
            signature: None,
            qualifier,
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
            qualifier: None,
            score,
        });
    }
}

/// Functions every database ships, which no catalog lists.
fn push_builtins(items: &mut Vec<CompletionItem>, prefix: &str) {
    let upper = shouted(prefix);
    for (name, signature) in BUILTINS {
        let Some(score) = rank::match_score(name, prefix) else {
            continue;
        };
        items.push(CompletionItem {
            label: cased(name, upper),
            kind: CompletionKind::Function,
            detail: Some("built-in".into()),
            target_id: None,
            signature: Some((*signature).into()),
            qualifier: None,
            score,
        });
    }
}

/// Keywords always go in capitals, however they were typed: the editor writes them that
/// way.
fn push_keywords(items: &mut Vec<CompletionItem>, words: &[&str], prefix: &str) {
    for keyword in words {
        let Some(score) = rank::match_score(keyword, prefix) else {
            continue;
        };
        items.push(CompletionItem {
            label: keyword.to_ascii_uppercase(),
            kind: CompletionKind::Keyword,
            detail: None,
            target_id: None,
            signature: None,
            qualifier: None,
            score,
        });
    }
}

/// Whether the user is writing keywords in capitals: `SEL` should become `SELECT`.
fn shouted(prefix: &str) -> bool {
    prefix.chars().any(|ch| ch.is_ascii_uppercase())
        && !prefix.chars().any(|ch| ch.is_ascii_lowercase())
}

fn cased(word: &str, upper: bool) -> String {
    if upper {
        word.to_ascii_uppercase()
    } else {
        word.to_string()
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
    catalog.table(source.schema.as_deref(), &source.name)
}

/// The table a dotted qualifier spells out: `venda`, `public.venda`, `db.public.venda`.
pub fn named_table(qualifier: &[String], catalog: &dyn Catalog) -> Option<TableInfo> {
    let (name, rest) = qualifier.split_last()?;
    catalog.table(rest.last().map(String::as_str), name)
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
    "select",
    "from",
    "where",
    "and",
    "or",
    "not",
    "in",
    "is",
    "null",
    "is null",
    "is not null",
    "like",
    "ilike",
    "between",
    "exists",
    "as",
    "distinct",
    "all",
    "any",
    "join",
    "inner join",
    "left join",
    "right join",
    "full join",
    "cross join",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "natural",
    "lateral",
    "on",
    "using",
    "group by",
    "having",
    "order by",
    "partition by",
    "asc",
    "desc",
    "nulls first",
    "nulls last",
    "limit",
    "offset",
    "fetch",
    "union",
    "union all",
    "intersect",
    "except",
    "case",
    "when",
    "then",
    "else",
    "end",
    "insert into",
    "values",
    "update",
    "set",
    "delete from",
    "returning",
    "with",
    "recursive",
    "over",
    "window",
    "filter",
    "true",
    "false",
    "default",
    "create",
    "table",
    "view",
    "index",
    "alter",
    "add",
    "column",
    "drop",
    "truncate",
    "primary key",
    "foreign key",
    "references",
    "unique",
    "check",
    "constraint",
    "not null",
    "begin",
    "commit",
    "rollback",
    "explain",
    "analyze",
];

/// What can follow a table in a FROM list, besides its alias.
const AFTER_TABLE: &[&str] = &[
    "where",
    "join",
    "inner join",
    "left join",
    "right join",
    "full join",
    "cross join",
    "on",
    "using",
    "as",
    "group by",
    "order by",
    "having",
    "limit",
    "offset",
    "union",
    "union all",
    "set",
    "values",
    "returning",
    "window",
];

/// Name and signature. Common to PostgreSQL and MySQL, or so widely used in one that
/// leaving it out would be the thing people notice.
const BUILTINS: &[(&str, &str)] = &[
    ("count", "count(expr)"),
    ("sum", "sum(expr)"),
    ("avg", "avg(expr)"),
    ("min", "min(expr)"),
    ("max", "max(expr)"),
    ("coalesce", "coalesce(value, ...)"),
    ("nullif", "nullif(a, b)"),
    ("greatest", "greatest(value, ...)"),
    ("least", "least(value, ...)"),
    ("cast", "cast(expr as type)"),
    ("lower", "lower(text)"),
    ("upper", "upper(text)"),
    ("length", "length(text)"),
    ("substring", "substring(text, start, length)"),
    ("trim", "trim(text)"),
    ("ltrim", "ltrim(text)"),
    ("rtrim", "rtrim(text)"),
    ("concat", "concat(text, ...)"),
    ("replace", "replace(text, from, to)"),
    ("position", "position(sub in text)"),
    ("left", "left(text, n)"),
    ("right", "right(text, n)"),
    ("round", "round(number, digits)"),
    ("floor", "floor(number)"),
    ("ceil", "ceil(number)"),
    ("abs", "abs(number)"),
    ("mod", "mod(a, b)"),
    ("now", "now()"),
    ("current_date", "current_date"),
    ("current_timestamp", "current_timestamp"),
    ("extract", "extract(field from source)"),
    ("date_trunc", "date_trunc(field, source)"),
    ("to_char", "to_char(value, format)"),
    ("date_format", "date_format(date, format)"),
    ("ifnull", "ifnull(value, fallback)"),
    ("string_agg", "string_agg(expr, delimiter)"),
    ("group_concat", "group_concat(expr)"),
    ("array_agg", "array_agg(expr)"),
    ("json_agg", "json_agg(expr)"),
    ("row_number", "row_number() over (...)"),
    ("rank", "rank() over (...)"),
    ("dense_rank", "dense_rank() over (...)"),
    ("lag", "lag(expr) over (...)"),
    ("lead", "lead(expr) over (...)"),
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

    fn shop() -> FakeCatalog {
        let mut catalog = FakeCatalog::default();
        catalog.add_table("reporting.daily", ["day", "total"], false, 0);
        catalog.add_table("public.products", ["id", "name"], false, 0);
        catalog.add_table("public.customers", ["id", "name"], false, 0);
        catalog.add_table("public.orders", ["id", "customer_id"], false, 0);
        catalog.add_foreign_key("public.orders", ["customer_id"], "public.customers", ["id"]);
        catalog
    }

    /// `daily` lives in `reporting`: written bare, the accepted query did not run.
    #[test]
    fn a_table_outside_the_default_schema_is_accepted_with_its_schema() {
        let items = complete("select * from dai", 17, &shop(), Dialect::Postgres);
        assert_eq!(items[0].label, "daily");
        assert_eq!(items[0].qualifier.as_deref(), Some("reporting"));
        let items = complete("select * from cust", 18, &shop(), Dialect::Postgres);
        assert_eq!(items[0].label, "customers");
        assert_eq!(items[0].qualifier, None, "public is reached bare");
        // Typed after the schema, nothing is added a second time.
        let items = complete(
            "select * from reporting.dai",
            27,
            &shop(),
            Dialect::Postgres,
        );
        assert_eq!(items[0].label, "daily");
        assert_eq!(items[0].qualifier, None);
    }

    /// After a join the tables a foreign key ties to the one read lead, and the schema
    /// off the search path comes after the ones in it.
    #[test]
    fn a_join_lists_the_tables_a_foreign_key_ties_first() {
        let sql = "select * from orders o join ";
        let items = complete(sql, sql.len(), &shop(), Dialect::Postgres);
        let tables: Vec<&str> = items
            .iter()
            .filter(|item| item.kind == super::CompletionKind::Table)
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(tables[0], "customers", "{tables:?}");
        assert_eq!(*tables.last().unwrap(), "daily", "{tables:?}");
    }

    /// `id` is in both tables of the join: bare, the server calls it ambiguous.
    #[test]
    fn a_column_two_tables_have_is_accepted_with_its_alias() {
        let sql = "select * from customers c join orders o on o.customer_id = c.id where ";
        let items = complete(sql, sql.len(), &shop(), Dialect::Postgres);
        let owners: Vec<Option<&str>> = items
            .iter()
            .filter(|item| item.label == "id" && item.kind == super::CompletionKind::Column)
            .map(|item| item.qualifier.as_deref())
            .collect();
        assert_eq!(owners.len(), 2, "{items:?}");
        assert!(
            owners.contains(&Some("c")) && owners.contains(&Some("o")),
            "{owners:?}"
        );
        let customer_id = items
            .iter()
            .find(|item| item.label == "customer_id")
            .unwrap();
        assert_eq!(customer_id.qualifier, None, "only one table has it");
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
