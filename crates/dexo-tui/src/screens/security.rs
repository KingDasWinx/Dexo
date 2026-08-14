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

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec!["security".into()];
        for (index, principal) in self.principals.iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            lines.push(format!("{marker} {principal}"));
        }
        for grant in &self.grants {
            lines.push(format!(
                "grant {} on {} ({})",
                grant.principal.object(),
                grant.target.display_unquoted(),
                grant.privileges.join(",")
            ));
        }
        if self.has_password {
            lines.push("password: ***".into());
        }
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
        let dump = screen.lines().join("\n");
        assert!(dump.contains("***"));
        assert!(!dump.to_ascii_lowercase().contains("s3cret"));
        assert!(!dump.to_ascii_lowercase().contains("password="));
    }
}
