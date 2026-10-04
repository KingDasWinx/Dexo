//! Starting points for the statements the Schema form does not cover: a new document on
//! the object's connection, holding the template for that driver, to edit and run.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlTemplate {
    AlterTable,
    View,
    Index,
    Routine,
    Trigger,
}

impl SqlTemplate {
    /// The document's file name, `alter-orders.sql` for a table and `new-view.sql` for none.
    pub fn title(self, table: Option<&str>) -> String {
        let stem = match (self, table) {
            (Self::AlterTable, Some(table)) => format!("alter-{table}"),
            (Self::AlterTable, None) => "alter-table".into(),
            (Self::View, _) => "new-view".into(),
            (Self::Index, _) => "new-index".into(),
            (Self::Routine, _) => "new-function".into(),
            (Self::Trigger, _) => "new-trigger".into(),
        };
        format!("{stem}.sql")
    }

    /// `table` is the qualified name as the driver reads it, `None` when no table is
    /// selected. A driver that cannot make the object says so instead.
    pub fn text(self, driver: &str, table: Option<&str>) -> Result<String, String> {
        let table_name = table.unwrap_or("table_name");
        let quote = |name: &str| match driver {
            "mysql" | "mariadb" => format!("`{name}`"),
            _ => format!("\"{name}\""),
        };
        let qualified = table
            .map(|table| {
                table
                    .split('.')
                    .map(|part| quote(part.trim_matches(['"', '`'])))
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_else(|| "table_name".into());
        let short = table_name
            .rsplit('.')
            .next()
            .unwrap_or(table_name)
            .trim_matches(['"', '`']);
        let postgres = !matches!(driver, "mysql" | "mariadb" | "sqlite" | "duckdb");
        match self {
            Self::AlterTable => {
                let column_type = if matches!(driver, "mysql" | "mariadb") {
                    "VARCHAR(255)"
                } else {
                    "text"
                };
                Ok(format!(
                    "ALTER TABLE {qualified}\n    ADD COLUMN new_column {column_type};\n"
                ))
            }
            Self::View => Ok(format!(
                "CREATE VIEW new_view AS\nSELECT *\nFROM {qualified};\n"
            )),
            Self::Index => Ok(format!(
                "CREATE INDEX idx_{short}_column_name\n    ON {qualified} (column_name);\n"
            )),
            Self::Routine => match driver {
                "sqlite" => Err("SQLite has no stored functions.".into()),
                "duckdb" => Ok("CREATE MACRO new_function(x) AS x + 1;\n".into()),
                "mysql" | "mariadb" => Ok(
                    "CREATE FUNCTION new_function(x INT)\nRETURNS INT DETERMINISTIC\nRETURN x + 1;\n"
                        .into(),
                ),
                _ => Ok("CREATE FUNCTION new_function(x integer)\nRETURNS integer\nLANGUAGE sql\nAS $$ SELECT x + 1 $$;\n".into()),
            },
            Self::Trigger => match driver {
                "duckdb" => Err("DuckDB has no triggers.".into()),
                "sqlite" => Ok(format!(
                    "CREATE TRIGGER new_trigger AFTER INSERT ON {qualified}\nBEGIN\n    SELECT 1;\nEND;\n"
                )),
                "mysql" | "mariadb" => Ok(format!(
                    "CREATE TRIGGER new_trigger BEFORE INSERT ON {qualified}\nFOR EACH ROW\nSET NEW.column_name = NEW.column_name;\n"
                )),
                _ => {
                    debug_assert!(postgres);
                    Ok(format!(
                        "CREATE FUNCTION new_trigger_fn() RETURNS trigger\nLANGUAGE plpgsql\nAS $$\nBEGIN\n    RETURN NEW;\nEND;\n$$;\n\nCREATE TRIGGER new_trigger BEFORE INSERT ON {qualified}\nFOR EACH ROW EXECUTE FUNCTION new_trigger_fn();\n"
                    ))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SqlTemplate;

    #[test]
    fn each_driver_gets_its_own_dialect() {
        let alter = SqlTemplate::AlterTable;
        assert!(
            alter
                .text("postgres", Some("public.orders"))
                .unwrap()
                .starts_with("ALTER TABLE \"public\".\"orders\"")
        );
        assert!(
            alter
                .text("mysql", Some("shop.orders"))
                .unwrap()
                .contains("`shop`.`orders`")
        );
        assert!(
            SqlTemplate::Trigger
                .text("postgres", Some("orders"))
                .unwrap()
                .contains("EXECUTE FUNCTION")
        );
        assert!(SqlTemplate::Trigger.text("duckdb", None).is_err());
        assert!(SqlTemplate::Routine.text("sqlite", None).is_err());
        assert_eq!(
            SqlTemplate::AlterTable.title(Some("orders")),
            "alter-orders.sql"
        );
    }
}
