use dexo_driver_api::{GrantRecord, PrivilegeDef, QualifiedName, SchemaChange};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SecurityScreen {
    pub principals: Vec<String>,
    pub grants: Vec<GrantRecord>,
    pub selected: usize,
}

impl SecurityScreen {
    pub fn create_role(name: &str) -> SchemaChange {
        SchemaChange::Grant {
            target: QualifiedName::new(None::<String>, None::<String>, name),
            def: PrivilegeDef {
                principal: QualifiedName::new(None::<String>, None::<String>, name),
                privileges: vec![],
                with_grant_option: false,
                role_membership: false,
                create_principal: true,
                login: false,
            },
        }
    }

    pub fn grant_select(table: QualifiedName, principal: &str) -> SchemaChange {
        SchemaChange::Grant {
            target: table,
            def: PrivilegeDef {
                principal: QualifiedName::new(None::<String>, None::<String>, principal),
                privileges: vec!["SELECT".into()],
                with_grant_option: false,
                role_membership: false,
                create_principal: false,
                login: false,
            },
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.principals.len() {
            self.selected += 1;
        }
    }

    /// The roles, then what the selected one holds, then what the keys do; each line at
    /// most `width` columns. `target` is the table Enter grants on.
    /// The roles, the picked one's privileges on `table` -- not its grants everywhere, a
    /// page of `information_schema` -- and how to grant from there.
    pub fn table_lines(&self, table: &str) -> Vec<String> {
        let mut lines: Vec<String> = self
            .principals
            .iter()
            .enumerate()
            .map(|(index, principal)| {
                let marker = if index == self.selected { ">" } else { " " };
                format!("{marker} {principal}")
            })
            .collect();
        let Some(selected) = self.principals.get(self.selected) else {
            lines.push("No roles or grants to show on this connection.".into());
            return lines;
        };
        lines.push(String::new());
        let on_table: Vec<&str> = self
            .grants
            .iter()
            .filter(|grant| {
                grant.principal.object() == selected
                    && grant.target.display_unquoted().eq_ignore_ascii_case(table)
            })
            .flat_map(|grant| grant.privileges.iter().map(String::as_str))
            .collect();
        lines.push(if on_table.is_empty() {
            format!("{selected} holds nothing on {table} of its own.")
        } else {
            format!("{selected} on {table}: {}", on_table.join(", "))
        });
        let elsewhere = self
            .grants
            .iter()
            .filter(|grant| {
                grant.principal.object() == selected
                    && !grant.target.display_unquoted().eq_ignore_ascii_case(table)
            })
            .count();
        if elsewhere > 0 {
            lines.push(format!(
                "and {elsewhere} grant{} on other objects.",
                if elsewhere == 1 { "" } else { "s" }
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "Enter grants SELECT on {table} to {selected}, after a preview."
        ));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::SecurityScreen;

    /// A role is shown with what it may do on the table, and its grants elsewhere are
    /// counted, not listed: they filled the view with `information_schema`.
    #[test]
    fn a_role_shows_what_it_may_do_on_the_table_and_counts_the_rest() {
        use dexo_driver_api::{GrantRecord, QualifiedName};
        let record = |target: QualifiedName, privilege: &str| GrantRecord {
            principal: QualifiedName::new(None::<String>, None::<String>, "reporter"),
            target,
            privileges: vec![privilege.into()],
        };
        let orders = QualifiedName::new(Some("qa"), Some("public"), "orders");
        let screen = SecurityScreen {
            principals: vec!["reporter".into()],
            grants: vec![
                record(orders.clone(), "SELECT"),
                record(orders, "INSERT"),
                record(
                    QualifiedName::new(Some("qa"), Some("public"), "users"),
                    "SELECT",
                ),
            ],
            ..SecurityScreen::default()
        };
        let text = screen.table_lines("qa.public.orders").join("\n");
        assert!(
            text.contains("reporter on qa.public.orders: SELECT, INSERT"),
            "{text}"
        );
        assert!(text.contains("and 1 grant on other objects"), "{text}");
        assert!(!text.contains("users"), "{text}");
        let empty = SecurityScreen::default()
            .table_lines("qa.public.orders")
            .join("\n");
        assert!(empty.contains("No roles or grants"), "{empty}");
    }
}
