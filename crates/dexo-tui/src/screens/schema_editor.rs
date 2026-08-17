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

#[derive(Clone, Debug, PartialEq)]
pub struct FormField {
    pub label: String,
    pub value: String,
    pub secret: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DdlPreviewState {
    pub target: String,
    pub sql: String,
    pub risk: String,
    pub confirmation: Confirmation,
    pub typed: String,
    pub confirmed: bool,
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
            typed: String::new(),
            confirmed: false,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("target: {}", self.target),
            format!("risk: {}", self.risk),
            self.sql.clone(),
            if matches!(self.confirmation, Confirmation::TypeTarget(_)) && !self.confirmed {
                "type target to confirm".into()
            } else {
                "ready".into()
            },
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchemaEditor {
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub focus: usize,
    pub errors: Vec<String>,
    pub raw_sql: String,
    pub preview: Option<DdlPreviewState>,
    pub form_diff: Option<String>,
}

impl Default for SchemaEditor {
    fn default() -> Self {
        Self::table_form("public.orders")
    }
}

impl SchemaEditor {
    pub fn table_form(target: impl Into<String>) -> Self {
        Self {
            kind: FormKind::Table,
            fields: vec![
                FormField {
                    label: "target".into(),
                    value: target.into(),
                    secret: false,
                },
                FormField {
                    label: "columns".into(),
                    value: "id bigint identity pk".into(),
                    secret: false,
                },
                FormField {
                    label: "defaults".into(),
                    value: String::new(),
                    secret: false,
                },
                FormField {
                    label: "indexes".into(),
                    value: String::new(),
                    secret: false,
                },
                FormField {
                    label: "constraints".into(),
                    value: String::new(),
                    secret: false,
                },
                FormField {
                    label: "foreign_keys".into(),
                    value: String::new(),
                    secret: false,
                },
            ],
            focus: 0,
            errors: Vec::new(),
            raw_sql: String::new(),
            preview: None,
            form_diff: None,
        }
    }

    pub fn view_form(target: impl Into<String>) -> Self {
        let mut editor = Self::table_form(target);
        editor.kind = FormKind::View;
        editor.fields = vec![
            FormField {
                label: "target".into(),
                value: editor.field("target").to_string(),
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
                value: editor.field("target").to_string(),
                secret: false,
            },
            FormField {
                label: "arguments".into(),
                value: if trigger {
                    String::new()
                } else {
                    "n integer".into()
                },
                secret: false,
            },
            FormField {
                label: "body".into(),
                value: if trigger {
                    "EXECUTE FUNCTION public.tg_fn()".into()
                } else {
                    "SELECT n + 1".into()
                },
                secret: false,
            },
            FormField {
                label: "table".into(),
                value: if trigger {
                    "public.orders".into()
                } else {
                    String::new()
                },
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
            field.value = value.into();
        }
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
            preview.confirmed = preview.typed == *expected;
        } else {
            preview.confirmed = true;
        }
    }

    pub fn apply_raw(&mut self, sql: String) {
        let risk = classify_raw_sql(&sql);
        let previous = self
            .fields
            .iter()
            .map(|field| format!("{}={}", field.label, field.value))
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
}
