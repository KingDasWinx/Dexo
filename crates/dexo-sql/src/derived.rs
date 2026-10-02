use dexo_driver_api::{Filter, Page, RawClauses, Sort};

use crate::Dialect;
use crate::statement::{StatementEffect, split_statements_in};

pub fn derive_page(
    sql: &str,
    sort: &[Sort],
    filter: &Option<Filter>,
    page: Page,
) -> Result<String, String> {
    derive_page_in(
        sql,
        sort,
        filter,
        &RawClauses::default(),
        page,
        Dialect::Postgres,
    )
}

/// [`derive_page`] with `dialect`'s identifier quoting. It always used double quotes,
/// which MySQL reads as a string: `WHERE "name" = ?` compared the text `name` with the
/// value and quietly returned the wrong rows.
pub fn derive_page_in(
    sql: &str,
    sort: &[Sort],
    filter: &Option<Filter>,
    clauses: &RawClauses,
    page: Page,
    dialect: Dialect,
) -> Result<String, String> {
    let quote = |ident: &str| quote(ident, dialect);
    let mut wrapped = filtered("*", sql, filter, clauses, dialect)?;
    if let Some(order) = clauses.order() {
        wrapped.push_str(" ORDER BY ");
        wrapped.push_str(order);
    } else if !sort.is_empty() {
        wrapped.push_str(" ORDER BY ");
        wrapped.push_str(
            &sort
                .iter()
                .map(|sort| {
                    format!(
                        "{} {}",
                        quote(&sort.column.0),
                        if sort.descending { "DESC" } else { "ASC" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    // On a line of its own: a comment ending the ORDER BY text would take it otherwise.
    wrapped.push_str(&format!("\nLIMIT {} OFFSET {}", page.limit, page.offset));
    Ok(wrapped)
}

/// How many rows [`derive_page_in`] pages through: the same statement and filters
/// under `SELECT COUNT(*)`.
pub fn derive_count_in(
    sql: &str,
    filter: &Option<Filter>,
    clauses: &RawClauses,
    dialect: Dialect,
) -> Result<String, String> {
    filtered("COUNT(*)", sql, filter, clauses, dialect)
}

/// `SELECT {select} FROM (sql) WHERE ...`, once `sql` is known to be one plain read.
fn filtered(
    select: &str,
    sql: &str,
    filter: &Option<Filter>,
    clauses: &RawClauses,
    dialect: Dialect,
) -> Result<String, String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("empty query".into());
    }
    let statements = split_statements_in(trimmed, dialect);
    if statements.len() != 1 {
        return Err("only one statement can be re-run remotely".into());
    }
    let span = &statements[0];
    if !span.understood || span.effect != StatementEffect::ReadOnly {
        return Err("only a read-only SELECT can be re-run remotely".into());
    }
    let body = trimmed.trim_end_matches(';').trim();
    let lower = body.to_ascii_lowercase();
    if lower.contains(" for update") || lower.contains(" for share") {
        return Err("locking queries are local-only".into());
    }
    // A line comment ending the statement (`--`, or MySQL's `#`) took the `)` and the
    // rest of the line with it: the `)` goes on a line of its own then.
    let close = if body.contains("--") || body.contains('#') {
        "\n)"
    } else {
        ")"
    };
    let mut wrapped = format!("SELECT {select} FROM ({body}{close} AS _dexo_derived");
    wrapped.push_str(&where_clause(filter, clauses, dialect)?);
    Ok(wrapped)
}

/// ` WHERE (raw) AND (typed)`, or nothing: the bars' text and the typed filter, its
/// values bound with the dialect's own placeholders -- numbered here, as rewriting `?`
/// afterwards also rewrote the user's `'%?%'` and jsonb's `?` operator.
fn where_clause(
    filter: &Option<Filter>,
    clauses: &RawClauses,
    dialect: Dialect,
) -> Result<String, String> {
    let quote = |ident: &str| quote(ident, dialect);
    let mut bound = 0;
    let mut placeholder = || {
        bound += 1;
        match dialect {
            Dialect::Postgres => format!("${bound}"),
            Dialect::Mysql | Dialect::Sqlite => "?".to_string(),
        }
    };
    let typed = filter
        .as_ref()
        .map(|filter| render_filter(filter, &quote, &mut placeholder))
        .transpose()?;
    Ok(clauses
        .condition(typed)
        .map(|condition| format!(" WHERE {condition}"))
        .unwrap_or_default())
}

/// How many rows a table document pages through: `SELECT COUNT(*) FROM schema.table`
/// under the same WHERE, the table named as the page names it -- so a WHERE written
/// with the table's name (`orders.id > 1`) counts as it filters.
pub fn table_count_in(
    name: &dexo_driver_api::QualifiedName,
    filter: &Option<Filter>,
    clauses: &RawClauses,
    dialect: Dialect,
) -> Result<String, String> {
    let from = table_select(name, dialect);
    let table = from.trim_start_matches("SELECT * FROM ");
    Ok(format!(
        "SELECT COUNT(*) FROM {table}{}",
        where_clause(filter, clauses, dialect)?
    ))
}

/// `SELECT * FROM schema.table` for `name`, each part quoted: what a table document's
/// rows are, as a statement the derivations can wrap. Postgres leaves the database
/// out, which it does not take in a name.
pub fn table_select(name: &dexo_driver_api::QualifiedName, dialect: Dialect) -> String {
    let container = match dialect {
        Dialect::Postgres => name.schema(),
        Dialect::Mysql | Dialect::Sqlite => name.schema().or(name.catalog()),
    };
    let object = quote(name.object(), dialect);
    match container {
        Some(container) => format!("SELECT * FROM {}.{object}", quote(container, dialect)),
        None => format!("SELECT * FROM {object}"),
    }
}

fn quote(ident: &str, dialect: Dialect) -> String {
    match dialect {
        Dialect::Postgres | Dialect::Sqlite => format!("\"{}\"", ident.replace('"', "\"\"")),
        Dialect::Mysql => format!("`{}`", ident.replace('`', "``")),
    }
}

fn render_filter(
    filter: &Filter,
    quote: &dyn Fn(&str) -> String,
    placeholder: &mut dyn FnMut() -> String,
) -> Result<String, String> {
    let mut compare =
        |column: &dexo_driver_api::ColumnId, operator: &str| -> Result<String, String> {
            Ok(format!("{} {operator} {}", quote(&column.0), placeholder()))
        };
    match filter {
        Filter::Eq(column, _) => compare(column, "="),
        Filter::Ne(column, _) => compare(column, "<>"),
        Filter::Gt(column, _) => compare(column, ">"),
        Filter::Gte(column, _) => compare(column, ">="),
        Filter::Lt(column, _) => compare(column, "<"),
        Filter::Lte(column, _) => compare(column, "<="),
        Filter::IsNull(column) => Ok(format!("{} IS NULL", quote(&column.0))),
        Filter::IsNotNull(column) => Ok(format!("{} IS NOT NULL", quote(&column.0))),
        Filter::And(parts) => Ok(format!(
            "({})",
            parts
                .iter()
                .map(|part| render_filter(part, quote, placeholder))
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND ")
        )),
        Filter::Or(parts) => Ok(format!(
            "({})",
            parts
                .iter()
                .map(|part| render_filter(part, quote, placeholder))
                .collect::<Result<Vec<_>, _>>()?
                .join(" OR ")
        )),
        Filter::Not(inner) => Ok(format!(
            "NOT ({})",
            render_filter(inner, quote, placeholder)?
        )),
    }
}

pub fn filter_values(filter: &Filter) -> Vec<dexo_driver_api::DbValue> {
    match filter {
        Filter::Eq(_, value)
        | Filter::Ne(_, value)
        | Filter::Gt(_, value)
        | Filter::Gte(_, value)
        | Filter::Lt(_, value)
        | Filter::Lte(_, value) => vec![value.clone()],
        Filter::IsNull(_) | Filter::IsNotNull(_) => Vec::new(),
        Filter::And(parts) | Filter::Or(parts) => parts.iter().flat_map(filter_values).collect(),
        Filter::Not(inner) => filter_values(inner),
    }
}

#[cfg(test)]
mod tests {
    use super::derive_page;
    use crate::Dialect;
    use dexo_driver_api::DbValue;
    use dexo_driver_api::{ColumnId, Filter, Page, Sort};

    fn page() -> Page {
        Page::new(0, 50).unwrap()
    }

    fn sort() -> Vec<Sort> {
        vec![Sort {
            column: ColumnId("id".into()),
            descending: false,
        }]
    }

    fn filter() -> Option<Filter> {
        Some(Filter::Eq(ColumnId("id".into()), DbValue::I64(1)))
    }

    #[test]
    fn wraps_only_one_read_only_select_without_locking_or_terminator() {
        assert!(derive_page("select id,name from users", &sort(), &filter(), page()).is_ok());
        assert!(derive_page("update users set name='x'", &sort(), &filter(), page()).is_err());
        assert!(derive_page("select * from users for update", &sort(), &filter(), page()).is_err());
    }

    /// The count wraps what the page wraps, filters and all, and nothing that writes.
    #[test]
    fn a_count_covers_what_the_pages_do() {
        let clauses = dexo_driver_api::RawClauses {
            where_sql: Some("total > 5".into()),
            order_by: Some("id".into()),
        };
        let table = dexo_driver_api::QualifiedName::new(Some("shop"), Some("public"), "orders");
        let sql = super::table_select(&table, Dialect::Postgres);
        assert_eq!(sql, "SELECT * FROM \"public\".\"orders\"");
        assert_eq!(
            super::derive_count_in(&sql, &filter(), &clauses, Dialect::Postgres).unwrap(),
            "SELECT COUNT(*) FROM (SELECT * FROM \"public\".\"orders\") AS _dexo_derived \
             WHERE (total > 5) AND (\"id\" = $1)"
        );
        // The user's own `?` -- in a literal, jsonb's operator -- is left as written.
        let clauses = dexo_driver_api::RawClauses {
            where_sql: Some("note LIKE '%?%' AND tags ? 'x'".into()),
            order_by: None,
        };
        let both = Some(Filter::And(vec![
            Filter::Eq(ColumnId("a".into()), DbValue::I64(1)),
            Filter::Gt(ColumnId("b".into()), DbValue::I64(2)),
        ]));
        assert_eq!(
            super::derive_count_in(&sql, &both, &clauses, Dialect::Postgres).unwrap(),
            "SELECT COUNT(*) FROM (SELECT * FROM \"public\".\"orders\") AS _dexo_derived \
             WHERE (note LIKE '%?%' AND tags ? 'x') AND ((\"a\" = $1 AND \"b\" > $2))"
        );
        assert!(
            super::derive_count_in(&sql, &both, &clauses, Dialect::Mysql)
                .unwrap()
                .ends_with("((`a` = ? AND `b` > ?))")
        );
        // A comment ending the statement or a bar ends on its own line, before the
        // text that follows it.
        let commented = dexo_driver_api::RawClauses {
            where_sql: Some("total > 5 -- big".into()),
            order_by: Some("id # newest".into()),
        };
        assert_eq!(
            super::derive_page_in(
                "select * from orders -- all of them",
                &[],
                &None,
                &commented,
                page(),
                Dialect::Mysql
            )
            .unwrap(),
            "SELECT * FROM (select * from orders -- all of them\n) AS _dexo_derived \
             WHERE (total > 5 -- big\n) ORDER BY id # newest\nLIMIT 50 OFFSET 0"
        );
        // A table's count names the table as its page does, so the WHERE can too.
        let qualified = dexo_driver_api::RawClauses {
            where_sql: Some("orders.total > 5".into()),
            order_by: None,
        };
        assert_eq!(
            super::table_count_in(&table, &filter(), &qualified, Dialect::Postgres).unwrap(),
            "SELECT COUNT(*) FROM \"public\".\"orders\" WHERE (orders.total > 5) AND (\"id\" = $1)"
        );
        let mysql = dexo_driver_api::QualifiedName::new(Some("shop"), None::<String>, "orders");
        assert_eq!(
            super::table_select(&mysql, Dialect::Mysql),
            "SELECT * FROM `shop`.`orders`"
        );
        assert!(
            super::derive_count_in("delete from t", &None, &Default::default(), Dialect::Mysql)
                .is_err()
        );
    }
}
