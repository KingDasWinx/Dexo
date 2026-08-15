use dexo_app::NewConnection;

use crate::screens::schema_editor::FormField;

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionForm {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            open: false,
            fields: blank_fields(),
            focus: 0,
            errors: Vec::new(),
        }
    }
}

impl ConnectionForm {
    pub fn open() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn focus_next(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        self.focus = (self.focus + 1) % self.fields.len();
    }

    pub fn focus_prev(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        self.focus = if self.focus == 0 {
            self.fields.len() - 1
        } else {
            self.focus - 1
        };
    }

    pub fn type_char(&mut self, ch: char) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.value.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.value.pop();
        }
    }

    pub fn set_error(&mut self, message: String) {
        self.errors = vec![message];
    }

    pub fn submit(&mut self) -> Option<(NewConnection, String)> {
        self.errors.clear();
        let password = field(&self.fields, "password");
        match to_input(&self.fields) {
            Ok(input) => {
                if password.is_empty() {
                    self.errors.push("password is required".into());
                    return None;
                }
                if let Some(field) = self
                    .fields
                    .iter_mut()
                    .find(|field| field.label == "password")
                {
                    field.value.clear();
                }
                Some((input, password))
            }
            Err(error) => {
                self.errors.push(error);
                None
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec!["Add connection".into()];
        for (index, field) in self.fields.iter().enumerate() {
            let marker = if index == self.focus { ">" } else { " " };
            let value = if field.secret && !field.value.is_empty() {
                "***"
            } else {
                field.value.as_str()
            };
            lines.push(format!("{marker} {}: {value}", field.label));
        }
        for error in &self.errors {
            lines.push(format!("error: {error}"));
        }
        lines
    }
}

fn blank_fields() -> Vec<FormField> {
    vec![
        field_of("name", false),
        field_of("driver", false),
        field_of("host", false),
        field_of("port", false),
        field_of("database", false),
        field_of("username", false),
        field_of("password", true),
        FormField {
            label: "environment".into(),
            value: "local".into(),
            secret: false,
        },
    ]
}

fn field_of(label: &str, secret: bool) -> FormField {
    FormField {
        label: label.into(),
        value: String::new(),
        secret,
    }
}

fn field(fields: &[FormField], label: &str) -> String {
    fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.clone())
        .unwrap_or_default()
}

fn to_input(fields: &[FormField]) -> Result<NewConnection, String> {
    let port = field(fields, "port");
    let port = if port.trim().is_empty() {
        None
    } else {
        Some(
            port.trim()
                .parse()
                .map_err(|_| "port must be a number".to_string())?,
        )
    };
    Ok(NewConnection {
        name: field(fields, "name"),
        driver: field(fields, "driver"),
        host: field(fields, "host"),
        port,
        database: field(fields, "database"),
        username: field(fields, "username"),
        environment: field(fields, "environment"),
    })
}

#[cfg(test)]
mod tests {
    use super::ConnectionForm;

    #[test]
    fn password_field_is_masked_and_cleared_on_submit() {
        let mut form = ConnectionForm::open();
        for (label, value) in [
            ("name", "local-pg"),
            ("driver", "postgres"),
            ("host", "127.0.0.1"),
            ("database", "dexo"),
            ("username", "dexo"),
            ("password", "SUPER_SECRET_SENTINEL"),
        ] {
            let field = form
                .fields
                .iter_mut()
                .find(|field| field.label == label)
                .unwrap();
            field.value = value.into();
        }
        let dump = form.lines().join("\n");
        assert!(dump.contains("password: ***"));
        assert!(!dump.contains("SUPER_SECRET_SENTINEL"));
        let (input, password) = form.submit().unwrap();
        assert_eq!(input.name, "local-pg");
        assert_eq!(password, "SUPER_SECRET_SENTINEL");
        assert!(
            form.fields
                .iter()
                .find(|field| field.label == "password")
                .unwrap()
                .value
                .is_empty()
        );
        assert!(!form.lines().join("\n").contains("SUPER_SECRET_SENTINEL"));
    }
}
