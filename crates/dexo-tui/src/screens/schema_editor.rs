use crate::model::wrap_line;
use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;
use dexo_app::schema::{Confirmation, DdlPreview};
use dexo_driver_api::{
    ChangeRisk, ColumnSpec, ConstraintKind, ConstraintSpec, ForeignKeySpec, IdentitySpec, IndexDef,
    QualifiedName, RoutineDef, RoutineKind, SchemaChange, TableDef, TableShape, ViewDef,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormKind {
    Table,
    View,
    Routine,
    Trigger,
    Index,
}

#[derive(Clone, PartialEq)]
pub struct FormField {
    pub label: String,
    /// Edited like any single-line input: a cursor, Ctrl+A, the word keys.
    pub value: TextInput,
    pub secret: bool,
}

/// A secret field's value -- a password kept while its form is open -- never shows in
/// a debug print.
impl std::fmt::Debug for FormField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = if self.secret {
            "[redacted]"
        } else {
            self.value.as_str()
        };
        f.debug_struct("FormField")
            .field("label", &self.label)
            .field("value", &value)
            .field("secret", &self.secret)
            .finish()
    }
}

/// Where a preview came from, which is where its Cancel goes back to.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreviewOrigin {
    /// The Schema form, which keeps what was typed.
    #[default]
    Form,
    /// The Security panel, still open underneath.
    Security,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DdlPreviewState {
    pub target: String,
    pub sql: String,
    /// What the change risks, in words; empty when there is nothing to say.
    pub risk: String,
    /// What the driver warns of.
    pub warnings: Vec<String>,
    pub confirmation: Confirmation,
    /// The name typed to confirm a destructive change.
    pub typed: TextInput,
    pub confirmed: bool,
    /// The typed name, or one of the two buttons under it.
    pub footer: FooterFocus,
    /// Why Apply did nothing.
    pub error: Option<String>,
    /// The first SQL line shown, for a statement longer than the dialog.
    pub scroll: usize,
    pub origin: PreviewOrigin,
    /// What Apply applies: the change the preview was made from.
    pub change: Option<SchemaChange>,
}

/// The risk of a change as a sentence. Only what is worth saying is said: a plain CREATE
/// or GRANT risks nothing a person must be told, and printing the flags of the struct
/// (`destructive=false lock=None`) told them nothing.
pub fn describe_risk(risk: &ChangeRisk) -> String {
    match (risk.destructive || risk.data_loss, risk.reversible) {
        (true, false) => "removes data or objects, and cannot be undone".into(),
        (true, true) => "removes data or objects".into(),
        (false, false) => "cannot be undone".into(),
        (false, true) => String::new(),
    }
}

impl DdlPreviewState {
    pub fn from_preview(
        target: String,
        preview: &DdlPreview,
        origin: PreviewOrigin,
        change: Option<SchemaChange>,
    ) -> Self {
        let sql = preview
            .plan
            .statements
            .iter()
            .map(|statement| format!("{};", statement.sql.trim_end_matches(';')))
            .collect::<Vec<_>>()
            .join("\n");
        let mut warnings = preview.plan.warnings.clone();
        warnings.extend(preview.warnings.iter().cloned());
        warnings.dedup();
        Self {
            target,
            sql,
            risk: describe_risk(&preview.risk),
            warnings,
            confirmation: preview.confirmation.clone(),
            typed: TextInput::default(),
            confirmed: false,
            // With a name to type the input has the focus; with none, Apply has it, as
            // Enter applied before there were buttons.
            footer: if matches!(preview.confirmation, Confirmation::TypeTarget(_)) {
                FooterFocus::Input
            } else {
                FooterFocus::Submit
            },
            error: None,
            scroll: 0,
            origin,
            change,
        }
    }

    pub fn needs_typing(&self) -> bool {
        matches!(self.confirmation, Confirmation::TypeTarget(_))
    }

    /// The preview in at most `rows` lines of `width` columns, and how far its SQL can
    /// scroll. The SQL gives way first -- scrolled, with a line that says where it is: the
    /// name to type and the buttons always show. It was cut with "..." and still offered
    /// Apply, for SQL nobody could read.
    pub fn lines(&self, rows: usize, width: usize) -> (Vec<String>, usize) {
        let mut tail = vec![String::new()];
        if let Confirmation::TypeTarget(expected) = &self.confirmation {
            tail.push(format!("Type {expected} to apply this."));
            tail.push(self.typed.inline_line_within(
                "name: ",
                self.footer == FooterFocus::Input,
                width,
            ));
        }
        if let Some(error) = &self.error {
            tail.extend(wrap_line(error, width));
        }
        tail.push(footer_line("Apply", self.footer));
        let mut head = vec![format!("target: {}", self.target)];
        if !self.risk.is_empty() {
            head.push(format!("risk: {}", self.risk));
        }
        for warning in &self.warnings {
            head.extend(wrap_line(&format!("note: {warning}"), width));
        }
        let sql: Vec<String> = self
            .sql
            .lines()
            .flat_map(|line| wrap_line(line, width))
            .collect();
        let room = rows.saturating_sub(head.len() + tail.len()).max(1);
        let mut lines = head;
        let mut max_scroll = 0;
        if sql.len() <= room {
            lines.extend(sql);
        } else {
            // One row of the room says where the window is.
            let shown = room.saturating_sub(1).max(1);
            max_scroll = sql.len() - shown;
            let from = self.scroll.min(max_scroll);
            lines.extend(sql[from..from + shown].iter().cloned());
            lines.push(format!(
                "  lines {}-{} of {} \u{b7} PageUp/PageDown scroll",
                from + 1,
                from + shown,
                sql.len()
            ));
        }
        lines.extend(tail);
        (lines, max_scroll)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchemaEditor {
    /// The form used to be the DDL tab's fallback content, so nothing ever opened it.
    pub open: bool,
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
    pub raw_sql: String,
    pub preview: Option<DdlPreviewState>,
    pub form_diff: Option<String>,
    /// The fields, or one of the two buttons under them.
    pub footer: crate::widgets::form::FooterFocus,
    /// The change a preview has been asked for, and where it came from: what Apply will
    /// apply, and where Cancel goes back to.
    pub pending: Option<(PreviewOrigin, SchemaChange)>,
    /// Where the change being applied came from, to bring its form back if it fails.
    pub applying: Option<PreviewOrigin>,
}

/// What the form opens on: a name that is no table's, so pressing Enter twice does not try
/// to create one that exists.
pub const NEW_TABLE: &str = "new_table";

impl Default for SchemaEditor {
    fn default() -> Self {
        Self::table_form(NEW_TABLE)
    }
}

impl SchemaEditor {
    pub fn table_form(target: impl Into<String>) -> Self {
        Self {
            open: false,
            kind: FormKind::Table,
            fields: vec![
                FormField {
                    label: "target".into(),
                    value: TextInput::new(target),
                    secret: false,
                },
                FormField {
                    label: "columns".into(),
                    value: "id bigint identity pk".into(),
                    secret: false,
                },
                FormField {
                    label: "constraints".into(),
                    value: TextInput::default(),
                    secret: false,
                },
                FormField {
                    label: "foreign_keys".into(),
                    value: TextInput::default(),
                    secret: false,
                },
            ],
            focus: 0,
            errors: Vec::new(),
            raw_sql: String::new(),
            preview: None,
            form_diff: None,
            footer: crate::widgets::form::FooterFocus::Input,
            pending: None,
            applying: None,
        }
    }

    pub fn view_form(target: impl Into<String>) -> Self {
        let mut editor = Self::table_form(target);
        editor.kind = FormKind::View;
        editor.fields = vec![
            FormField {
                label: "target".into(),
                value: editor.field("target").into(),
                secret: false,
            },
            FormField {
                label: "sql".into(),
                value: "SELECT 1".into(),
                secret: false,
            },
            FormField {
                label: "materialized".into(),
                value: "false".into(),
                secret: false,
            },
        ];
        editor
    }

    pub fn routine_form(target: impl Into<String>, trigger: bool) -> Self {
        let mut editor = Self::table_form(target);
        editor.kind = if trigger {
            FormKind::Trigger
        } else {
            FormKind::Routine
        };
        editor.fields = vec![
            FormField {
                label: "target".into(),
                value: editor.field("target").into(),
                secret: false,
            },
            FormField {
                label: "arguments".into(),
                value: if trigger { "" } else { "n integer" }.into(),
                secret: false,
            },
            FormField {
                label: "body".into(),
                value: if trigger {
                    "EXECUTE FUNCTION public.tg_fn()"
                } else {
                    "SELECT n + 1"
                }
                .into(),
                secret: false,
            },
            FormField {
                label: "table".into(),
                value: if trigger { "public.orders" } else { "" }.into(),
                secret: false,
            },
        ];
        editor
    }

    pub fn field(&self, label: &str) -> &str {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.as_str())
            .unwrap_or("")
    }

    pub fn set_field(&mut self, label: &str, value: impl Into<String>) {
        if let Some(field) = self.fields.iter_mut().find(|field| field.label == label) {
            field.value.set_text(value);
        }
    }

    /// Opened on raw SQL by Apply Raw DDL: Submit runs that SQL, whatever the fields say.
    pub fn is_raw(&self) -> bool {
        !self.raw_sql.is_empty()
    }

    /// What the form's Submit does: preview the DDL its fields make, or run the SQL it
    /// was opened with.
    pub fn submit_label(&self) -> &'static str {
        if self.raw_sql.is_empty() {
            "Preview"
        } else {
            "Run"
        }
    }

    /// Hands `key` to the focused field; false when it is not an input's key.
    pub fn edit_focused(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.fields
            .get_mut(self.focus)
            .is_some_and(|field| field.value.handle_key(key))
    }

    pub fn focus_next(&mut self) {
        if !self.fields.is_empty() {
            self.focus = (self.focus + 1) % self.fields.len();
        }
    }

    pub fn focus_prev(&mut self) {
        if !self.fields.is_empty() {
            self.focus = if self.focus == 0 {
                self.fields.len() - 1
            } else {
                self.focus - 1
            };
        }
    }

    pub fn validate(&mut self) -> bool {
        self.errors.clear();
        if self.field("target").trim().is_empty() {
            self.errors.push("target is required".into());
        }
        if self.kind == FormKind::Table {
            if self.field("columns").trim().is_empty() {
                self.errors.push("columns are required".into());
            } else if let Err(problems) = self.table_def() {
                self.errors.extend(problems);
            }
        }
        if self.kind == FormKind::View && self.field("sql").trim().is_empty() {
            self.errors.push("view sql is required".into());
        }
        self.errors.is_empty()
    }

    pub fn to_change(&self) -> Result<SchemaChange, Vec<String>> {
        if self.field("target").trim().is_empty() {
            return Err(vec!["target is required".into()]);
        }
        let target = parse_target(self.field("target"));
        match self.kind {
            FormKind::Table => Ok(SchemaChange::CreateTable {
                def: self.table_def()?,
                target,
            }),
            FormKind::View => Ok(SchemaChange::CreateView {
                target,
                def: ViewDef {
                    sql: self.field("sql").to_string(),
                    materialized: self.field("materialized") == "true",
                    replace: false,
                },
            }),
            FormKind::Routine => Ok(SchemaChange::AlterRoutine {
                target,
                def: RoutineDef {
                    kind: RoutineKind::Function,
                    arguments: self.field("arguments").to_string(),
                    language: "sql".into(),
                    body: self.field("body").to_string(),
                    returns: Some("integer".into()),
                    volatility: None,
                    table: None,
                    timing: None,
                    schedule: None,
                },
            }),
            FormKind::Trigger => Ok(SchemaChange::AlterRoutine {
                target,
                def: RoutineDef {
                    kind: RoutineKind::Trigger,
                    arguments: String::new(),
                    language: "sql".into(),
                    body: self.field("body").to_string(),
                    returns: None,
                    volatility: None,
                    table: Some(parse_target(self.field("table"))),
                    timing: Some("BEFORE INSERT".into()),
                    schedule: None,
                },
            }),
            FormKind::Index => Ok(SchemaChange::CreateIndex {
                target: QualifiedName::new(None::<String>, None::<String>, "idx"),
                def: IndexDef {
                    table: parse_target(self.field("target")),
                    columns: vec![QualifiedName::new(None::<String>, None::<String>, "id")],
                    unique: false,
                    concurrently: false,
                    method: None,
                    include: vec![],
                    predicate: None,
                },
            }),
        }
    }

    /// The table the fields describe, or what in them is not understood. Every field is
    /// read; none is dropped silently, as `defaults`, `indexes` and the rest were.
    fn table_def(&self) -> Result<TableDef, Vec<String>> {
        if self.field("target").trim().is_empty() {
            // Said once already, by `validate`.
            return Err(Vec::new());
        }
        let table = parse_target(self.field("target").trim())
            .object()
            .to_string();
        let (columns, mut constraints, mut problems) = parse_columns(self.field("columns"), &table);
        match parse_constraints(self.field("constraints"), &table) {
            Ok(more) => constraints.extend(more),
            Err(more) => problems.extend(more),
        }
        match parse_foreign_keys(self.field("foreign_keys"), &table) {
            Ok(more) => constraints.extend(more),
            Err(more) => problems.extend(more),
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(TableDef {
            shape: TableShape::Table,
            columns,
            constraints,
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        })
    }

    /// Previews the form's own change.
    pub fn open_preview(&mut self, preview: DdlPreview) {
        let change = self.to_change().ok();
        self.open_preview_for(preview, PreviewOrigin::Form, change);
    }

    pub fn open_preview_for(
        &mut self,
        preview: DdlPreview,
        origin: PreviewOrigin,
        change: Option<SchemaChange>,
    ) {
        let target = match (&change, origin) {
            (Some(change), PreviewOrigin::Security) => change.target().display_unquoted(),
            _ => self.field("target").to_string(),
        };
        self.preview = Some(DdlPreviewState::from_preview(
            target, &preview, origin, change,
        ));
    }

    pub fn confirm_typed(&mut self) {
        let Some(preview) = &mut self.preview else {
            return;
        };
        if let Confirmation::TypeTarget(expected) = &preview.confirmation {
            preview.confirmed = preview.typed.as_str() == expected;
        } else {
            preview.confirmed = true;
        }
    }

    /// Opens the form on `sql` to run as it is. The statement is shown, and the guard of
    /// the connection judges it when it runs; the dialog used to dump the Preview form's
    /// old fields beside it and label an ADD COLUMN destructive.
    pub fn apply_raw(&mut self, sql: String) {
        self.form_diff = None;
        self.raw_sql = sql;
    }

    /// The lines at the width of the longest of them: nothing cut.
    pub fn lines(&self) -> Vec<String> {
        let widest = self
            .fields
            .iter()
            .map(|field| field.label.len() + field.value.len() + 8)
            .chain([
                self.raw_sql.lines().map(str::len).max().unwrap_or(0) + 4,
                100,
            ])
            .max()
            .unwrap_or(100);
        self.lines_within(widest)
    }

    /// The form's lines in `width` columns: a field's value that does not fit scrolls with
    /// its cursor, and the others end in an ellipsis -- they were cut at the border with
    /// the end of the text, where the cursor is, out of sight.
    pub fn lines_within(&self, width: usize) -> Vec<String> {
        if self.is_raw() {
            let mut lines = vec!["Run this SQL as it is:".to_string(), String::new()];
            let sql: Vec<String> = self
                .raw_sql
                .trim()
                .lines()
                .flat_map(|line| wrap_line(line, width.saturating_sub(2)))
                .collect();
            let shown = sql.len().min(RAW_LINES);
            lines.extend(sql[..shown].iter().map(|line| format!("  {line}")));
            if sql.len() > shown {
                lines.push(format!("  ... and {} more lines", sql.len() - shown));
            }
            for error in &self.errors {
                lines.push(format!("error: {error}"));
            }
            return lines;
        }
        let mut lines = vec![format!("New {}:", kind_label(self.kind))];
        let on_fields = self.footer == crate::widgets::form::FooterFocus::Input;
        // Raw SQL runs as it is: fields shown beside it took typing that changed nothing.
        let fields = if self.is_raw() {
            &[][..]
        } else {
            &self.fields[..]
        };
        for (index, field) in fields.iter().enumerate() {
            let marker = if on_fields && index == self.focus {
                ">"
            } else {
                " "
            };
            let label = format!("{marker} {}: ", field.label);
            // One mark per character typed, so a slip of the finger shows; the characters
            // themselves never reach the screen.
            if field.secret {
                lines.push(format!("{label}{}", "*".repeat(field.value.len())));
            } else if on_fields && index == self.focus {
                // The stretch around the cursor, which the terminal's own cursor marks.
                let room =
                    width.saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()));
                let (shown, _) = field.value.window(room.max(1));
                lines.push(format!("{label}{}", shown.trim_end()));
            } else {
                lines.push(format!(
                    "{label}{}",
                    crate::model::truncate_cell(
                        field.value.as_str(),
                        width.saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()))
                    )
                ));
            }
        }
        for error in &self.errors {
            lines.extend(wrap_line(&format!("error: {error}"), width));
        }
        if self.kind == FormKind::Table {
            // What the fields take, since nothing else says: they are written in a few
            // words of their own.
            lines.push(String::new());
            lines.extend(TABLE_HINTS.iter().flat_map(|hint| wrap_line(hint, width)));
        }
        lines
    }
}

/// Lines of raw SQL the dialog shows before saying how many more there are.
const RAW_LINES: usize = 12;

const TABLE_HINTS: [&str; 4] = [
    "  columns: name type [pk] [not null] [unique] [identity] [default x], ...",
    "  a type may hold commas: numeric(10,2)",
    "  constraints: check (price > 0), unique (a, b)",
    "  foreign_keys: customer_id references customers(id)",
];

fn kind_label(kind: FormKind) -> &'static str {
    match kind {
        FormKind::Table => "table",
        FormKind::View => "view",
        FormKind::Routine => "routine",
        FormKind::Trigger => "trigger",
        FormKind::Index => "index",
    }
}

fn parse_target(input: &str) -> QualifiedName {
    dexo_app::parse_qualified(input)
}

/// Splits `text` at the commas that are not inside parentheses or quotes, so the comma
/// in `numeric(10,2)` stays in its type.
fn split_commas(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let (mut depth, mut quote) = (0usize, None::<char>);
    for ch in text.chars() {
        match (quote, ch) {
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Splits `text` at spaces, keeping a parenthesized or quoted stretch in one piece.
fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let (mut depth, mut quote) = (0usize, None::<char>);
    for ch in text.chars() {
        match (quote, ch) {
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ch) if ch.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn column_ident(name: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, None::<String>, name.trim_matches('"'))
}

/// Words a type may go on with after its first: `double precision`, `character varying`,
/// `timestamp with time zone`, `interval day to second`.
const TYPE_WORDS: [&str; 15] = [
    "precision",
    "varying",
    "with",
    "without",
    "time",
    "zone",
    "to",
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "unsigned",
    "signed",
];

/// The columns the field names: `name type [pk] [not null] [unique] [identity] [autoinc]
/// [default x]`, separated by commas. Returns the columns, the constraints they ask for
/// (`unique`, a key over several `pk` columns), and what was not understood: a word that
/// is none of these used to be dropped, and `not null` was never read at all.
fn parse_columns(spec: &str, table: &str) -> (Vec<ColumnSpec>, Vec<ConstraintSpec>, Vec<String>) {
    let mut columns = Vec::new();
    let mut constraints = Vec::new();
    let mut problems = Vec::new();
    for part in split_commas(spec) {
        let words = split_words(&part);
        let name = words.first().cloned().unwrap_or_default();
        let mut at = 1;
        let mut data_type: Vec<String> = Vec::new();
        while let Some(word) = words.get(at) {
            let lower = word.to_ascii_lowercase();
            let is_type_word = data_type.is_empty()
                || word.starts_with('(')
                || word.starts_with('[')
                || word.ends_with("[]")
                || TYPE_WORDS.contains(&lower.as_str());
            if !is_type_word {
                break;
            }
            data_type.push(word.clone());
            at += 1;
        }
        if data_type.is_empty() {
            problems.push(format!(
                "column {name} has no type: write `{name} text`, for example"
            ));
            continue;
        }
        let mut column = ColumnSpec {
            name: column_ident(&name),
            data_type: data_type.join(" "),
            nullable: true,
            default_sql: None,
            identity: None,
            auto_increment: false,
            generated: None,
            primary_key: false,
        };
        let mut unique = false;
        while let Some(word) = words.get(at) {
            let lower = word.to_ascii_lowercase();
            let next = words.get(at + 1).map(|word| word.to_ascii_lowercase());
            match (lower.as_str(), next.as_deref()) {
                ("pk", _) => column.primary_key = true,
                ("primary", Some("key")) => {
                    column.primary_key = true;
                    at += 1;
                }
                ("not", Some("null")) => {
                    column.nullable = false;
                    at += 1;
                }
                ("null", _) => column.nullable = true,
                ("unique", _) => unique = true,
                ("identity", _) => column.identity = Some(IdentitySpec { always: true }),
                ("autoinc" | "auto_increment", _) => column.auto_increment = true,
                ("default", Some(_)) => {
                    column.default_sql = Some(words[at + 1].clone());
                    at += 1;
                }
                ("default", None) => {
                    problems.push(format!("column {name}: default needs a value"));
                }
                _ => problems.push(format!(
                    "column {name}: `{word}` is not understood; use pk, not null, unique, identity, autoinc or default <value>"
                )),
            }
            at += 1;
        }
        if column.primary_key {
            column.nullable = false;
        }
        if unique {
            constraints.push(ConstraintSpec {
                name: column_ident(&format!("{table}_{name}_key")),
                kind: ConstraintKind::Unique {
                    columns: vec![column_ident(&name)],
                },
            });
        }
        columns.push(column);
    }
    // Several columns marked pk are one key over all of them, not a key each.
    let keys: Vec<QualifiedName> = columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.clone())
        .collect();
    if keys.len() > 1 {
        for column in &mut columns {
            column.primary_key = false;
        }
        constraints.push(ConstraintSpec {
            name: column_ident(&format!("{table}_pkey")),
            kind: ConstraintKind::PrimaryKey { columns: keys },
        });
    }
    (columns, constraints, problems)
}

/// Column names in `text`: `a`, or `(a, b)`.
fn column_list(text: &str) -> Vec<QualifiedName> {
    text.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(column_ident)
        .collect()
}

fn names_joined(columns: &[QualifiedName]) -> String {
    columns
        .iter()
        .map(QualifiedName::object)
        .collect::<Vec<_>>()
        .join("_")
}

/// `check (price > 0), unique (a, b), primary key (a, b)`.
fn parse_constraints(spec: &str, table: &str) -> Result<Vec<ConstraintSpec>, Vec<String>> {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for (index, part) in split_commas(spec).into_iter().enumerate() {
        let lower = part.to_ascii_lowercase();
        let rest_of = |keyword: &str| part[keyword.len()..].trim().to_string();
        if lower.starts_with("check") {
            let expression = rest_of("check");
            let expression = expression
                .strip_prefix('(')
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(&expression)
                .trim()
                .to_string();
            if expression.is_empty() {
                problems.push(format!("constraint `{part}`: check needs a condition"));
                continue;
            }
            found.push(ConstraintSpec {
                name: column_ident(&format!("{table}_check{}", index + 1)),
                kind: ConstraintKind::Check { expression },
            });
        } else if lower.starts_with("unique") || lower.starts_with("primary key") {
            let primary = lower.starts_with("primary key");
            let columns = column_list(&rest_of(if primary { "primary key" } else { "unique" }));
            if columns.is_empty() {
                problems.push(format!(
                    "constraint `{part}`: name the columns, as in (a, b)"
                ));
                continue;
            }
            let suffix = if primary { "pkey" } else { "key" };
            found.push(ConstraintSpec {
                name: column_ident(&format!("{table}_{}_{suffix}", names_joined(&columns))),
                kind: if primary {
                    ConstraintKind::PrimaryKey { columns }
                } else {
                    ConstraintKind::Unique { columns }
                },
            });
        } else {
            problems.push(format!(
                "constraint `{part}` is not understood; use check (...), unique (...) or primary key (...)"
            ));
        }
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

/// `customer_id references customers(id), (a, b) references t(x, y)`.
fn parse_foreign_keys(spec: &str, table: &str) -> Result<Vec<ConstraintSpec>, Vec<String>> {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for part in split_commas(spec) {
        let lower = part.to_ascii_lowercase();
        let Some(at) = lower.find(" references ") else {
            problems.push(format!(
                "foreign key `{part}`: write it as column references table(column)"
            ));
            continue;
        };
        let local = column_list(&part[..at]);
        let target = part[at + " references ".len()..].trim();
        let (referenced_table, referenced_columns) = match target.split_once('(') {
            Some((name, columns)) => (name.trim(), column_list(columns)),
            None => (target, Vec::new()),
        };
        if local.is_empty() || referenced_table.is_empty() {
            problems.push(format!(
                "foreign key `{part}`: write it as column references table(column)"
            ));
            continue;
        }
        if referenced_columns.is_empty() {
            problems.push(format!(
                "foreign key `{part}`: name the referenced column, as {referenced_table}(id)"
            ));
            continue;
        }
        found.push(ConstraintSpec {
            name: column_ident(&format!("{table}_{}_fkey", names_joined(&local))),
            kind: ConstraintKind::ForeignKey(ForeignKeySpec {
                columns: local,
                referenced_table: parse_target(referenced_table),
                referenced_columns,
            }),
        });
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeRisk, ConstraintKind, ConstraintSpec, DdlPreviewState, FormKind, PreviewOrigin,
        SchemaEditor,
    };
    use crate::action::Action;
    use crate::model::Model;
    use crate::update;
    use dexo_driver_api::SchemaChange;

    #[test]
    fn table_form_validates_and_builds_typed_change() {
        let mut editor = SchemaEditor::table_form("public.orders");
        assert!(editor.validate());
        assert!(matches!(
            editor.to_change(),
            Ok(SchemaChange::CreateTable { .. })
        ));
        editor.set_field("target", "");
        assert!(!editor.validate());
        assert!(editor.errors.iter().any(|error| error.contains("target")));
    }

    #[test]
    fn view_routine_trigger_forms_exist() {
        assert_eq!(SchemaEditor::view_form("public.v").kind, FormKind::View);
        assert_eq!(
            SchemaEditor::routine_form("public.add1", false).kind,
            FormKind::Routine
        );
        assert_eq!(
            SchemaEditor::routine_form("orders_tg", true).kind,
            FormKind::Trigger
        );
    }

    /// The dialog showed the Preview form's old fields as `-` lines and a `key=value`
    /// dump of its risk, with the statement twice and the `;` dropped.
    #[test]
    fn raw_ddl_shows_the_statement_and_nothing_of_the_form() {
        let mut editor = SchemaEditor::table_form("public.qa_items");
        editor.set_field("columns", "id bigint pk");
        editor.apply_raw("ALTER TABLE qa_items ADD COLUMN note text;".into());
        assert!(editor.is_raw());
        let shown = editor.lines().join("\n");
        assert!(
            shown.contains("ALTER TABLE qa_items ADD COLUMN note text;"),
            "{shown}"
        );
        for leftover in ["target", "columns", "destructive", "risk", "- ", "="] {
            assert!(!shown.contains(leftover), "{leftover}: {shown}");
        }
        assert_eq!(shown.matches("ADD COLUMN").count(), 1, "{shown}");
    }

    /// `numeric(10,2)` is one type, `not null`, `unique` and `default` are read, and a
    /// word that is none of them is said, not dropped.
    #[test]
    fn the_columns_field_is_read_whole() {
        let mut editor = SchemaEditor::table_form("public.items");
        editor.set_field(
            "columns",
            "id bigint identity pk, name text not null unique, price numeric(10,2) default 0, at timestamp with time zone default now()",
        );
        let Ok(SchemaChange::CreateTable { def, .. }) = editor.to_change() else {
            panic!("{:?}", editor.to_change());
        };
        let described: Vec<(String, String, bool, Option<String>)> = def
            .columns
            .iter()
            .map(|c| {
                (
                    c.name.object().into(),
                    c.data_type.clone(),
                    c.nullable,
                    c.default_sql.clone(),
                )
            })
            .collect();
        assert_eq!(
            described,
            [
                ("id".into(), "bigint".into(), false, None),
                ("name".into(), "text".into(), false, None),
                (
                    "price".into(),
                    "numeric(10,2)".into(),
                    true,
                    Some("0".into())
                ),
                (
                    "at".into(),
                    "timestamp with time zone".into(),
                    true,
                    Some("now()".into())
                ),
            ]
        );
        assert!(def.columns[0].primary_key && def.columns[0].identity.is_some());
        assert_eq!(def.constraints.len(), 1, "the unique column's key");

        editor.set_field("columns", "id bigint pk, name text required");
        assert!(!editor.validate());
        assert!(
            editor
                .errors
                .iter()
                .any(|error| error.contains("`required` is not understood")),
            "{:?}",
            editor.errors
        );
        editor.set_field("columns", "id");
        assert!(!editor.validate());
        assert!(
            editor.errors[0].contains("has no type"),
            "{:?}",
            editor.errors
        );
    }

    /// Two columns marked pk are one key over both, not two `PRIMARY KEY`s.
    #[test]
    fn several_pk_columns_make_one_composite_key() {
        let mut editor = SchemaEditor::table_form("public.order_items");
        editor.set_field("columns", "order_id int pk, product_id int pk, qty int");
        let Ok(SchemaChange::CreateTable { def, .. }) = editor.to_change() else {
            panic!("{:?}", editor.to_change());
        };
        assert!(def.columns.iter().all(|column| !column.primary_key));
        assert!(matches!(
            &def.constraints[..],
            [ConstraintSpec { kind: ConstraintKind::PrimaryKey { columns }, .. }] if columns.len() == 2
        ));
    }

    /// `constraints` and `foreign_keys` reach the DDL; they were ignored by the preview
    /// and by Apply.
    #[test]
    fn constraints_and_foreign_keys_are_part_of_the_change() {
        let mut editor = SchemaEditor::table_form("public.qa_items");
        editor.set_field("columns", "id bigint pk, customer_id int, label text");
        editor.set_field(
            "constraints",
            "check (length(label) > 0), unique (customer_id, label)",
        );
        editor.set_field("foreign_keys", "customer_id references customers(id)");
        let Ok(SchemaChange::CreateTable { def, .. }) = editor.to_change() else {
            panic!("{:?}", editor.to_change());
        };
        let kinds: Vec<&str> = def
            .constraints
            .iter()
            .map(|c| match &c.kind {
                ConstraintKind::Check { expression } if expression == "length(label) > 0" => {
                    "check"
                }
                ConstraintKind::Unique { columns } if columns.len() == 2 => "unique",
                ConstraintKind::ForeignKey(fk) if fk.referenced_table.object() == "customers" => {
                    "fk"
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(kinds, ["check", "unique", "fk"]);

        editor.set_field("foreign_keys", "customer_id references customers");
        assert!(!editor.validate());
        assert!(
            editor.errors[0].contains("name the referenced column"),
            "{:?}",
            editor.errors
        );
        editor.set_field("foreign_keys", "");
        editor.set_field("constraints", "primary");
        assert!(!editor.validate());
    }

    /// The form opens on a name no table has, and says what its fields take.
    #[test]
    fn the_form_opens_on_a_new_name_and_explains_its_fields() {
        let editor = SchemaEditor::default();
        assert_eq!(editor.field("target"), "new_table");
        assert!(
            editor
                .fields
                .iter()
                .all(|f| !["defaults", "indexes"].contains(&f.label.as_str()))
        );
        let shown = editor.lines().join("\n");
        assert!(
            shown.contains("[not null]") && shown.contains("numeric(10,2)"),
            "{shown}"
        );
    }

    /// A long DDL scrolls under its header and buttons, and says which lines it shows.
    #[test]
    fn a_long_preview_scrolls_and_keeps_its_buttons() {
        use dexo_app::schema::Confirmation;
        let sql = (0..30)
            .map(|n| format!("  c{n:02} int"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut preview = DdlPreviewState {
            target: "public.wide".into(),
            sql,
            risk: String::new(),
            warnings: Vec::new(),
            confirmation: Confirmation::None,
            typed: Default::default(),
            confirmed: false,
            footer: crate::widgets::form::FooterFocus::Submit,
            error: None,
            scroll: 0,
            origin: PreviewOrigin::Form,
            change: None,
        };
        let (first, max) = preview.lines(14, 60);
        assert_eq!(first.len(), 14);
        assert!(first.iter().any(|line| line.contains("c00")));
        assert!(
            first
                .iter()
                .any(|line| line.contains("lines 1-") && line.contains("of 30")),
            "{first:?}"
        );
        assert!(first.last().unwrap().contains("[Apply]"));
        assert_eq!(max, 30 - 10, "{first:?}");
        preview.scroll = 999;
        let (last, _) = preview.lines(14, 60);
        assert!(last.iter().any(|line| line.contains("c29")), "{last:?}");
        assert!(last.iter().any(|line| line.contains("of 30")));
        assert!(last.last().unwrap().contains("[Apply]"));
        // Short enough: all of it, no indicator.
        preview.sql = "CREATE TABLE t (id int);".into();
        let (short, max) = preview.lines(14, 60);
        assert_eq!(max, 0);
        assert!(short.iter().all(|line| !line.contains("PageDown")));
    }

    /// The preview says in words what a change risks, and nothing when it risks nothing.
    #[test]
    fn the_risk_is_a_sentence_or_nothing() {
        use super::describe_risk;
        assert_eq!(describe_risk(&ChangeRisk::default()), "");
        assert_eq!(
            describe_risk(&ChangeRisk {
                destructive: true,
                data_loss: true,
                lock_level: dexo_driver_api::LockLevel::AccessExclusive,
                reversible: false,
            }),
            "removes data or objects, and cannot be undone"
        );
    }

    #[test]
    fn reducer_opens_preview() {
        let mut model = Model {
            schema_editor: SchemaEditor::table_form("prod.public.orders"),
            ..Model::default()
        };
        update(&mut model, Action::OpenDdlPreview);
        assert!(model.schema_editor.preview.is_some());
    }

    /// On a short terminal the focus walked into fields cut off under the buttons:
    /// `QQ` typed there landed in `foreign_keys`, out of sight. The fields scroll to
    /// keep the focused one in view, and only the rows drawn as fields take a click.
    #[test]
    fn a_short_terminal_scrolls_the_form_to_the_focused_field() {
        use crate::mouse::HitTarget;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model {
            schema_editor: SchemaEditor::table_form("public.t"),
            ..Model::default()
        };
        model.apply_size(100, 9);
        model.schema_editor.open = true;
        for _ in 0..3 {
            update(&mut model, key(KeyCode::Down));
        }
        assert_eq!(
            model.schema_editor.fields[model.schema_editor.focus].label,
            "foreign_keys"
        );
        update(&mut model, key(KeyCode::Char('Q')));
        update(&mut model, key(KeyCode::Char('Q')));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 9)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        let frame = terminal
            .draw(|frame| crate::render::render(frame, &model, &mut hits))
            .unwrap();
        let rows: Vec<String> = frame
            .buffer
            .content()
            .chunks(100)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        let screen = rows.join("\n");
        assert!(screen.contains("> foreign_keys: QQ"), "{screen}");
        let (_, y) = hits.center(HitTarget::FormField(3));
        assert!(rows[usize::from(y)].contains("foreign_keys"), "{screen}");
        // The target scrolled out of view, and no field lies under the buttons.
        assert_eq!(hits.center(HitTarget::FormField(0)), (0, 0));
        for (y, row) in rows.iter().enumerate() {
            if row.contains("[Cancel]") || row.contains("esc cancel") {
                for x in 0..100 {
                    assert!(
                        !matches!(hits.at(x, y as u16), Some(HitTarget::FormField(_))),
                        "a field under row {y}: {row}"
                    );
                }
            }
        }
    }

    /// Apply Raw DDL showed the fields, took typing into them and marked them, and Run
    /// ignored all of it. Errors from an earlier form stayed on screen in either mode.
    #[test]
    fn raw_ddl_mode_has_no_fields_to_type_in_and_no_old_errors() {
        use crate::widgets::form::FooterFocus;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model::default();
        model.active_document_mut().sql = dexo_sql::SqlDocument::new("CREATE INDEX i ON t (id)");
        model.schema_editor.errors = vec!["target is required".into()];
        update(&mut model, Action::ApplyRawDdl);
        assert!(model.schema_editor.open);
        assert!(model.schema_editor.errors.is_empty(), "an old error stayed");
        assert_eq!(model.schema_editor.footer, FooterFocus::Submit);
        let before = model.schema_editor.fields.clone();
        for code in [
            KeyCode::Char('x'),
            KeyCode::Up,
            KeyCode::Char('y'),
            KeyCode::Down,
        ] {
            update(&mut model, key(code));
        }
        assert_eq!(model.schema_editor.fields, before, "typing reached a field");
        assert_ne!(model.schema_editor.footer, FooterFocus::Input);
        let screen = crate::render::render_to_string(&model, 100, 30);
        assert!(!screen.contains("> target:"), "{screen}");
        assert!(!screen.contains("  columns:"), "{screen}");
        assert!(screen.contains("[Run]"), "{screen}");

        // The fields' own Preview starts without the errors of the last one.
        update(&mut model, key(KeyCode::Esc));
        model.schema_editor.errors = vec!["columns are required".into()];
        model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
        update(&mut model, Action::OpenPalette);
        update(&mut model, Action::PaletteQuery("Preview DDL".into()));
        let entries = crate::palette::palette_entries(&model);
        model.palette.selected = crate::palette::filter_entries(&entries, "Preview DDL")
            .iter()
            .position(|entry| entry.id == "schema.preview")
            .expect("Preview DDL is in the palette");
        update(&mut model, Action::PaletteSelect);
        assert!(model.schema_editor.open);
        assert!(!model.schema_editor.is_raw());
        assert!(model.schema_editor.errors.is_empty(), "an old error stayed");
    }

    /// The preview after the form had no Submit and Cancel, and what was typed to
    /// confirm a destructive change never showed.
    #[test]
    fn the_ddl_preview_has_buttons_and_shows_the_typed_name() {
        use super::DdlPreviewState;
        use crate::widgets::form::FooterFocus;
        use crossterm::event::{
            KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };
        use dexo_app::schema::Confirmation;
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let preview = DdlPreviewState {
            target: "public.orders".into(),
            sql: "DROP TABLE public.orders".into(),
            risk: "removes data or objects, and cannot be undone".into(),
            warnings: Vec::new(),
            confirmation: Confirmation::TypeTarget("public.orders".into()),
            typed: Default::default(),
            confirmed: false,
            footer: FooterFocus::Input,
            error: None,
            scroll: 0,
            origin: super::PreviewOrigin::Form,
            change: None,
        };
        let mut model = Model::default();
        model.apply_size(100, 30);
        model.schema_editor.preview = Some(preview.clone());
        for ch in "public.ord".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        let screen = crate::render::render_to_string(&model, 100, 30);
        assert!(screen.contains("name: public.ord█"), "{screen}");
        assert!(screen.contains(" [Apply]   [Cancel]"), "{screen}");
        // Ctrl+A selects the name, in reverse, and typing replaces it.
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        let frame = terminal
            .draw(|frame| crate::render::render(frame, &model, &mut hits))
            .unwrap();
        let reversed: String = frame
            .buffer
            .content()
            .iter()
            .filter(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED))
            .map(|cell| cell.symbol())
            .collect();
        assert!(reversed.contains("public.ord"), "{reversed:?}");
        for ch in "public.ord".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        // A name that does not match applies nothing, and says so.
        update(&mut model, key(KeyCode::Enter));
        let current = model.schema_editor.preview.as_ref().expect("still open");
        assert!(current.error.is_some());
        for ch in "ers".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        assert!(model.schema_editor.preview.as_ref().unwrap().confirmed);

        // The arrows walk to the buttons, and Enter on Cancel cancels.
        update(&mut model, key(KeyCode::Down));
        update(&mut model, key(KeyCode::Right));
        let current = model.schema_editor.preview.as_ref().unwrap();
        assert_eq!(current.footer, FooterFocus::Cancel);
        update(&mut model, key(KeyCode::Enter));
        assert!(model.schema_editor.preview.is_none());
        assert!(
            model.schema_editor.open,
            "Cancel goes back to the form, to change it"
        );

        // And Cancel takes a click.
        model.schema_editor.open = false;
        model.schema_editor.preview = Some(preview);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| crate::render::render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let (column, row) = model.hits.center(crate::mouse::HitTarget::FooterCancel);
        update(
            &mut model,
            Action::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }),
        );
        assert!(model.schema_editor.preview.is_none());
    }

    /// The form took Tab, Enter and Esc and nothing else: nothing typed reached a field,
    /// and it had no buttons to walk to.
    #[test]
    fn the_form_takes_typing_and_has_submit_and_cancel() {
        use crate::widgets::form::FooterFocus;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model {
            schema_editor: SchemaEditor::table_form("public.t"),
            ..Model::default()
        };
        model.schema_editor.open = true;
        update(&mut model, key(KeyCode::Backspace));
        update(&mut model, key(KeyCode::Char('2')));
        assert_eq!(model.schema_editor.field("target"), "public.2");
        let fields = model.schema_editor.fields.len();
        for _ in 0..fields {
            update(&mut model, key(KeyCode::Down));
        }
        assert_eq!(model.schema_editor.footer, FooterFocus::Submit);
        let screen = crate::render::render_to_string(&model, 100, 30);
        assert!(screen.contains(">[Preview]"), "{screen}");
        update(&mut model, key(KeyCode::Right));
        assert_eq!(model.schema_editor.footer, FooterFocus::Cancel);
        update(&mut model, key(KeyCode::Enter));
        assert!(!model.schema_editor.open, "Enter on Cancel cancels");

        model.schema_editor.open = true;
        model.schema_editor.footer = FooterFocus::Submit;
        update(&mut model, key(KeyCode::Enter));
        assert!(model.schema_editor.preview.is_some());
        assert!(!model.schema_editor.open, "the preview takes over");

        model.schema_editor.preview = None;
        model.schema_editor.open = true;
        update(&mut model, key(KeyCode::Esc));
        assert!(!model.schema_editor.open);
    }
}
