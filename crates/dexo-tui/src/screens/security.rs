use dexo_driver_api::{GrantRecord, PrivilegeDef, QualifiedName, SchemaChange};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SecurityScreen {
    pub open: bool,
    pub principals: Vec<String>,
    pub grants: Vec<GrantRecord>,
    pub selected: usize,
    pub has_password: bool,
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
    pub fn lines(&self, width: usize, target: &str) -> Vec<String> {
        let fit = |text: String| crate::model::truncate_cell(&text, width);
        let mut lines = Vec::new();
        for (index, principal) in self.principals.iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            lines.push(fit(format!("{marker} {principal}")));
        }
        if let Some(selected) = self.principals.get(self.selected) {
            lines.push(String::new());
            let held: Vec<&GrantRecord> = self
                .grants
                .iter()
                .filter(|grant| grant.principal.object() == selected)
                .collect();
            if held.is_empty() {
                lines.push(fit(format!("{selected} holds no grants here")));
            }
            for grant in held {
                lines.push(fit(format!(
                    "  {} on {}",
                    grant.privileges.join(", "),
                    grant.target.display_unquoted()
                )));
            }
        }
        if self.has_password {
            lines.push("password: ***".into());
        }
        lines.push(String::new());
        let hint = if target.is_empty() {
            "Up/Down pick a role  Esc close".to_string()
        } else {
            format!("Up/Down pick a role  Enter grant SELECT on {target} to it  Esc close")
        };
        lines.push(fit(hint));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::SecurityScreen;

    #[test]
    fn password_is_never_rendered() {
        let mut screen = SecurityScreen {
            principals: vec!["reporter".into()],
            has_password: true,
            ..SecurityScreen::default()
        };
        screen.open = true;
        let dump = screen.lines(60, "").join("\n");
        assert!(dump.contains("***"));
        assert!(!dump.to_ascii_lowercase().contains("s3cret"));
        assert!(!dump.to_ascii_lowercase().contains("password="));
    }
}
