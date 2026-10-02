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
    /// Whether only this spelling names the column, as in Postgres, which takes a quoted
    /// name as written and folds a bare one to lower case first. MySQL and SQLite match
    /// a column's name in any case, quoted or not.
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
            Expr::Identifier(ident) if item.options.nulls_first.is_none() => {
                let (column, exact) = match dialect {
                    Dialect::Postgres if ident.quote_style.is_none() => {
                        (ident.value.to_ascii_lowercase(), true)
                    }
                    Dialect::Postgres => (ident.value.clone(), true),
                    Dialect::Mysql | Dialect::Sqlite => (ident.value.clone(), false),
                };
                Some(OrderKey {
                    column,
                    descending: item.options.asc == Some(false),
                    exact,
                })
            }
            _ => None,
        })
        .collect()
}

/// `keys` as ORDER BY text. A name goes bare only when it is a plain lower-case word
/// no dialect takes for a keyword -- `total`, `created_at` -- and is quoted otherwise,
/// so it names the column everywhere: Postgres read a bare `user` as the current user
/// and sorted by a constant, and MySQL refused a bare `rank`.
pub fn order_text(keys: &[OrderKey], dialect: Dialect) -> String {
    keys.iter()
        .map(|key| {
            let name = if bare(&key.column) {
                key.column.clone()
            } else {
                let quote = dialect.quote();
                let escaped = key.column.replace(quote, &format!("{quote}{quote}"));
                format!("{quote}{escaped}{quote}")
            };
            format!("{name} {}", if key.descending { "DESC" } else { "ASC" })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `name` means the same column written bare in every dialect.
fn bare(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !crate::is_reserved(name)
        && KEYWORDS.binary_search(&name).is_err()
}

/// The words Postgres reserves, MySQL reserves, and SQLite's keywords: one of these
/// bare is not a column name in some dialect. sqlparser's list holds every word any
/// dialect knows -- `id`, `name`, `status` -- and quoting those helps nobody. Sorted,
/// for the binary search.
const KEYWORDS: &[&str] = &[
    "abort",
    "accessible",
    "action",
    "add",
    "after",
    "all",
    "alter",
    "always",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asensitive",
    "asymmetric",
    "attach",
    "authorization",
    "autoincrement",
    "before",
    "begin",
    "between",
    "bigint",
    "binary",
    "blob",
    "both",
    "by",
    "call",
    "cascade",
    "case",
    "cast",
    "change",
    "char",
    "character",
    "check",
    "collate",
    "collation",
    "column",
    "commit",
    "concurrently",
    "condition",
    "conflict",
    "constraint",
    "continue",
    "convert",
    "create",
    "cross",
    "cube",
    "cume_dist",
    "current",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "cursor",
    "database",
    "databases",
    "day_hour",
    "day_microsecond",
    "day_minute",
    "day_second",
    "dec",
    "decimal",
    "declare",
    "default",
    "deferrable",
    "deferred",
    "delayed",
    "delete",
    "dense_rank",
    "desc",
    "describe",
    "detach",
    "deterministic",
    "distinct",
    "distinctrow",
    "div",
    "do",
    "double",
    "drop",
    "dual",
    "each",
    "else",
    "elseif",
    "empty",
    "enclosed",
    "end",
    "escape",
    "escaped",
    "except",
    "exclude",
    "exclusive",
    "exists",
    "exit",
    "explain",
    "fail",
    "false",
    "fetch",
    "filter",
    "first",
    "first_value",
    "float",
    "float4",
    "float8",
    "following",
    "for",
    "force",
    "foreign",
    "freeze",
    "from",
    "full",
    "fulltext",
    "function",
    "generated",
    "get",
    "glob",
    "grant",
    "group",
    "grouping",
    "groups",
    "having",
    "high_priority",
    "hour_microsecond",
    "hour_minute",
    "hour_second",
    "if",
    "ignore",
    "ilike",
    "immediate",
    "in",
    "index",
    "indexed",
    "infile",
    "initially",
    "inner",
    "inout",
    "insensitive",
    "insert",
    "instead",
    "int",
    "int1",
    "int2",
    "int3",
    "int4",
    "int8",
    "integer",
    "intersect",
    "interval",
    "into",
    "io_after_gtids",
    "io_before_gtids",
    "is",
    "isnull",
    "iterate",
    "join",
    "json_table",
    "key",
    "keys",
    "kill",
    "lag",
    "last",
    "last_value",
    "lateral",
    "lead",
    "leading",
    "leave",
    "left",
    "like",
    "limit",
    "linear",
    "lines",
    "load",
    "localtime",
    "localtimestamp",
    "lock",
    "long",
    "longblob",
    "longtext",
    "loop",
    "low_priority",
    "manual",
    "master_bind",
    "master_ssl_verify_server_cert",
    "match",
    "materialized",
    "maxvalue",
    "mediumblob",
    "mediumint",
    "mediumtext",
    "middleint",
    "minute_microsecond",
    "minute_second",
    "mod",
    "modifies",
    "natural",
    "no",
    "no_write_to_binlog",
    "not",
    "nothing",
    "notnull",
    "nth_value",
    "ntile",
    "null",
    "nulls",
    "numeric",
    "of",
    "offset",
    "on",
    "only",
    "optimize",
    "optimizer_costs",
    "option",
    "optionally",
    "or",
    "order",
    "others",
    "out",
    "outer",
    "outfile",
    "over",
    "overlaps",
    "parallel",
    "partition",
    "percent_rank",
    "placing",
    "plan",
    "pragma",
    "preceding",
    "precision",
    "primary",
    "procedure",
    "purge",
    "qualify",
    "query",
    "raise",
    "range",
    "rank",
    "read",
    "read_write",
    "reads",
    "real",
    "recursive",
    "references",
    "regexp",
    "reindex",
    "release",
    "rename",
    "repeat",
    "replace",
    "require",
    "resignal",
    "restrict",
    "return",
    "returning",
    "revoke",
    "right",
    "rlike",
    "rollback",
    "row",
    "row_number",
    "rows",
    "savepoint",
    "schema",
    "schemas",
    "second_microsecond",
    "select",
    "sensitive",
    "separator",
    "session_user",
    "set",
    "show",
    "signal",
    "similar",
    "smallint",
    "some",
    "spatial",
    "specific",
    "sql",
    "sql_big_result",
    "sql_calc_found_rows",
    "sql_small_result",
    "sqlexception",
    "sqlstate",
    "sqlwarning",
    "ssl",
    "starting",
    "stored",
    "straight_join",
    "symmetric",
    "system",
    "system_user",
    "table",
    "tablesample",
    "temp",
    "temporary",
    "terminated",
    "then",
    "ties",
    "tinyblob",
    "tinyint",
    "tinytext",
    "to",
    "trailing",
    "transaction",
    "trigger",
    "true",
    "unbounded",
    "undo",
    "union",
    "unique",
    "unlock",
    "unsigned",
    "update",
    "usage",
    "use",
    "user",
    "using",
    "utc_date",
    "utc_time",
    "utc_timestamp",
    "vacuum",
    "values",
    "varbinary",
    "varchar",
    "varcharacter",
    "variadic",
    "varying",
    "verbose",
    "view",
    "virtual",
    "when",
    "where",
    "while",
    "window",
    "with",
    "without",
    "write",
    "xor",
    "year_month",
    "zerofill",
];

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
        let keys = order_keys("TOTAL desc, \"Name\", id asc", Dialect::Postgres).unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (key.column.as_str(), key.descending, key.exact))
                .collect::<Vec<_>>(),
            [
                ("total", true, true),
                ("Name", false, true),
                ("id", false, true)
            ]
        );
        // Postgres folds a bare name to lower case and takes a quoted one as written.
        assert!(keys[0].names("total") && !keys[0].names("TOTAL"));
        assert!(keys[1].names("Name") && !keys[1].names("name"));
        // MySQL and SQLite match a column's name in any case, quoted or not.
        let mysql = order_keys("`TOTAL` desc", Dialect::Mysql).unwrap();
        assert!(mysql[0].names("total") && mysql[0].names("Total"));
        let sqlite = order_keys("\"TOTAL\"", Dialect::Sqlite).unwrap();
        assert!(sqlite[0].names("total"));
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
        assert_eq!(
            order_text(&keys, Dialect::Mysql),
            "`order` DESC, `Total` ASC"
        );
        let back = order_keys(&order_text(&keys, Dialect::Postgres), Dialect::Postgres).unwrap();
        assert_eq!(back, keys);
        assert!(super::KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
        // A keyword in any dialect is quoted in every one: bare, Postgres read `user`
        // as the current user, and MySQL refused `rank`.
        let named = |column: &str| OrderKey {
            column: column.into(),
            descending: false,
            exact: true,
        };
        for dialect in [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite] {
            for keyword in [
                "user",
                "current_user",
                "current_role",
                "localtime",
                "rank",
                "rows",
                "range",
            ] {
                let quote = dialect.quote();
                assert_eq!(
                    order_text(&[named(keyword)], dialect),
                    format!("{quote}{keyword}{quote} ASC"),
                    "{dialect:?}"
                );
            }
            for plain in ["total", "created_at", "_n2", "id", "name", "status"] {
                assert_eq!(order_text(&[named(plain)], dialect), format!("{plain} ASC"));
            }
        }
    }
}
