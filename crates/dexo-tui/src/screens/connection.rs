use dexo_app::{ConnectionPolicyOverrides, ConnectionProfile, NewConnection};
use dexo_driver_api::DriverDescriptor;

use crate::screens::schema_editor::FormField;

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionForm {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
    pub editing: Option<ConnectionProfile>,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            open: false,
            fields: blank_fields(""),
            focus: 0,
            errors: Vec::new(),
            editing: None,
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

    pub fn open_edit(profile: &ConnectionProfile) -> Self {
        let mut form = Self {
            open: true,
            fields: blank_fields(&profile.driver),
            focus: 0,
            errors: Vec::new(),
            editing: Some(profile.clone()),
        };
        set_field(&mut form.fields, "name", &profile.name);
        set_field(&mut form.fields, "driver", &profile.driver);
        set_field(
            &mut form.fields,
            "host",
            profile
                .config
                .get("host")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        if let Some(port) = profile.config.get("port") {
            let port = port.to_string();
            set_field(&mut form.fields, "port", port.trim_matches('"'));
        }
        set_field(
            &mut form.fields,
            "database",
            profile
                .config
                .get("database")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        set_field(
            &mut form.fields,
            "username",
            profile
                .config
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        set_field(&mut form.fields, "environment", &profile.environment);
        if let Some(group) = &profile.group_path {
            set_field(&mut form.fields, "group", group);
        }
        form.sync_descriptor_fields();
        form
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
        if self.focused_label() == Some("driver") {
            return;
        }
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.value.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        if self.focused_label() == Some("driver") {
            return;
        }
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.value.pop();
        }
    }

    pub fn cycle_driver(&mut self, delta: i32) {
        if self.focused_label() != Some("driver") {
            return;
        }
        let current = field(&self.fields, "driver");
        let next = next_driver(&current, delta);
        set_field(&mut self.fields, "driver", next);
        self.sync_descriptor_fields();
    }

    fn focused_label(&self) -> Option<&str> {
        self.fields
            .get(self.focus)
            .map(|field| field.label.as_str())
    }

    pub fn set_error(&mut self, message: String) {
        self.errors = vec![message];
    }

    pub fn sync_descriptor_fields(&mut self) {
        let driver = field(&self.fields, "driver");
        let preserved: Vec<(String, String)> = self
            .fields
            .iter()
            .map(|field| (field.label.clone(), field.value.clone()))
            .collect();
        let focus_label = self
            .fields
            .get(self.focus)
            .map(|field| field.label.clone())
            .unwrap_or_default();
        self.fields = blank_fields(&driver);
        for (label, value) in preserved {
            set_field(&mut self.fields, &label, &value);
        }
        self.focus = self
            .fields
            .iter()
            .position(|field| field.label == focus_label)
            .unwrap_or(self.focus.min(self.fields.len().saturating_sub(1)));
    }

    pub fn submit(&mut self) -> Option<(NewConnection, String)> {
        self.errors.clear();
        let password = field(&self.fields, "password");
        match to_input(&self.fields) {
            Ok(input) => {
                if self.editing.is_none() && password.is_empty() {
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
        let mut lines = vec![if self.editing.is_some() {
            "Edit connection".into()
        } else {
            "Add connection".into()
        }];
        for (index, field) in self.fields.iter().enumerate() {
            let marker = if index == self.focus { ">" } else { " " };
            if field.label == "driver" {
                let name = DriverDescriptor::for_id(&field.value)
                    .map(|item| item.display_name)
                    .unwrap_or(field.value.as_str());
                lines.push(format!("{marker} driver: < {name} >  left/right"));
                continue;
            }
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

fn drivers() -> [&'static str; 2] {
    [
        DriverDescriptor::postgres().id,
        DriverDescriptor::mysql().id,
    ]
}

fn normalize_driver(driver: &str) -> &'static str {
    let id = driver.trim();
    drivers()
        .into_iter()
        .find(|known| *known == id)
        .unwrap_or(DriverDescriptor::postgres().id)
}

fn next_driver(current: &str, delta: i32) -> &'static str {
    let known = drivers();
    let index = known
        .iter()
        .position(|id| *id == current.trim())
        .unwrap_or(0);
    let next = (index as i32 + delta).rem_euclid(known.len() as i32) as usize;
    known[next]
}

fn blank_fields(driver: &str) -> Vec<FormField> {
    let driver = normalize_driver(driver);
    let descriptor = DriverDescriptor::for_id(driver);
    let mut fields = vec![
        field_of("name", false),
        FormField {
            label: "driver".into(),
            value: driver.into(),
            secret: false,
        },
        field_of("host", false),
        FormField {
            label: "port".into(),
            value: descriptor
                .as_ref()
                .map(|item| item.default_port.to_string())
                .unwrap_or_default(),
            secret: false,
        },
        field_of("database", false),
        field_of("username", false),
        field_of("password", true),
        FormField {
            label: "environment".into(),
            value: "local".into(),
            secret: false,
        },
        field_of("group", false),
    ];
    let Some(descriptor) = descriptor else {
        return fields;
    };
    if descriptor.options.tls {
        fields.push(field_of("tls_mode", false));
        fields.push(field_of("ca_file", false));
    }
    if descriptor.options.client_certificate {
        fields.push(field_of("client_cert", false));
        fields.push(field_of("client_key", false));
    }
    if descriptor.options.ssh {
        fields.push(field_of("ssh_host", false));
        fields.push(field_of("ssh_port", false));
        fields.push(field_of("ssh_user", false));
        fields.push(field_of("ssh_key", false));
    }
    if descriptor.options.proxy {
        fields.push(field_of("proxy_kind", false));
        fields.push(field_of("proxy_host", false));
        fields.push(field_of("proxy_port", false));
    }
    fields.push(field_of("read_only", false));
    fields.push(field_of("confirm_destructive", false));
    fields.push(field_of("require_verified_tls", false));
    fields.push(field_of("max_rows", false));
    fields.push(field_of("timeout_secs", false));
    fields
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

fn set_field(fields: &mut [FormField], label: &str, value: &str) {
    if let Some(field) = fields.iter_mut().find(|field| field.label == label) {
        field.value = value.to_string();
    }
}

fn optional_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
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
    let mut extra = serde_json::Map::new();
    let tls_mode = field(fields, "tls_mode");
    if !tls_mode.trim().is_empty() {
        let mut tls = serde_json::Map::new();
        tls.insert(
            "mode".into(),
            serde_json::Value::String(tls_mode.trim().into()),
        );
        let ca = field(fields, "ca_file");
        if !ca.trim().is_empty() {
            tls.insert("ca_file".into(), serde_json::Value::String(ca));
        }
        let cert = field(fields, "client_cert");
        if !cert.trim().is_empty() {
            tls.insert("client_cert".into(), serde_json::Value::String(cert));
        }
        let key = field(fields, "client_key");
        if !key.trim().is_empty() {
            tls.insert("client_key".into(), serde_json::Value::String(key));
        }
        extra.insert("tls".into(), serde_json::Value::Object(tls));
    }
    let ssh_host = field(fields, "ssh_host");
    if !ssh_host.trim().is_empty() {
        let mut ssh = serde_json::json!({
            "host": ssh_host,
            "port": field(fields, "ssh_port").parse::<u16>().unwrap_or(22),
            "username": field(fields, "ssh_user"),
        });
        let key = field(fields, "ssh_key");
        if !key.trim().is_empty() {
            ssh.as_object_mut()
                .expect("ssh object")
                .insert("key_file".into(), serde_json::Value::String(key));
        }
        extra.insert("ssh".into(), ssh);
    }
    let proxy_host = field(fields, "proxy_host");
    if !proxy_host.trim().is_empty() {
        extra.insert(
            "proxy".into(),
            serde_json::json!({
                "kind": field(fields, "proxy_kind"),
                "host": proxy_host,
                "port": field(fields, "proxy_port").parse::<u16>().unwrap_or(0),
            }),
        );
    }
    let group = field(fields, "group");
    Ok(NewConnection {
        name: field(fields, "name"),
        driver: field(fields, "driver"),
        host: field(fields, "host"),
        port,
        database: field(fields, "database"),
        username: field(fields, "username"),
        environment: field(fields, "environment"),
        extra_config: serde_json::Value::Object(extra),
        policy: ConnectionPolicyOverrides {
            read_only: optional_bool(&field(fields, "read_only")),
            confirm_destructive: optional_bool(&field(fields, "confirm_destructive")),
            require_verified_tls: optional_bool(&field(fields, "require_verified_tls")),
            max_rows: field(fields, "max_rows").parse().ok(),
            timeout_secs: field(fields, "timeout_secs").parse().ok(),
        },
        group_path: if group.trim().is_empty() {
            None
        } else {
            Some(group)
        },
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
        form.sync_descriptor_fields();
        let dump = form.lines().join("\n");
        assert!(dump.contains("password: ***"));
        assert!(!dump.contains("SUPER_SECRET_SENTINEL"));
        assert!(dump.contains("tls_mode"));
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

    #[test]
    fn driver_is_a_left_right_picker() {
        let mut form = ConnectionForm::open();
        let dump = form.lines().join("\n");
        assert!(dump.contains("< PostgreSQL >"));
        assert!(dump.contains("tls_mode"));
        form.focus = form
            .fields
            .iter()
            .position(|field| field.label == "driver")
            .unwrap();
        form.type_char('x');
        form.backspace();
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value,
            "postgres"
        );
        form.cycle_driver(1);
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value,
            "mysql"
        );
        let dump = form.lines().join("\n");
        assert!(dump.contains("< MySQL >"));
        assert!(dump.contains("left/right"));
        form.cycle_driver(1);
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value,
            "postgres"
        );
    }
}
