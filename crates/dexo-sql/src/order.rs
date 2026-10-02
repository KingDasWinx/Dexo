//! The ORDER BY bar's text as a list of columns, and back. A header click rewrites the
//! text, and the headers read their markers from it.

use sqlparser::ast::{Expr, OrderByKind, SetExpr, Statement};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::Dialect;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderKey {
    pub column: String,
    pub descending: bool,
    /// Written quoted, so it names exactly this spelling; a bare name matches any case.
    pub exact: bool,
}

impl OrderKey {
    /// Whether this key names the column `name`.
    pub fn names(&self, name: &str) -> bool {
        if self.exact {
            self.column == name
        } else {
            self.column.eq_ignore_ascii_case(name)
        }
    }
}

/// The columns `text` orders by, first to last. `None` when it says more than column
/// names and directions -- an expression, a position, `NULLS FIRST` -- which no header
/// can show; an empty text orders by nothing.
pub fn order_keys(text: &str, dialect: Dialect) -> Option<Vec<OrderKey>> {
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    let sql = format!("SELECT 1 ORDER BY {text}");
    let statements = match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, &sql),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, &sql),
        Dialect::Sqlite => Parser::parse_sql(&SQLiteDialect {}, &sql),
    }
    .ok()?;
    let [Statement::Query(query)] = statements.as_slice() else {
        return None;
    };
    if !matches!(query.body.as_ref(), SetExpr::Select(_)) || query.limit_clause.is_some() {
        return None;
    }
    let OrderByKind::Expressions(items) = &query.order_by.as_ref()?.kind else {
        return None;
    };
    items
        .iter()
        .map(|item| match &item.expr {
            Expr::Identifier(ident) if item.options.nulls_first.is_none() => Some(OrderKey {
                column: ident.value.clone(),
                descending: item.options.asc == Some(false),
                exact: ident.quote_style.is_some(),
            }),
            _ => None,
        })
        .collect()
}

/// `keys` as ORDER BY text, each name quoted when it has to be.
pub fn order_text(keys: &[OrderKey], dialect: Dialect) -> String {
    keys.iter()
        .map(|key| {
            let name = if crate::is_reserved(&key.column) {
                let quote = dialect.quote();
                let escaped = key.column.replace(quote, &format!("{quote}{quote}"));
                format!("{quote}{escaped}{quote}")
            } else {
                dialect.quote_if_needed(&key.column)
            };
            format!("{name} {}", if key.descending { "DESC" } else { "ASC" })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The keys after a header click on `column`: ascending, then descending, then not
/// sorted. A plain click sorts by that column alone; `add` keeps the others, putting a
/// new column after them.
pub fn cycle_order(keys: &[OrderKey], column: &str, add: bool) -> Vec<OrderKey> {
    let at = keys.iter().position(|key| key.names(column));
    let next = match at.map(|index| keys[index].descending) {
        None => Some(false),
        Some(false) => Some(true),
        Some(true) => None,
    };
    let key = |descending| OrderKey {
        column: column.to_string(),
        descending,
        exact: true,
    };
    if !add {
        return next.map(key).into_iter().collect();
    }
    let mut keys = keys.to_vec();
    match (at, next) {
        (Some(index), Some(descending)) => keys[index] = key(descending),
        (Some(index), None) => {
            keys.remove(index);
        }
        (None, Some(descending)) => keys.push(key(descending)),
        (None, None) => {}
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::{OrderKey, cycle_order, order_keys, order_text};
    use crate::Dialect;

    #[test]
    fn order_text_reads_back_as_columns_or_not_at_all() {
        let keys = order_keys("total desc, \"Name\", id asc", Dialect::Postgres).unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (key.column.as_str(), key.descending, key.exact))
                .collect::<Vec<_>>(),
            [
                ("total", true, false),
                ("Name", false, true),
                ("id", false, false)
            ]
        );
        assert!(keys[0].names("TOTAL") && !keys[1].names("name"));
        assert_eq!(order_keys("  ", Dialect::Mysql), Some(Vec::new()));
        for unreadable in [
            "lower(name)",
            "2",
            "id nulls first",
            "id; drop table t",
            "id limit 1",
        ] {
            assert_eq!(
                order_keys(unreadable, Dialect::Postgres),
                None,
                "{unreadable}"
            );
        }
    }

    #[test]
    fn a_click_cycles_and_shift_adds() {
        let asc = |column: &str| OrderKey {
            column: column.into(),
            descending: false,
            exact: true,
        };
        let one = cycle_order(&[], "id", false);
        assert_eq!(one, [asc("id")]);
        let two = cycle_order(&one, "id", false);
        assert!(two[0].descending);
        assert!(cycle_order(&two, "id", false).is_empty());
        // A plain click on another column sorts by it alone.
        assert_eq!(cycle_order(&two, "name", false), [asc("name")]);
        // Shift keeps the others and goes after them, then cycles in place.
        let added = cycle_order(&one, "name", true);
        assert_eq!(added, [asc("id"), asc("name")]);
        let flipped = cycle_order(&added, "id", true);
        assert!(flipped[0].descending && flipped[1] == asc("name"));
        assert_eq!(cycle_order(&flipped, "id", true), [asc("name")]);
    }

    #[test]
    fn written_names_mean_themselves() {
        let keys = [
            OrderKey {
                column: "order".into(),
                descending: true,
                exact: true,
            },
            OrderKey {
                column: "Total".into(),
                descending: false,
                exact: true,
            },
        ];
        assert_eq!(
            order_text(&keys, Dialect::Postgres),
            "\"order\" DESC, \"Total\" ASC"
        );
        assert_eq!(order_text(&keys, Dialect::Mysql), "`order` DESC, Total ASC");
        let back = order_keys(&order_text(&keys, Dialect::Postgres), Dialect::Postgres).unwrap();
        assert_eq!(back, keys);
    }
}
