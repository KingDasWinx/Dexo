use dexo_driver_api::{Filter, Page, Sort};

use crate::statement::{StatementEffect, split_statements};

pub fn derive_page(
    sql: &str,
    sort: &[Sort],
    filter: &Option<Filter>,
    page: Page,
) -> Result<String, String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("empty query".into());
    }
    let statements = split_statements(trimmed);
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
    let mut wrapped = format!("SELECT * FROM ({body}) AS _dexo_derived");
    if let Some(filter) = filter {
        wrapped.push_str(" WHERE ");
        wrapped.push_str(&render_filter(filter)?);
    }
    if !sort.is_empty() {
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
    wrapped.push_str(&format!(" LIMIT {} OFFSET {}", page.limit, page.offset));
    Ok(wrapped)
}

fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn render_filter(filter: &Filter) -> Result<String, String> {
    match filter {
        Filter::Eq(column, _) => Ok(format!("{} = ?", quote(&column.0))),
        Filter::Ne(column, _) => Ok(format!("{} <> ?", quote(&column.0))),
        Filter::Gt(column, _) => Ok(format!("{} > ?", quote(&column.0))),
        Filter::Gte(column, _) => Ok(format!("{} >= ?", quote(&column.0))),
        Filter::Lt(column, _) => Ok(format!("{} < ?", quote(&column.0))),
        Filter::Lte(column, _) => Ok(format!("{} <= ?", quote(&column.0))),
        Filter::IsNull(column) => Ok(format!("{} IS NULL", quote(&column.0))),
        Filter::IsNotNull(column) => Ok(format!("{} IS NOT NULL", quote(&column.0))),
        Filter::And(parts) => Ok(format!(
            "({})",
            parts
                .iter()
                .map(render_filter)
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND ")
        )),
        Filter::Or(parts) => Ok(format!(
            "({})",
            parts
                .iter()
                .map(render_filter)
                .collect::<Result<Vec<_>, _>>()?
                .join(" OR ")
        )),
        Filter::Not(inner) => Ok(format!("NOT ({})", render_filter(inner)?)),
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
    use dexo_driver_api::{ColumnId, Filter, Page, Sort};
    use dexo_driver_api::DbValue;

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
}
