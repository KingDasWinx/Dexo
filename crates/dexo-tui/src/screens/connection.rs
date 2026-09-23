use dexo_app::{ConnectionPolicyOverrides, ConnectionProfile, NewConnection};
use dexo_driver_api::DriverDescriptor;

use crate::screens::schema_editor::FormField;
use crate::widgets::form::{FooterFocus, footer_line};

const BASIC_FIELDS: &[&str] = &[
    "name", "driver", "host", "port", "database", "username", "password",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionForm {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
    pub editing: Option<ConnectionProfile>,
    pub advanced: bool,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            open: false,
            fields: blank_fields(""),
            focus: 0,
            errors: Vec::new(),
            editing: None,
            advanced: false,
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
            advanced: false,
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
        populate_advanced_fields(&mut form.fields, profile);
        form.advanced = has_advanced_values(&form.fields);
        form
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn focus_next(&mut self) {
        self.move_focus(1);
    }

    pub fn focus_prev(&mut self) {
        self.move_focus(-1);
    }

    fn move_focus(&mut self, delta: i32) {
        let order = self.focus_order();
        if order.is_empty() {
            return;
        }
        let current = order
            .iter()
            .position(|candidate| *candidate == self.focus)
            .unwrap_or(0);
        let next = (current as i32 + delta).rem_euclid(order.len() as i32) as usize;
        self.focus = order[next];
    }

    fn focus_order(&self) -> Vec<usize> {
        let mut order = self.basic_field_indices();
        order.push(self.advanced_focus_index());
        if self.advanced {
            order.extend(self.advanced_field_indices());
        }
        order.push(self.fields.len());
        order.push(self.fields.len() + 1);
        order
    }

    pub fn advanced_focus_index(&self) -> usize {
        self.fields.len() + 2
    }

    pub fn on_advanced(&self) -> bool {
        self.focus == self.advanced_focus_index()
    }

    pub fn set_advanced(&mut self, open: bool) {
        self.advanced = open;
        if !open && self.focused_label().is_some_and(|label| !is_basic(label)) {
            self.focus = self.advanced_focus_index();
        }
    }

    pub fn toggle_advanced(&mut self) {
        self.set_advanced(!self.advanced);
    }

    fn basic_field_indices(&self) -> Vec<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| is_basic(&field.label).then_some(index))
            .collect()
    }

    fn advanced_field_indices(&self) -> Vec<usize> {
        self.fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| (!is_basic(&field.label)).then_some(index))
            .collect()
    }

    pub fn footer_focus(&self) -> FooterFocus {
        if self.focus == self.fields.len() {
            FooterFocus::Submit
        } else if self.focus == self.fields.len() + 1 {
            FooterFocus::Cancel
        } else {
            FooterFocus::Input
        }
    }

    pub fn on_submit(&self) -> bool {
        self.footer_focus() == FooterFocus::Submit
    }

    pub fn on_cancel(&self) -> bool {
        self.footer_focus() == FooterFocus::Cancel
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
        let old_len = self.fields.len();
        let special_focus = if self.focus == old_len {
            Some(FooterFocus::Submit)
        } else if self.focus == old_len + 1 {
            Some(FooterFocus::Cancel)
        } else if self.focus == old_len + 2 {
            Some(FooterFocus::Input)
        } else {
            None
        };
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
        self.focus = match special_focus {
            Some(FooterFocus::Submit) => self.fields.len(),
            Some(FooterFocus::Cancel) => self.fields.len() + 1,
            Some(FooterFocus::Input) => self.advanced_focus_index(),
            None => self
                .fields
                .iter()
                .position(|field| field.label == focus_label)
                .unwrap_or(0),
        };
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

    pub fn title(&self) -> &'static str {
        if self.editing.is_some() {
            "Edit connection"
        } else {
            "Add connection"
        }
    }

    fn field_rows(&self) -> Vec<(Option<usize>, String)> {
        let mut rows = Vec::new();
        for index in self.basic_field_indices() {
            rows.push((Some(index), self.render_field(index)));
        }
        let advanced = self.advanced_focus_index();
        let marker = if self.focus == advanced { ">" } else { " " };
        rows.push((
            Some(advanced),
            format!(
                "{marker} [{}] Advanced options",
                if self.advanced { "v" } else { ">" }
            ),
        ));
        if self.advanced {
            for index in self.advanced_field_indices() {
                rows.push((Some(index), self.render_field(index)));
            }
        }
        for error in &self.errors {
            rows.push((None, format!("error: {error}")));
        }
        rows
    }

    fn render_field(&self, index: usize) -> String {
        let field = &self.fields[index];
        let marker = if index == self.focus { ">" } else { " " };
        if field.label == "driver" {
            let name = DriverDescriptor::for_id(&field.value)
                .map(|item| item.display_name)
                .unwrap_or(field.value.as_str());
            return format!("{marker} driver: < {name} >  left/right");
        }
        let value = if field.secret && !field.value.is_empty() {
            "***"
        } else {
            field.value.as_str()
        };
        format!("{marker} {}: {value}", field.label)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = self
            .field_rows()
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        lines.push(footer_line("Submit", self.footer_focus()));
        lines
    }

    pub fn visible_rows(&self, rows: usize) -> Vec<(Option<usize>, String)> {
        let body = self.field_rows();
        let body_rows = rows.saturating_sub(1).max(1);
        let focus_line = body
            .iter()
            .position(|(target, _)| *target == Some(self.focus))
            .unwrap_or_else(|| body.len().saturating_sub(1));
        let offset = crate::palette::scroll_to_selection(focus_line, 0, body.len(), body_rows);
        let mut visible = body
            .into_iter()
            .skip(offset)
            .take(body_rows)
            .collect::<Vec<_>>();
        visible.push((None, footer_line("Submit", self.footer_focus())));
        visible
    }

    pub fn visible_lines(&self, rows: usize) -> Vec<String> {
        self.visible_rows(rows)
            .into_iter()
            .map(|(_, line)| line)
            .collect()
    }
}

fn is_basic(label: &str) -> bool {
    BASIC_FIELDS.contains(&label)
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

fn populate_advanced_fields(fields: &mut [FormField], profile: &ConnectionProfile) {
    if let Some(tls) = profile.config.get("tls") {
        set_json_field(fields, "tls_mode", tls.get("mode"));
        set_json_field(fields, "ca_file", tls.get("ca_file"));
        set_json_field(fields, "client_cert", tls.get("client_cert"));
        set_json_field(fields, "client_key", tls.get("client_key"));
    }
    if let Some(ssh) = profile.config.get("ssh") {
        set_json_field(fields, "ssh_host", ssh.get("host"));
        set_json_field(fields, "ssh_port", ssh.get("port"));
        set_json_field(fields, "ssh_user", ssh.get("username"));
        set_json_field(fields, "ssh_key", ssh.get("key_file"));
    }
    if let Some(proxy) = profile.config.get("proxy") {
        set_json_field(fields, "proxy_kind", proxy.get("kind"));
        set_json_field(fields, "proxy_host", proxy.get("host"));
        set_json_field(fields, "proxy_port", proxy.get("port"));
    }
    set_option_field(fields, "read_only", profile.policy.read_only);
    set_option_field(
        fields,
        "confirm_destructive",
        profile.policy.confirm_destructive,
    );
    set_option_field(
        fields,
        "require_verified_tls",
        profile.policy.require_verified_tls,
    );
    if let Some(value) = profile.policy.max_rows {
        set_field(fields, "max_rows", &value.to_string());
    }
    if let Some(value) = profile.policy.timeout_secs {
        set_field(fields, "timeout_secs", &value.to_string());
    }
}

fn set_json_field(fields: &mut [FormField], label: &str, value: Option<&serde_json::Value>) {
    let Some(value) = value else {
        return;
    };
    let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    set_field(fields, label, &text);
}

fn set_option_field(fields: &mut [FormField], label: &str, value: Option<bool>) {
    if let Some(value) = value {
        set_field(fields, label, if value { "true" } else { "false" });
    }
}

fn has_advanced_values(fields: &[FormField]) -> bool {
    fields.iter().any(|field| {
        if is_basic(&field.label) {
            return false;
        }
        let value = field.value.trim();
        !(value.is_empty() || field.label == "environment" && value == "local")
    })
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
        assert!(dump.contains("Advanced options"));
        assert!(!dump.contains("tls_mode"));
        form.toggle_advanced();
        assert!(form.lines().join("\n").contains("tls_mode"));
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
        assert!(!dump.contains("tls_mode"));
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

    #[test]
    fn advanced_options_are_collapsed_by_default() {
        let mut form = ConnectionForm::open();
        let basic = form.lines().join("\n");
        for label in [
            "name:",
            "driver:",
            "host:",
            "port:",
            "database:",
            "username:",
            "password:",
        ] {
            assert!(basic.contains(label));
        }
        assert!(basic.contains("[>] Advanced options"));
        assert!(!basic.contains("environment:"));
        assert!(!basic.contains("ssh_host:"));

        form.focus = form.advanced_focus_index();
        form.toggle_advanced();
        let advanced = form.lines().join("\n");
        assert!(advanced.contains("[v] Advanced options"));
        assert!(advanced.contains("environment:"));
        assert!(advanced.contains("ssh_host:"));
    }

    #[test]
    fn keyboard_focus_skips_collapsed_fields() {
        let mut form = ConnectionForm::open();
        form.focus = form
            .fields
            .iter()
            .position(|field| field.label == "password")
            .unwrap();
        form.focus_next();
        assert!(form.on_advanced());
        form.focus_next();
        assert!(form.on_submit());
        form.focus_prev();
        form.toggle_advanced();
        form.focus_next();
        assert_eq!(form.focused_label(), Some("environment"));
    }

    #[test]
    fn long_form_scrolls_to_focus_and_keeps_actions() {
        let mut form = ConnectionForm::open();
        assert!(form.fields.len() > 8);
        form.set_advanced(true);
        form.focus = form.fields.len() - 1;
        let last = form.fields.last().unwrap().label.clone();
        let lines = form.visible_lines(8);
        assert!(lines.iter().any(|line| line.contains(&last)));
        assert!(lines.iter().any(|line| line.contains("[Submit]")));
        assert!(lines.iter().any(|line| line.contains("[Cancel]")));
        assert!(!lines.iter().any(|line| line.contains(" name:")));
        form.focus_next();
        assert!(form.on_submit());
        form.focus_next();
        assert!(form.on_cancel());
        form.focus_next();
        assert_eq!(form.focus, 0);
    }
}
