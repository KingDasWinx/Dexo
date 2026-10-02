use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;
use dexo_app::schema::{Confirmation, DdlPreview};
use dexo_driver_api::{
    ColumnSpec, IdentitySpec, IndexDef, QualifiedName, RoutineDef, RoutineKind, SchemaChange,
    TableDef, TableShape, ViewDef, classify_raw_sql,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DdlPreviewState {
    pub target: String,
    pub sql: String,
    pub risk: String,
    pub confirmation: Confirmation,
    /// The name typed to confirm a destructive change.
    pub typed: TextInput,
    pub confirmed: bool,
    /// The typed name, or one of the two buttons under it.
    pub footer: FooterFocus,
    /// Why Apply did nothing.
    pub error: Option<String>,
}

impl DdlPreviewState {
    pub fn from_preview(target: String, preview: &DdlPreview) -> Self {
        let sql = preview
            .plan
            .statements
            .iter()
            .map(|statement| statement.sql.as_str())
            .collect::<Vec<_>>()
            .join(";\n");
        Self {
            target,
            sql,
            risk: format!(
                "destructive={} lock={:?}",
                preview.risk.destructive, preview.risk.lock_level
            ),
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
        }
    }

    pub fn needs_typing(&self) -> bool {
        matches!(self.confirmation, Confirmation::TypeTarget(_))
    }

    /// The preview in at most `rows` lines. The SQL gives way first: the name to type
    /// and the buttons always show.
    pub fn lines(&self, rows: usize) -> Vec<String> {
        let mut tail = vec![String::new()];
        if let Confirmation::TypeTarget(expected) = &self.confirmation {
            tail.push(format!("Type {expected} to apply this."));
            tail.push(
                self.typed
                    .inline_line("name: ", self.footer == FooterFocus::Input),
            );
        }
        if let Some(error) = &self.error {
            tail.push(error.clone());
        }
        tail.push(footer_line("Apply", self.footer));
        let mut lines = vec![
            format!("target: {}", self.target),
            format!("risk: {}", self.risk),
        ];
        lines.extend(self.sql.lines().map(str::to_string));
        let room = rows.saturating_sub(tail.len());
        if lines.len() > room {
            lines.truncate(room.saturating_sub(1));
            if room > 0 {
                lines.push("…".into());
            }
        }
        lines.extend(tail);
        lines
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
}

impl Default for SchemaEditor {
    fn default() -> Self {
        Self::table_form("public.orders")
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
                    label: "defaults".into(),
                    value: TextInput::default(),
                    secret: false,
                },
                FormField {
                    label: "indexes".into(),
                    value: TextInput::default(),
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
        if self.kind == FormKind::Table && self.field("columns").trim().is_empty() {
            self.errors.push("columns are required".into());
        }
        if self.kind == FormKind::View && self.field("sql").trim().is_empty() {
            self.errors.push("view sql is required".into());
        }
        self.errors.is_empty()
    }

    pub fn to_change(&self) -> Result<SchemaChange, Vec<String>> {
        let target = parse_target(self.field("target"));
        match self.kind {
            FormKind::Table => Ok(SchemaChange::CreateTable {
                target,
                def: TableDef {
                    shape: TableShape::Table,
                    columns: parse_columns(self.field("columns")),
                    constraints: vec![],
                    partition: None,
                    engine: None,
                    charset: None,
                    collation: None,
                },
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

    pub fn open_preview(&mut self, preview: DdlPreview) {
        let target = self.field("target").to_string();
        self.preview = Some(DdlPreviewState::from_preview(target, &preview));
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

    pub fn apply_raw(&mut self, sql: String) {
        let risk = classify_raw_sql(&sql);
        let previous = self
            .fields
            .iter()
            .map(|field| format!("{}={}", field.label, field.value.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        self.form_diff = Some(format!(
            "- {previous}\n+ {sql}\nrisk destructive={}",
            risk.destructive
        ));
        self.raw_sql = sql;
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("schema {}", kind_label(self.kind))];
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
            // One mark per character typed, so a slip of the finger shows; the characters
            // themselves never reach the screen.
            let value = if field.secret {
                "*".repeat(field.value.len())
            } else {
                field.value.as_str().to_string()
            };
            lines.push(format!("{marker} {}: {value}", field.label));
        }
        for error in &self.errors {
            lines.push(format!("error: {error}"));
        }
        if let Some(diff) = &self.form_diff {
            lines.push(diff.clone());
        }
        if !self.raw_sql.is_empty() {
            lines.push(format!("raw: {}", self.raw_sql));
        }
        lines
    }
}

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

fn parse_columns(spec: &str) -> Vec<ColumnSpec> {
    spec.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut bits = part.split_whitespace();
            let name = bits.next().unwrap_or("col");
            let data_type = bits.next().unwrap_or("text");
            let rest: Vec<_> = bits.collect();
            ColumnSpec {
                name: QualifiedName::new(None::<String>, None::<String>, name),
                data_type: data_type.into(),
                nullable: !rest.iter().any(|bit| bit.eq_ignore_ascii_case("pk")),
                default_sql: None,
                identity: rest
                    .iter()
                    .any(|bit| bit.eq_ignore_ascii_case("identity"))
                    .then_some(IdentitySpec { always: true }),
                auto_increment: rest.iter().any(|bit| bit.eq_ignore_ascii_case("autoinc")),
                generated: None,
                primary_key: rest.iter().any(|bit| bit.eq_ignore_ascii_case("pk")),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FormKind, SchemaEditor};
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

    #[test]
    fn raw_ddl_shows_diff_before_replacing_form() {
        let mut editor = SchemaEditor::table_form("public.t");
        editor.apply_raw("CREATE INDEX t_idx ON t (id)".into());
        assert!(editor.form_diff.as_ref().unwrap().contains("CREATE INDEX"));
        assert!(editor.raw_sql.contains("CREATE INDEX"));
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
            risk: "destructive=true lock=None".into(),
            confirmation: Confirmation::TypeTarget("public.orders".into()),
            typed: Default::default(),
            confirmed: false,
            footer: FooterFocus::Input,
            error: None,
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

        // And Cancel takes a click.
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
