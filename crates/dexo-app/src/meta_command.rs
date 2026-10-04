//! psql's backslash commands, answered from the catalog instead of the server, on every
//! driver: `\dt \dv \di \dn \df [pattern]`, `\d name`, `\l`, `\x` and `\?`.

use dexo_driver_api::{CatalogListOptions, CatalogObject, CatalogReader, ObjectKind};

use crate::catalog_service::CatalogService;
use crate::error::{AppError, ErrorCategory};
use crate::query_service::map_driver_error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetaCommand {
    /// `\dt`, `\dv`, `\di`, `\dn`, `\df`: the objects of these kinds whose name or
    /// `schema.name` matches the pattern.
    List {
        kinds: &'static [ObjectKind],
        pattern: Option<String>,
    },
    /// `\d name`: a table's or view's columns, keys and indexes.
    Describe(String),
    /// `\l`: the databases.
    Databases,
    /// `\x`, `\x on`, `\x off`: rows shown one field per line, or back to the grid;
    /// without an argument it switches.
    RecordView(Option<bool>),
    /// `\?`: what this list says.
    Help,
}

/// Columns and rows, all text, the way a query's result would come back.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetaAnswer {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Vec<String>>,
}

const HELP: &[(&str, &str)] = &[
    ("\\dt [pattern]", "tables"),
    ("\\dv [pattern]", "views"),
    ("\\di [pattern]", "indexes"),
    ("\\dn [pattern]", "schemas"),
    ("\\df [pattern]", "functions and procedures"),
    ("\\d name", "a table's or view's columns, keys and indexes"),
    ("\\l", "databases"),
    (
        "\\x [on|off]",
        "rows one field per line, or back to the grid",
    ),
    ("\\?", "this list"),
];

/// Whether `sql` is a backslash command rather than SQL.
pub fn is_meta(sql: &str) -> bool {
    sql.trim_start().starts_with('\\')
}

pub fn parse(line: &str) -> Result<MetaCommand, AppError> {
    // psql users end a line with `;` out of habit; it belongs to no argument.
    let line = line.trim();
    let line = line.strip_suffix(';').unwrap_or(line).trim_end();
    let (command, argument) = match line.split_once(char::is_whitespace) {
        Some((command, rest)) => (command, Some(rest.trim()).filter(|rest| !rest.is_empty())),
        None => (line, None),
    };
    let pattern = argument.map(str::to_string);
    let list = |kinds: &'static [ObjectKind]| {
        Ok(MetaCommand::List {
            kinds,
            pattern: pattern.clone(),
        })
    };
    // psql takes `S` and `+` after the letters (system objects, more detail); both are
    // accepted and mean the plain command here.
    let base = command.trim_end_matches(['S', '+']);
    match base {
        "\\dt" => list(&[ObjectKind::Table]),
        "\\dv" => list(&[ObjectKind::View, ObjectKind::MaterializedView]),
        "\\di" => list(&[ObjectKind::Index]),
        "\\dn" => list(&[ObjectKind::Schema]),
        "\\df" => list(&[ObjectKind::Function, ObjectKind::Procedure]),
        "\\d" => match argument {
            Some(name) => Ok(MetaCommand::Describe(name.to_string())),
            None => list(&[
                ObjectKind::Table,
                ObjectKind::View,
                ObjectKind::MaterializedView,
                ObjectKind::Sequence,
            ]),
        },
        "\\l" | "\\list" => Ok(MetaCommand::Databases),
        "\\x" => match argument.map(str::to_ascii_lowercase).as_deref() {
            None => Ok(MetaCommand::RecordView(None)),
            Some("on") => Ok(MetaCommand::RecordView(Some(true))),
            Some("off") => Ok(MetaCommand::RecordView(Some(false))),
            Some(other) => Err(AppError::new(
                ErrorCategory::Syntax,
                format!("\\x takes on or off, not {other}"),
            )),
        },
        "\\?" => Ok(MetaCommand::Help),
        _ => Err(AppError::new(
            ErrorCategory::Syntax,
            format!("{command} is not a command Dexo knows; \\? lists the ones it does"),
        )),
    }
}

/// The answer to `command`, from what the catalog lists. `\x` has none: the caller
/// toggles its view.
pub async fn answer(
    reader: &dyn CatalogReader,
    command: &MetaCommand,
) -> Result<MetaAnswer, AppError> {
    let options = CatalogListOptions::default();
    match command {
        MetaCommand::Help => Ok(MetaAnswer {
            columns: vec!["Command", "Shows"],
            rows: HELP
                .iter()
                .map(|(command, shows)| vec![command.to_string(), shows.to_string()])
                .collect(),
        }),
        MetaCommand::RecordView(_) => Ok(MetaAnswer::default()),
        MetaCommand::Databases => Ok(MetaAnswer {
            columns: vec!["Name"],
            rows: reader
                .databases()
                .await
                .map_err(map_driver_error)?
                .into_iter()
                .map(|name| vec![name])
                .collect(),
        }),
        MetaCommand::List { kinds, pattern } => {
            let into_tables = kinds.contains(&ObjectKind::Index);
            let mut found = Vec::new();
            collect(reader, None, kinds, into_tables, &options, &mut found).await?;
            // MySQL and SQLite have no schemas under their databases: `\dn` lists those.
            if kinds == &[ObjectKind::Schema] && found.is_empty() {
                collect(
                    reader,
                    None,
                    &[ObjectKind::Catalog],
                    false,
                    &options,
                    &mut found,
                )
                .await?;
            }
            let rows = found
                .iter()
                .filter(|object| {
                    pattern
                        .as_deref()
                        .is_none_or(|pattern| matches_pattern(object, pattern))
                })
                .map(|object| {
                    vec![
                        schema_of(object).unwrap_or("").to_string(),
                        leaf_name(object).to_string(),
                        crate::catalog_service::exact_kind(object)
                            .as_str()
                            .replace('_', " "),
                    ]
                })
                .collect();
            Ok(MetaAnswer {
                columns: vec!["Schema", "Name", "Type"],
                rows,
            })
        }
        MetaCommand::Describe(name) => {
            // The server's own answer to what the name means: the search_path, the
            // current database. A reader that cannot give one is looked through.
            let (schema, leaf) = split_name(name);
            let relations = match reader
                .relations_named(schema.as_deref(), &leaf)
                .await
                .map_err(map_driver_error)?
            {
                Some(relations) => relations,
                None => {
                    let mut relations = Vec::new();
                    collect(reader, None, DESCRIBED, false, &options, &mut relations).await?;
                    relations
                }
            };
            let object = describe_target(relations, name).ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Syntax,
                    format!("Did not find any relation named \"{name}\"."),
                )
            })?;
            let children =
                CatalogService::list_children(reader, Some(&object.id), &options).await?;
            let mut rows = Vec::new();
            for child in &children.objects {
                let row = match child.kind {
                    ObjectKind::Column => vec![
                        leaf_name(child).to_string(),
                        text_attribute(child, "type"),
                        if nullable(child) { "yes" } else { "no" }.to_string(),
                        String::new(),
                    ],
                    ObjectKind::Index | ObjectKind::Constraint | ObjectKind::Trigger => vec![
                        leaf_name(child).to_string(),
                        child.kind.as_str().to_string(),
                        String::new(),
                        references(child),
                    ],
                    _ => continue,
                };
                rows.push(row);
            }
            Ok(MetaAnswer {
                columns: vec!["Name", "Type", "Nullable", "Detail"],
                rows,
            })
        }
    }
}

/// Walks catalogs and schemas -- and tables too when indexes are asked for, since they
/// hang off their table -- keeping the objects of `kinds`. Four lists deep at most
/// (catalogs, schemas, tables, what hangs off a table): a reader that hands a node back
/// as its own child cannot send the walk round for ever.
async fn collect(
    reader: &dyn CatalogReader,
    parent: Option<&dexo_driver_api::ObjectId>,
    kinds: &[ObjectKind],
    into_tables: bool,
    options: &CatalogListOptions,
    found: &mut Vec<CatalogObject>,
) -> Result<(), AppError> {
    let mut level = vec![parent.cloned()];
    for _ in 0..4 {
        let mut next = Vec::new();
        for parent in &level {
            let page = CatalogService::list_children(reader, parent.as_ref(), options).await?;
            for object in page.objects {
                if kinds.contains(&object.kind) {
                    found.push(object.clone());
                }
                let descend = matches!(object.kind, ObjectKind::Catalog | ObjectKind::Schema)
                    || (into_tables && object.kind == ObjectKind::Table);
                if descend {
                    next.push(Some(object.id));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    Ok(())
}

/// What `\d name` describes.
const DESCRIBED: &[ObjectKind] = &[
    ObjectKind::Table,
    ObjectKind::View,
    ObjectKind::MaterializedView,
];

/// `orders` or `public.orders`, quotes left out, as its schema and its own name.
fn split_name(name: &str) -> (Option<String>, String) {
    let unquoted = name.replace(['"', '`'], "");
    match unquoted.rsplit_once('.') {
        Some((schema, leaf)) => (Some(schema.to_string()), leaf.to_string()),
        None => (None, unquoted),
    }
}

/// The relation `name` -- `orders` or `public.orders` -- means among `relations`, in
/// the order the server looks: one spelled exactly so first, then one that differs
/// only in case.
fn describe_target(relations: Vec<CatalogObject>, name: &str) -> Option<CatalogObject> {
    let (schema, leaf) = split_name(name);
    let (schema, leaf) = (schema.as_deref(), leaf.as_str());
    let named = |object: &CatalogObject, exact: bool| {
        let same = |a: &str, b: &str| {
            if exact {
                a == b
            } else {
                a.eq_ignore_ascii_case(b)
            }
        };
        same(leaf_name(object), leaf)
            && schema.is_none_or(|schema| schema_of(object).is_some_and(|own| same(own, schema)))
    };
    let exact = relations.iter().position(|object| named(object, true));
    let index = exact.or_else(|| relations.iter().position(|object| named(object, false)))?;
    relations.into_iter().nth(index)
}

/// The schema an object sits in, or its database where there are no schemas (MySQL).
fn schema_of(object: &CatalogObject) -> Option<&str> {
    object
        .qualified_name
        .schema()
        .or_else(|| object.qualified_name.catalog())
}

/// The object's own name: MySQL names a column `table.column`, the others just `column`.
fn leaf_name(object: &CatalogObject) -> &str {
    let name = object.qualified_name.object();
    match object.kind {
        ObjectKind::Column => name.rsplit('.').next().unwrap_or(name),
        _ => name,
    }
}

/// psql's patterns: `*` for any run of characters, `?` for one, matched without regard
/// to case against the name, or against `schema.name` when the pattern has a dot.
fn matches_pattern(object: &CatalogObject, pattern: &str) -> bool {
    let subject = if pattern.contains('.') {
        match schema_of(object) {
            Some(schema) => format!("{schema}.{}", leaf_name(object)),
            None => leaf_name(object).to_string(),
        }
    } else {
        leaf_name(object).to_string()
    };
    wildcard(
        &pattern.to_lowercase().chars().collect::<Vec<_>>(),
        &subject.to_lowercase().chars().collect::<Vec<_>>(),
    )
}

fn wildcard(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| wildcard(rest, &text[skip..])),
        Some(('?', rest)) => !text.is_empty() && wildcard(rest, &text[1..]),
        Some((ch, rest)) => text.first() == Some(ch) && wildcard(rest, &text[1..]),
    }
}

fn text_attribute(object: &CatalogObject, key: &str) -> String {
    object
        .attributes
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Each driver says it its own way: Postgres, SQLite and DuckDB flag `not_null`, MySQL answers
/// `YES` or `NO`.
fn nullable(column: &CatalogObject) -> bool {
    let not_null = [
        "driver.postgres.not_null",
        "driver.sqlite.not_null",
        "driver.duckdb.not_null",
    ]
    .iter()
    .find_map(|key| column.attributes.get(*key))
    .and_then(serde_json::Value::as_bool);
    match not_null {
        Some(not_null) => !not_null,
        None => column
            .attributes
            .get("driver.mysql.nullable")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|nullable| nullable.eq_ignore_ascii_case("yes")),
    }
}

/// `→ schema.table (columns)` for a foreign key, blank for anything else.
fn references(object: &CatalogObject) -> String {
    let table = text_attribute(object, "fk_table");
    if table.is_empty() {
        return String::new();
    }
    let schema = text_attribute(object, "fk_schema");
    let columns = match object.attributes.get("fk_referenced") {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.as_str().unwrap_or("").to_string(),
        None => String::new(),
    };
    let target = if schema.is_empty() {
        table
    } else {
        format!("{schema}.{table}")
    };
    format!("→ {target} ({columns})")
}

#[cfg(test)]
mod tests {
    use super::{MetaCommand, is_meta, parse, wildcard};
    use dexo_driver_api::ObjectKind;

    #[test]
    fn backslash_commands_parse_with_their_patterns() {
        assert_eq!(
            parse("\\dt  public.ord*").unwrap(),
            MetaCommand::List {
                kinds: &[ObjectKind::Table],
                pattern: Some("public.ord*".into())
            }
        );
        assert_eq!(parse("\\dtS+").unwrap(), parse("\\dt").unwrap());
        assert_eq!(
            parse("\\d orders").unwrap(),
            MetaCommand::Describe("orders".into())
        );
        assert_eq!(parse("\\x").unwrap(), MetaCommand::RecordView(None));
        assert_eq!(
            parse("\\x on").unwrap(),
            MetaCommand::RecordView(Some(true))
        );
        assert_eq!(
            parse("\\x OFF;").unwrap(),
            MetaCommand::RecordView(Some(false))
        );
        assert!(parse("\\x maybe").is_err());
        assert_eq!(parse("\\dt;").unwrap(), parse("\\dt").unwrap());
        assert_eq!(
            parse("\\d orders;").unwrap(),
            MetaCommand::Describe("orders".into())
        );
        assert!(parse("\\! rm -rf /").is_err());
        assert!(parse("\\copy t to 'x'").is_err());
        assert!(is_meta("  \\dt") && !is_meta("select '\\dt'"));
    }

    /// `\d orders` found nothing on Postgres, whose tables sit in a schema: the name is
    /// looked for under every schema, and `schema.name` narrows it.
    #[test]
    fn describe_finds_a_relation_by_its_own_name() {
        use dexo_driver_api::{CatalogObject, ObjectId, QualifiedName};
        let table = |id: &str, catalog: Option<&str>, schema: Option<&str>, name: &str| {
            CatalogObject::new(
                ObjectId::new(id),
                ObjectKind::Table,
                QualifiedName::new(catalog, schema, name),
                None,
            )
        };
        let relations = vec![
            table("a", Some("db"), Some("audit"), "Orders"),
            table("b", Some("db"), Some("public"), "orders"),
            table("c", Some("shop"), None, "items"),
        ];
        let found = |name: &str| {
            super::describe_target(relations.clone(), name)
                .map(|object| object.id.as_str().to_string())
        };
        assert_eq!(found("orders").as_deref(), Some("b"));
        assert_eq!(found("ORDERS").as_deref(), Some("a"));
        assert_eq!(found("audit.orders").as_deref(), Some("a"));
        assert_eq!(found("\"public\".\"orders\"").as_deref(), Some("b"));
        assert_eq!(found("shop.items").as_deref(), Some("c"));
        assert_eq!(found("missing"), None);
    }

    #[test]
    fn patterns_are_psql_wildcards() {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        assert!(wildcard(&chars("ord*"), &chars("orders")));
        assert!(wildcard(&chars("?rders"), &chars("orders")));
        assert!(!wildcard(&chars("ord"), &chars("orders")));
        assert!(wildcard(&chars("*"), &chars("")));
    }
}
