use dexo_driver_api::QualifiedName;

use crate::completion::{Catalog, resolve_source};
use crate::context::analyze;
use crate::dialect::Dialect;

pub fn definition_at(sql: &str, cursor: usize, catalog: &dyn Catalog) -> Option<QualifiedName> {
    let token = token_around(sql, cursor);
    if token.is_empty() {
        return None;
    }
    let (qualifier, name) = match token.rsplit_once('.') {
        Some((qualifier, name)) => (Some(qualifier), name),
        None => (None, token),
    };
    if let Some(alias) = qualifier {
        // The same FROM-list resolution completion uses: this file carried a copy of
        // the old `sql.contains("users u")` scan, with the same two defects.
        let context = analyze(sql, cursor, Dialect::Postgres);
        let source = context
            .row_sources
            .iter()
            .find(|source| source.qualifier().eq_ignore_ascii_case(alias))?;
        let table = resolve_source(source, catalog)?;
        if table
            .columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case(name))
        {
            return Some(split_target(&table.qualified, Some(name)));
        }
        return Some(split_target(&table.qualified, None));
    }
    catalog.tables().into_iter().find_map(|table| {
        if table.name.eq_ignore_ascii_case(name) || table.qualified.eq_ignore_ascii_case(token) {
            Some(split_target(&table.qualified, None))
        } else {
            None
        }
    })
}

fn token_around(sql: &str, cursor: usize) -> &str {
    let bytes = sql.as_bytes();
    let mut start = cursor.min(sql.len());
    let mut end = start;
    while start > 0 {
        let ch = bytes[start - 1];
        if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.' {
            start -= 1;
        } else {
            break;
        }
    }
    while end < bytes.len() {
        let ch = bytes[end];
        if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.' {
            end += 1;
        } else {
            break;
        }
    }
    sql.get(start..end).unwrap_or("")
}

fn split_target(qualified: &str, column: Option<&str>) -> QualifiedName {
    let parts: Vec<&str> = qualified.split('.').collect();
    let (catalog, schema, object) = match parts.as_slice() {
        [catalog, schema, object] => (Some(*catalog), Some(*schema), *object),
        [schema, object] => (None, Some(*schema), *object),
        [object] => (None, None, *object),
        _ => (None, None, qualified),
    };
    let object = match column {
        Some(column) => format!("{object}.{column}"),
        None => object.to_string(),
    };
    QualifiedName::new(catalog, schema, object)
}

#[cfg(test)]
mod tests {
    use super::definition_at;
    use crate::completion::FakeCatalog;

    fn catalog() -> FakeCatalog {
        let mut catalog = FakeCatalog::default();
        catalog.add_table("db.public.orders", ["id", "note"], false, 0);
        catalog
    }

    #[test]
    fn goto_definition_resolves_qualified_and_aliased_names() {
        let target = definition_at("select o.id from public.orders o", 9, &catalog()).unwrap();
        assert_eq!(target.display_unquoted(), "db.public.orders.id");
    }
}
