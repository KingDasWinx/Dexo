use dexo_app::{ConnectionPolicyOverrides, ConnectionProfile, Environment, NewConnection};
use dexo_driver_api::DriverDescriptor;

use crate::screens::schema_editor::FormField;
use crate::widgets::form::FooterFocus;

/// How many rows above the buttons the form keeps for what it has to say.
const STATUS_ROWS: usize = 2;

const ENVIRONMENTS: &[&str] = &["local", "development", "staging", "production"];

const BASIC_FIELDS: &[&str] = &[
    "name", "driver", "path", "host", "port", "database", "username", "password",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionForm {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
    /// What the last Test said when it passed, or that one is running; shown where the
    /// errors are, so it never hides in a toast behind the dialog.
    pub notice: Option<String>,
    pub editing: Option<ConnectionProfile>,
    pub advanced: bool,
    /// The temporary connection this form saves. Its session is already open, so
    /// saving dials nothing new; the saved profile takes over its session and documents.
    pub saving_temporary: Option<ConnectionProfile>,
    /// Filled from a Docker container that lets its user in without a password: an
    /// empty password is that, not a mistake.
    pub allow_empty_password: bool,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            open: false,
            fields: blank_fields(""),
            focus: 0,
            errors: Vec::new(),
            notice: None,
            editing: None,
            advanced: false,
            saving_temporary: None,
            allow_empty_password: false,
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
            ..Self::default()
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
        set_field(
            &mut form.fields,
            "path",
            profile
                .config
                .get("path")
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
        order.push(self.test_focus_index());
        order.push(self.fields.len() + 1);
        order
    }

    /// Submit, Test and Cancel come after the fields, in the order of the buttons on
    /// screen; Advanced options sits among the fields, and its index is not that.
    pub fn test_focus_index(&self) -> usize {
        self.fields.len() + 3
    }

    pub fn on_test(&self) -> bool {
        self.focus == self.test_focus_index()
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

    /// Whether the focused field is picked from a list with Left and Right, never typed.
    pub fn on_choice(&self) -> bool {
        self.is_choice_at(self.focus)
    }

    pub fn is_choice_at(&self, index: usize) -> bool {
        self.fields
            .get(index)
            .is_some_and(|field| is_choice(&field.label))
    }

    /// Hands `key` to the focused text field; a choice is picked, never typed. A key
    /// that changed the form takes back what the last Submit or Test said about it.
    pub fn edit(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let edited = !self.on_choice()
            && self
                .fields
                .get_mut(self.focus)
                .is_some_and(|field| field.value.handle_key(key));
        if edited {
            self.clear_status();
        }
        edited
    }

    fn clear_status(&mut self) {
        self.errors.clear();
        self.notice = None;
    }

    /// Steps the focused choice by `delta`. A new driver brings its own default port,
    /// unless the port was changed by hand.
    pub fn cycle_choice(&mut self, delta: i32) {
        let Some(label) = self.focused_label().filter(|label| is_choice(label)) else {
            return;
        };
        let label = label.to_string();
        let current = field(&self.fields, &label);
        self.clear_status();
        if label == "driver" {
            let next = next_driver(&current, delta);
            let old_port = field(&self.fields, "port");
            let kept = old_port.trim().is_empty()
                || DriverDescriptor::for_id(&current)
                    .is_some_and(|old| old.default_port.to_string() == old_port.trim());
            set_field(&mut self.fields, "driver", next);
            self.sync_descriptor_fields();
            if kept
                && let Some(new) = DriverDescriptor::for_id(next)
                && !new.file
            {
                set_field(&mut self.fields, "port", &new.default_port.to_string());
            }
            return;
        }
        let values = choice_values(&label, &current);
        let at = values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0);
        let next = (at as i32 + delta).rem_euclid(values.len() as i32) as usize;
        set_field(&mut self.fields, &label, &values[next]);
    }

    fn focused_label(&self) -> Option<&str> {
        self.fields
            .get(self.focus)
            .map(|field| field.label.as_str())
    }

    pub fn set_value(&mut self, label: &str, value: &str) {
        set_field(&mut self.fields, label, value);
    }

    pub fn set_error(&mut self, message: String) {
        self.notice = None;
        self.errors = vec![message];
    }

    pub fn set_notice(&mut self, message: String) {
        self.errors.clear();
        self.notice = Some(message);
    }

    pub fn sync_descriptor_fields(&mut self) {
        let driver = field(&self.fields, "driver");
        let old_len = self.fields.len();
        // The buttons and the Advanced row sit past the fields, so their stops move with
        // the field count.
        let special_focus = self
            .focus
            .checked_sub(old_len)
            .filter(|offset| *offset <= 3);
        let preserved: Vec<(String, String)> = self
            .fields
            .iter()
            .map(|field| (field.label.clone(), field.value.as_str().to_string()))
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
            Some(offset) => self.fields.len() + offset,
            None => self
                .fields
                .iter()
                .position(|field| field.label == focus_label)
                .unwrap_or(0),
        };
    }

    /// The fields the form cannot be submitted without, in the order they are drawn.
    fn missing_fields(&self) -> Vec<&'static str> {
        let opens_file = DriverDescriptor::for_id(&field(&self.fields, "driver"))
            .is_some_and(|descriptor| descriptor.file);
        let from_command = !field(&self.fields, "password_command").trim().is_empty();
        let needs_password =
            self.editing.is_none() && !opens_file && !from_command && !self.allow_empty_password;
        let required: &[&str] = if opens_file {
            &["name", "path"]
        } else {
            &["name", "host", "database", "username", "password"]
        };
        required
            .iter()
            .copied()
            .filter(|label| {
                let value = field(&self.fields, label);
                value.trim().is_empty() && (*label != "password" || needs_password)
            })
            .collect()
    }

    pub fn submit(&mut self) -> Option<(NewConnection, String)> {
        self.clear_status();
        let password = field(&self.fields, "password");
        let missing = self.missing_fields();
        if let Some(first) = missing.first() {
            self.focus = self
                .fields
                .iter()
                .position(|field| field.label == *first)
                .unwrap_or(self.focus);
            self.errors.push(match missing.as_slice() {
                [head @ .., last] if !head.is_empty() => {
                    format!("{} and {last} are required", head.join(", "))
                }
                _ => format!("{first} is required"),
            });
            return None;
        }
        match to_input(&self.fields) {
            Ok(mut input) => {
                input.allow_empty_password = self.allow_empty_password;
                // The password stays in the form until it closes: a save the app turns
                // down -- a name taken, a field missing -- comes back to it as typed.
                Some((input, password))
            }
            Err(error) => {
                // The field the message is about may be in the folded part.
                if let Some(label) = error.split_whitespace().next()
                    && let Some(index) = self.fields.iter().position(|field| field.label == label)
                {
                    self.advanced |= !is_basic(label);
                    self.focus = index;
                }
                self.errors.push(error);
                None
            }
        }
    }

    pub fn title(&self) -> &'static str {
        if self.editing.is_some() {
            "Edit connection"
        } else if self.saving_temporary.is_some() {
            "Save connection"
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
        rows
    }

    fn render_field(&self, index: usize) -> String {
        let field = &self.fields[index];
        let marker = if index == self.focus { ">" } else { " " };
        if field.label == "driver" {
            let name = DriverDescriptor::for_id(field.value.as_str())
                .map(|item| item.display_name)
                .unwrap_or(field.value.as_str());
            return format!("{marker} driver: < {name} >  Left/Right");
        }
        let label = shown_label(&field.label);
        if is_choice(&field.label) {
            let shown = choice_label(&field.label, field.value.as_str());
            return format!("{marker} {label}: < {shown} >  Left/Right");
        }
        // An edit leaves the saved password alone unless a new one is typed.
        if field.secret && self.editing.is_some() && field.value.as_str().is_empty() {
            return format!("{marker} {label}: (unchanged; type to replace it)");
        }
        // One mark per character typed, so a slip of the finger shows; the characters
        // themselves never reach the screen.
        let value = if field.secret {
            "*".repeat(field.value.len())
        } else {
            field.value.as_str().to_string()
        };
        format!("{marker} {label}: {value}")
    }

    fn footer(&self) -> String {
        let mark = |on: bool| if on { ">" } else { " " };
        format!(
            "{}[Submit]  {}[Test]  {}[Cancel]",
            mark(self.on_submit()),
            mark(self.on_test()),
            mark(self.on_cancel())
        )
    }

    /// What the form says about itself, in a fixed place above the buttons: the last
    /// error or test result, or else how to use the form. The place never moves, so the
    /// message shows wherever the fields are scrolled to and the buttons stay put.
    fn status_rows(&self, width: usize) -> Vec<String> {
        let text = match (self.errors.first(), &self.notice) {
            // The message names fields by their keys, as the app checks them.
            (Some(error), _) => format!(
                "error: {}",
                self.fields
                    .iter()
                    .filter(|field| field.label.contains('_'))
                    .fold(error.clone(), |text, field| {
                        text.replace(field.label.as_str(), &shown_label(&field.label))
                    })
            ),
            (None, Some(notice)) => notice.clone(),
            _ => self
                .focused_label()
                .and_then(field_hint)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    "Enter save  Tab next field  Left/Right pick a value  Esc cancel".into()
                }),
        };
        let room = width.saturating_sub(2).max(1);
        let mut lines = crate::model::wrap_words(&text, room);
        if lines.len() > STATUS_ROWS {
            lines.truncate(STATUS_ROWS);
            let last = lines.pop().unwrap_or_default();
            lines.push(crate::model::truncate_cell(&format!("{last}…"), room));
        }
        lines.resize(STATUS_ROWS, String::new());
        lines.into_iter().map(|line| format!("  {line}")).collect()
    }

    /// The rows the form takes to show every field, its message row and its buttons.
    pub fn content_rows(&self) -> usize {
        self.field_rows().len() + STATUS_ROWS + 1
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = self
            .field_rows()
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        lines.extend(self.status_rows(70));
        lines.push(self.footer());
        lines
    }

    pub fn visible_rows(&self, rows: usize, width: usize) -> Vec<(Option<usize>, String)> {
        let body = self.field_rows();
        let body_rows = rows.saturating_sub(STATUS_ROWS + 1).max(1);
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
        visible.extend(self.status_rows(width).into_iter().map(|line| (None, line)));
        visible.push((None, self.footer()));
        visible
    }

    pub fn visible_lines(&self, rows: usize, width: usize) -> Vec<String> {
        self.visible_rows(rows, width)
            .into_iter()
            .map(|(_, line)| line)
            .collect()
    }
}

/// How a field is named on screen: `ssh_host` reads as "SSH host", next to "name".
fn shown_label(label: &str) -> String {
    if label == "pre_connect" {
        return "pre-connect command".into();
    }
    label
        .split('_')
        .map(|word| match word {
            "tls" => "TLS",
            "ssh" => "SSH",
            "ca" => "CA",
            "cert" => "certificate",
            "secs" => "seconds",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_basic(label: &str) -> bool {
    BASIC_FIELDS.contains(&label)
}

/// What a field is for, when its name does not say: shown where the errors are while the
/// field has the focus. None of the advanced fields said what its values were.
fn field_hint(label: &str) -> Option<&'static str> {
    Some(match label {
        "environment" => {
            "local and development are free; staging and production require verified TLS and confirm writes"
        }
        "group" => "a folder name: connections with the same group sort together",
        "password_command" => {
            "a command that prints the password, as `op read op://vault/db/password`; used in place of the keychain"
        }
        "pre_connect" => {
            "a command run before connecting that opens a tunnel, as `kubectl port-forward svc/db ${port}:5432`; it must keep running in the foreground"
        }
        "tls_mode" => {
            "required encrypts without checking the certificate; verify_ca and verify_full check it, verify_full the host name too"
        }
        "ca_file" => {
            "the PEM file of the certificate authority that signed the server's certificate"
        }
        "client_cert" | "client_key" => {
            "PEM files, both or neither, for servers that ask for a client certificate"
        }
        "ssh_host" => {
            "reach the database through this SSH server; leave empty for a direct connection"
        }
        "proxy_kind" | "proxy_host" | "proxy_port" => {
            "reach the database through a proxy: pick its kind, then its host and port"
        }
        "read_only" => {
            "yes refuses every write on this connection; default follows the environment"
        }
        "confirm_destructive" => "yes asks before DROP, TRUNCATE and DELETE without WHERE",
        "require_verified_tls" => "yes refuses to connect unless the certificate is checked",
        "max_rows" => "the most rows a query returns",
        "timeout_secs" => "seconds before a query is cancelled",
        _ => return None,
    })
}

/// A field whose value is picked with Left and Right from a short list. A value typed
/// into these used to be saved as it came, and failed only when the connection did.
fn is_choice(label: &str) -> bool {
    matches!(
        label,
        "driver"
            | "environment"
            | "tls_mode"
            | "proxy_kind"
            | "read_only"
            | "confirm_destructive"
            | "require_verified_tls"
    )
}

/// What Left and Right walk through for `label`. The empty value is "not set": nothing
/// is saved and the app's own default applies. A value the field already holds that is
/// not on the list -- a custom environment from the command line, `disable` -- stays on
/// it, so editing the connection does not lose it.
fn choice_values(label: &str, current: &str) -> Vec<String> {
    let listed: &[&str] = match label {
        "environment" => ENVIRONMENTS,
        "tls_mode" => &["", "preferred", "required", "verify_ca", "verify_full"],
        "proxy_kind" => &["", "socks5"],
        _ => &["", "true", "false"],
    };
    let mut values: Vec<String> = listed.iter().map(|value| value.to_string()).collect();
    if !values.iter().any(|value| value == current) {
        values.push(current.to_string());
    }
    values
}

/// The words a choice is shown with.
fn choice_label(label: &str, value: &str) -> String {
    match (label, value) {
        ("tls_mode", "") => "no TLS".into(),
        ("proxy_kind", "") => "http".into(),
        (_, "") => "default".into(),
        (_, "true") => "yes".into(),
        (_, "false") => "no".into(),
        _ => value.into(),
    }
}

/// The drivers this build has: DuckDB's engine is large, and built in only with the
/// `duckdb` feature.
fn drivers() -> Vec<&'static str> {
    let mut drivers = vec![
        DriverDescriptor::postgres().id,
        DriverDescriptor::mysql().id,
        DriverDescriptor::mariadb().id,
        DriverDescriptor::sqlite().id,
    ];
    if cfg!(feature = "duckdb") {
        drivers.push(DriverDescriptor::duckdb().id);
    }
    drivers
}

/// A driver this build lists, or one Dexo knows that it left out -- a DuckDB connection
/// edited in a build without DuckDB keeps its driver and its path, where it used to
/// turn into a Postgres form asking for a host.
fn normalize_driver(driver: &str) -> &'static str {
    let id = driver.trim();
    drivers()
        .into_iter()
        .find(|known| *known == id)
        .or_else(|| DriverDescriptor::for_id(id).map(|descriptor| descriptor.id))
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
        if proxy.get("kind").and_then(|kind| kind.as_str()) != Some("http") {
            set_json_field(fields, "proxy_kind", proxy.get("kind"));
        }
        set_json_field(fields, "proxy_host", proxy.get("host"));
        set_json_field(fields, "proxy_port", proxy.get("port"));
    }
    set_json_field(
        fields,
        "password_command",
        profile.config.get("password_command"),
    );
    set_json_field(fields, "pre_connect", profile.config.get("pre_connect"));
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

/// The fields a driver takes. A file driver takes a path where the others take a host,
/// port, database, user and password, and has no transport or TLS to set.
fn blank_fields(driver: &str) -> Vec<FormField> {
    let driver = normalize_driver(driver);
    let descriptor = DriverDescriptor::for_id(driver);
    let driver_field = FormField {
        label: "driver".into(),
        value: driver.into(),
        secret: false,
    };
    let environment = FormField {
        label: "environment".into(),
        value: "local".into(),
        secret: false,
    };
    if descriptor
        .as_ref()
        .is_some_and(|descriptor| descriptor.file)
    {
        return vec![
            field_of("name", false),
            driver_field,
            field_of("path", false),
            environment,
            field_of("group", false),
            field_of("read_only", false),
            field_of("confirm_destructive", false),
            field_of("max_rows", false),
            field_of("timeout_secs", false),
        ];
    }
    let mut fields = vec![
        field_of("name", false),
        driver_field,
        field_of("host", false),
        FormField {
            label: "port".into(),
            value: descriptor
                .as_ref()
                .map(|item| item.default_port.to_string())
                .unwrap_or_default()
                .into(),
            secret: false,
        },
        field_of("database", false),
        field_of("username", false),
        field_of("password", true),
        environment,
        field_of("group", false),
        // Prints the password -- `op read …`, `pass show …` -- in place of the keychain.
        field_of("password_command", false),
        // Opens the way first -- `kubectl port-forward svc/db ${port}:5432` -- and runs
        // while the session does.
        field_of("pre_connect", false),
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
        value: Default::default(),
        secret,
    }
}

fn field(fields: &[FormField], label: &str) -> String {
    fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.as_str().to_string())
        .unwrap_or_default()
}

fn set_field(fields: &mut [FormField], label: &str, value: &str) {
    if let Some(field) = fields.iter_mut().find(|field| field.label == label) {
        field.value.set_text(value);
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

/// A number the user typed into `label`, or nothing when the field is empty. The ports
/// and limits used to fall back to a default when they were not numbers, and the
/// connection was saved with a value nobody typed.
fn number<T: std::str::FromStr>(fields: &[FormField], label: &str) -> Result<Option<T>, String> {
    let text = field(fields, label);
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.trim()
        .parse()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number"))
}

fn to_input(fields: &[FormField]) -> Result<NewConnection, String> {
    let port = number::<u16>(fields, "port")
        .map_err(|_| "port must be a number from 1 to 65535".to_string())?;
    let ssh_port = number::<u16>(fields, "ssh_port")?;
    let proxy_port = number::<u16>(fields, "proxy_port")?;
    let max_rows = number::<u64>(fields, "max_rows")?;
    let timeout_secs = number::<u64>(fields, "timeout_secs")?;
    let policy = ConnectionPolicyOverrides {
        read_only: optional_bool(&field(fields, "read_only")),
        confirm_destructive: optional_bool(&field(fields, "confirm_destructive")),
        require_verified_tls: optional_bool(&field(fields, "require_verified_tls")),
        max_rows,
        timeout_secs,
    };
    let environment = field(fields, "environment");
    // A label that is not one of the four has no policy of its own: the connection
    // would be saved and then refuse to connect, so the form asks for it now.
    if Environment::known(environment.trim()).is_none() && !environment.trim().is_empty() {
        let unset: Vec<&str> = [
            ("read_only", policy.read_only.is_none()),
            ("confirm_destructive", policy.confirm_destructive.is_none()),
            (
                "require_verified_tls",
                policy.require_verified_tls.is_none(),
            ),
            ("max_rows", policy.max_rows.is_none()),
            ("timeout_secs", policy.timeout_secs.is_none()),
        ]
        .into_iter()
        .filter_map(|(label, unset)| unset.then_some(label))
        .collect();
        if let Some(first) = unset.first() {
            return Err(format!(
                "{first} needs a value: environment '{}' is custom, so set {}, or pick local, development, staging or production",
                environment.trim(),
                unset.join(", ")
            ));
        }
    }
    let mut extra = serde_json::Map::new();
    let tls_mode = field(fields, "tls_mode");
    let tls_extras = ["ca_file", "client_cert", "client_key"]
        .into_iter()
        .find(|label| !field(fields, label).trim().is_empty());
    if let (true, Some(label)) = (tls_mode.trim().is_empty(), tls_extras) {
        return Err(format!("{label} is used only when tls_mode is set"));
    }
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
            "port": ssh_port.unwrap_or(22),
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
                "port": proxy_port.unwrap_or(0),
            }),
        );
    }
    let password_command = field(fields, "password_command");
    if !password_command.trim().is_empty() {
        extra.insert(
            "password_command".into(),
            serde_json::Value::String(password_command.trim().into()),
        );
    }
    let pre_connect = field(fields, "pre_connect");
    if !pre_connect.trim().is_empty() {
        extra.insert(
            "pre_connect".into(),
            serde_json::Value::String(pre_connect.trim().into()),
        );
    }
    let path = field(fields, "path");
    if !path.trim().is_empty() {
        extra.insert("path".into(), serde_json::Value::String(path.trim().into()));
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
        policy,
        group_path: if group.trim().is_empty() {
            None
        } else {
            Some(group)
        },
        allow_empty_password: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{ConnectionForm, shown_label};

    #[test]
    fn password_field_is_masked_and_kept_until_the_form_closes() {
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
        let masked = format!("password: {}\n", "*".repeat("SUPER_SECRET_SENTINEL".len()));
        assert!(dump.contains(&masked), "one mark per character:\n{dump}");
        assert!(!dump.contains("SUPER_SECRET_SENTINEL"));
        assert!(dump.contains("Advanced options"));
        assert!(!dump.contains("TLS mode"));
        form.toggle_advanced();
        assert!(form.lines().join("\n").contains("TLS mode"));
        let (input, password) = form.submit().unwrap();
        assert_eq!(input.name, "local-pg");
        assert_eq!(password, "SUPER_SECRET_SENTINEL");
        assert!(!form.lines().join("\n").contains("SUPER_SECRET_SENTINEL"));
        assert!(!format!("{form:?}").contains("SUPER_SECRET_SENTINEL"));
        form.close();
        assert!(
            form.fields
                .iter()
                .all(|field| !field.value.as_str().contains("SUPER_SECRET_SENTINEL"))
        );
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
        let key =
            |code| crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        form.edit(key(crossterm::event::KeyCode::Char('x')));
        form.edit(key(crossterm::event::KeyCode::Backspace));
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value
                .as_str(),
            "postgres"
        );
        form.cycle_choice(1);
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value
                .as_str(),
            "mysql"
        );
        let dump = form.lines().join("\n");
        assert!(dump.contains("< MySQL >"));
        assert!(dump.contains("Left/Right"));
        form.cycle_choice(1);
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value
                .as_str(),
            "mariadb"
        );
        assert!(form.lines().join("\n").contains("< MariaDB >"));
        form.cycle_choice(1);
        let dump = form.lines().join("\n");
        assert!(dump.contains("< SQLite >"));
        assert!(dump.contains("path:"));
        assert!(!dump.contains("host:") && !dump.contains("password:"));
        if cfg!(feature = "duckdb") {
            form.cycle_choice(1);
            let dump = form.lines().join("\n");
            assert!(dump.contains("< DuckDB >"));
            assert!(dump.contains("path:") && !dump.contains("password:"));
        }
        form.cycle_choice(1);
        assert_eq!(
            form.fields
                .iter()
                .find(|field| field.label == "driver")
                .unwrap()
                .value
                .as_str(),
            "postgres"
        );
    }

    /// A file connection submits its path and no password; asking for one would leave
    /// the form unsubmittable, with no field to type it in.
    #[test]
    fn a_sqlite_connection_submits_a_path_without_a_password() {
        let mut form = ConnectionForm::open();
        form.focus = 1;
        form.cycle_choice(3);
        for (label, value) in [("name", "shop"), ("path", "/data/shop.db")] {
            let field = form
                .fields
                .iter_mut()
                .find(|field| field.label == label)
                .unwrap();
            field.value = value.into();
        }
        let (input, password) = form.submit().expect("submits");
        assert_eq!(input.driver, "sqlite");
        assert_eq!(input.extra_config["path"], "/data/shop.db");
        assert!(password.is_empty());
    }

    /// A DuckDB connection edited in a build without DuckDB stays a DuckDB file: it used
    /// to become a Postgres form asking for a host, whose Submit said "path is required".
    #[test]
    fn a_connection_to_a_driver_left_out_of_the_build_keeps_its_form() {
        let profile = dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "warehouse",
            "duckdb",
            "local",
            serde_json::json!({ "path": "/data/warehouse.duckdb" }),
            dexo_app::SecretRef::new("unused".to_string()),
        );
        let mut form = ConnectionForm::open_edit(&profile);
        let dump = form.lines().join("\n");
        assert!(dump.contains("< DuckDB >"), "{dump}");
        assert!(dump.contains("/data/warehouse.duckdb") && !dump.contains("host:"));
        let (input, _) = form.submit().expect("submits");
        assert_eq!(input.driver, "duckdb");
    }

    /// A password manager's command stands in for the password, and comes back when the
    /// connection is edited.
    #[test]
    fn a_password_command_replaces_the_password() {
        let mut form = ConnectionForm::open();
        for (label, value) in [
            ("name", "vault"),
            ("host", "db"),
            ("database", "shop"),
            ("username", "ana"),
            ("password_command", " op read op://dev/shop/password "),
        ] {
            form.set_value(label, value);
        }
        let (input, password) = form.submit().expect("no password needed");
        assert!(password.is_empty());
        assert_eq!(
            input.extra_config["password_command"],
            "op read op://dev/shop/password"
        );
        let profile = dexo_app::test_connection_input(input).unwrap();
        let edit = ConnectionForm::open_edit(&profile);
        assert!(
            edit.lines()
                .join("\n")
                .contains("op read op://dev/shop/password")
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
        assert!(!basic.contains("SSH host:"));

        form.focus = form.advanced_focus_index();
        form.toggle_advanced();
        let advanced = form.lines().join("\n");
        assert!(advanced.contains("[v] Advanced options"));
        assert!(advanced.contains("environment:"));
        // Named as words, like the basic fields; the keys stay the app's.
        for label in [
            "SSH host:",
            "TLS mode:",
            "pre-connect command:",
            "password command:",
        ] {
            assert!(advanced.contains(label), "{advanced}");
        }
        assert!(!advanced.contains("ssh_host"), "{advanced}");
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

    fn focus_on(form: &mut ConnectionForm, label: &str) {
        form.focus = form
            .fields
            .iter()
            .position(|field| field.label == label)
            .unwrap_or_else(|| panic!("no {label} field"));
    }

    fn value(form: &ConnectionForm, label: &str) -> String {
        form.fields
            .iter()
            .find(|field| field.label == label)
            .unwrap()
            .value
            .as_str()
            .to_string()
    }

    /// The port is the driver's own until it is typed over: MySQL used to be offered on
    /// 5432 and PostgreSQL on 3306 after a change of driver.
    #[test]
    fn a_new_driver_brings_its_default_port_unless_the_port_was_typed() {
        let mut form = ConnectionForm::open();
        focus_on(&mut form, "driver");
        assert_eq!(value(&form, "port"), "5432");
        form.cycle_choice(1);
        assert_eq!(value(&form, "driver"), "mysql");
        assert_eq!(value(&form, "port"), "3306");
        form.cycle_choice(-1);
        assert_eq!(value(&form, "port"), "5432");
        form.set_value("port", "6543");
        form.cycle_choice(1);
        assert_eq!(value(&form, "port"), "6543", "a typed port is kept");
    }

    #[test]
    fn submit_names_what_is_missing_in_the_order_of_the_form() {
        let mut form = ConnectionForm::open();
        assert!(form.submit().is_none());
        assert_eq!(
            form.errors,
            ["name, host, database, username and password are required"]
        );
        // The first missing field has the focus, so typing goes where it is needed.
        assert_eq!(form.focused_label(), Some("name"));
        form.set_value("name", "x");
        form.set_value("host", "db");
        form.set_value("database", "d");
        form.set_value("username", "u");
        assert!(form.submit().is_none());
        assert_eq!(form.errors, ["password is required"]);
    }

    /// The message has a row of its own above the buttons, so it shows whatever the form
    /// is scrolled to, and the buttons do not move when it appears.
    #[test]
    fn an_error_shows_above_the_buttons_wherever_the_form_is_scrolled() {
        let mut form = ConnectionForm::open();
        form.set_advanced(true);
        form.set_value("port", "abc");
        focus_on(&mut form, "timeout_secs");
        form.set_error("port must be a number from 1 to 65535".into());
        let with_error = form.visible_lines(12, 70);
        assert!(
            with_error
                .iter()
                .any(|line| line.contains("port must be a number"))
        );
        form.errors.clear();
        let without = form.visible_lines(12, 70);
        assert_eq!(with_error.len(), without.len());
        assert_eq!(
            with_error.iter().position(|line| line.contains("[Submit]")),
            without.iter().position(|line| line.contains("[Submit]")),
            "the buttons stay where they were"
        );
    }

    #[test]
    fn the_modes_of_tls_are_picked_not_typed() {
        let mut form = ConnectionForm::open();
        form.set_advanced(true);
        focus_on(&mut form, "tls_mode");
        let key =
            |code| crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        form.edit(key(crossterm::event::KeyCode::Char('x')));
        assert_eq!(value(&form, "tls_mode"), "", "typing changes nothing");
        let mut seen = Vec::new();
        for _ in 0..5 {
            form.cycle_choice(1);
            seen.push(value(&form, "tls_mode"));
        }
        assert_eq!(
            seen,
            ["preferred", "required", "verify_ca", "verify_full", ""]
        );
        form.cycle_choice(-1);
        assert!(
            form.lines()
                .join("\n")
                .contains("TLS mode: < verify_full >")
        );
    }

    /// An environment the app has no policy for would be saved and then refuse to
    /// connect; the form says so before it saves.
    #[test]
    fn a_custom_environment_asks_for_its_policy_before_it_saves() {
        let mut form = ConnectionForm::open();
        for (label, text) in [
            ("name", "x"),
            ("host", "db"),
            ("database", "d"),
            ("username", "u"),
            ("password", "p"),
            ("environment", "dev"),
        ] {
            form.set_value(label, text);
        }
        assert!(form.submit().is_none());
        let error = &form.errors[0];
        assert!(error.contains("environment 'dev' is custom"), "{error}");
        assert!(
            error.contains("pick local, development, staging or production"),
            "{error}"
        );
        // The field that is needed is on screen.
        assert!(form.advanced);
        assert_eq!(form.focused_label(), Some("read_only"));
        form.set_value("environment", "development");
        assert!(form.submit().is_some());
    }

    #[test]
    fn a_number_that_is_not_one_is_refused_not_replaced() {
        let mut form = ConnectionForm::open();
        for (label, text) in [
            ("name", "x"),
            ("host", "db"),
            ("database", "d"),
            ("username", "u"),
            ("password", "p"),
            ("ssh_host", "bastion"),
            ("ssh_port", "/not/a/port"),
        ] {
            form.set_value(label, text);
        }
        assert!(form.submit().is_none());
        assert_eq!(form.errors, ["ssh_port must be a whole number"]);
    }

    #[test]
    fn long_form_scrolls_to_focus_and_keeps_actions() {
        let mut form = ConnectionForm::open();
        assert!(form.fields.len() > 8);
        form.set_advanced(true);
        form.focus = form.fields.len() - 1;
        let last = shown_label(&form.fields.last().unwrap().label);
        let lines = form.visible_lines(9, 70);
        assert!(lines.iter().any(|line| line.contains(&last)));
        assert!(lines.iter().any(|line| line.contains("[Submit]")));
        assert!(lines.iter().any(|line| line.contains("[Cancel]")));
        assert!(!lines.iter().any(|line| line.contains(" name:")));
        form.focus_next();
        assert!(form.on_submit());
        form.focus_next();
        assert!(form.on_test());
        form.focus_next();
        assert!(form.on_cancel());
        form.focus_next();
        assert_eq!(form.focus, 0);
    }
}
