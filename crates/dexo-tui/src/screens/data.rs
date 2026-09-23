use dexo_app::Environment;
use dexo_app::data::{
    ChangeSet, EditMode, ForeignKey, RowEditState, SqlDialect, TableMeta, ValueView, preview_sql,
};
use dexo_driver_api::{DbValue, QualifiedName};

use crate::widgets::form::{FooterFocus, footer_line};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertRowForm {
    pub open: bool,
    pub fields: Vec<crate::screens::schema_editor::FormField>,
    pub focus: usize,
}

impl InsertRowForm {
    pub fn open_for(&mut self, table: &TableMeta) {
        self.open = true;
        self.focus = 0;
        self.fields = table
            .columns
            .iter()
            .map(|column| crate::screens::schema_editor::FormField {
                label: column.name.clone(),
                value: String::new(),
                secret: false,
            })
            .collect();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.fields.clear();
        self.focus = 0;
    }

    /// Empty fields are omitted entirely rather than sent as `Null`, so a
    /// left-blank auto-increment/serial or defaulted column falls through to
    /// the database's own default instead of an explicit NULL overriding it.
    pub fn values(&self) -> Vec<(String, DbValue)> {
        self.fields
            .iter()
            .filter(|field| !field.value.is_empty())
            .map(|field| (field.label.clone(), DbValue::Text(field.value.clone())))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataQueryIntent {
    Sort,
    Filter,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataQueryPrompt {
    pub open: bool,
    pub intent: Option<DataQueryIntent>,
    pub column: String,
    pub value: String,
    pub descending: bool,
    pub error: Option<String>,
    pub focus_value: bool,
    pub footer: FooterFocus,
}

impl DataQueryPrompt {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = match self.intent {
            Some(DataQueryIntent::Sort) => vec![
                "sort column".into(),
                format!("column: {}", self.column),
                format!("descending: {}", self.descending),
            ],
            Some(DataQueryIntent::Filter) => vec![
                "filter column".into(),
                format!("column: {}", self.column),
                format!("value: {}", self.value),
            ],
            None => Vec::new(),
        };
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Pending,
    Applied,
    Reverted,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewModal {
    pub target: String,
    pub preview_sql: String,
    pub operations: usize,
    pub production: bool,
    pub confirmed: bool,
    pub status: ReviewStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataScreen {
    pub table: TableMeta,
    pub target: QualifiedName,
    pub changes: ChangeSet,
    pub review: Option<ReviewModal>,
    pub viewer: Option<ValueView>,
    pub clipboard: String,
    pub dialect: SqlDialect,
    pub environment: Environment,
    pub related_open: Vec<String>,
    pub related_fk: Option<ForeignKey>,
    pub related_row: Vec<(String, Option<DbValue>)>,
    /// The documents a foreign-key walk came from, most recent last. Each kept its own
    /// table state, so the way back is the document, not a copy of where it was.
    pub crumbs: Vec<String>,
    pub page_offset: u64,
    pub page_limit: u32,
    pub has_more: bool,
    pub loading: bool,
    pub filter: Option<dexo_driver_api::Filter>,
    pub sort: Vec<dexo_driver_api::Sort>,
    pub last_error: Option<String>,
    pub query_prompt: DataQueryPrompt,
    pub target_document: Option<String>,
    pub request_started: Option<std::time::Instant>,
    pub row_changes: std::collections::BTreeMap<usize, RowEditState>,
    pub insert_form: InsertRowForm,
}

impl Default for DataScreen {
    fn default() -> Self {
        let table = TableMeta {
            columns: Vec::new(),
        };
        Self {
            changes: ChangeSet::for_table(&table),
            table,
            target: QualifiedName::new(None::<String>, None::<String>, "tbl"),
            review: None,
            viewer: None,
            clipboard: String::new(),
            dialect: SqlDialect::Postgres,
            environment: Environment::Local,
            related_open: Vec::new(),
            related_fk: None,
            related_row: Vec::new(),
            crumbs: Vec::new(),
            page_offset: 0,
            page_limit: 100,
            has_more: false,
            loading: false,
            filter: None,
            sort: Vec::new(),
            last_error: None,
            query_prompt: DataQueryPrompt::default(),
            target_document: None,
            request_started: None,
            row_changes: std::collections::BTreeMap::new(),
            insert_form: InsertRowForm::default(),
        }
    }
}

impl DataScreen {
    /// Trades the state that belongs to one table -- what it is, where it is paged to,
    /// how it is filtered and sorted, the edits waiting on it -- with `parked`. The rest
    /// (modals, clipboard, the session's dialect) stays put: it is not the table's.
    pub fn swap_browse(&mut self, parked: &mut DataScreen) {
        use std::mem::swap;
        swap(&mut self.table, &mut parked.table);
        swap(&mut self.target, &mut parked.target);
        swap(&mut self.changes, &mut parked.changes);
        swap(&mut self.related_open, &mut parked.related_open);
        swap(&mut self.related_fk, &mut parked.related_fk);
        swap(&mut self.related_row, &mut parked.related_row);
        swap(&mut self.crumbs, &mut parked.crumbs);
        swap(&mut self.page_offset, &mut parked.page_offset);
        swap(&mut self.has_more, &mut parked.has_more);
        swap(&mut self.loading, &mut parked.loading);
        swap(&mut self.filter, &mut parked.filter);
        swap(&mut self.sort, &mut parked.sort);
        swap(&mut self.last_error, &mut parked.last_error);
        swap(&mut self.target_document, &mut parked.target_document);
        swap(&mut self.request_started, &mut parked.request_started);
        swap(&mut self.row_changes, &mut parked.row_changes);
    }

    pub fn has_pending_edits(&self) -> bool {
        !self.changes.pending().is_empty() || !self.row_changes.is_empty()
    }

    pub fn open_review(&mut self) {
        self.review = Some(ReviewModal {
            target: self.target.display_unquoted(),
            preview_sql: preview_sql(&self.target, &self.changes),
            operations: self.changes.pending().len(),
            production: self.environment == Environment::Production,
            confirmed: false,
            status: ReviewStatus::Pending,
            error: None,
        });
    }

    pub fn confirm_production(&mut self) {
        if let Some(review) = &mut self.review {
            review.confirmed = true;
        }
    }

    pub fn apply(&mut self) {
        let Some(review) = &mut self.review else {
            return;
        };
        if review.production && !review.confirmed {
            return;
        }
        review.status = ReviewStatus::Applied;
        self.changes.discard();
    }

    pub fn fail_apply(&mut self, message: String) {
        if let Some(review) = &mut self.review {
            review.status = ReviewStatus::Failed;
            review.error = Some(message);
        }
    }

    pub fn revert(&mut self) {
        self.changes.discard();
        if let Some(review) = &mut self.review {
            review.status = ReviewStatus::Reverted;
            review.operations = 0;
            review.preview_sql.clear();
        }
    }

    pub fn failed_still_editable(&self) -> bool {
        matches!(
            self.review.as_ref().map(|review| review.status),
            Some(ReviewStatus::Failed) | None
        ) && self.changes.mode() == EditMode::Editable
    }
}

pub fn review_lines(modal: &ReviewModal) -> Vec<String> {
    let mut lines = vec![
        format!("target: {}", modal.target),
        format!("ops: {}", modal.operations),
        format!("status: {:?}", modal.status),
    ];
    if let Some(error) = &modal.error {
        lines.push(format!("error: {error}"));
    }
    lines.push(if modal.production && !modal.confirmed {
        "confirm production to apply".into()
    } else {
        "ready".into()
    });
    lines.push(modal.preview_sql.clone());
    lines
}

#[cfg(test)]
mod tests {
    use super::ReviewStatus;
    use crate::action::Action;
    use crate::model::Model;
    use crate::update;
    use dexo_app::Environment;
    use dexo_app::data::{ColumnDef, ForeignKey, RowIdentity, TableMeta};
    use dexo_driver_api::{DbValue, QualifiedName};

    fn editable_table() -> TableMeta {
        TableMeta {
            columns: vec![ColumnDef {
                name: "id".into(),
                primary_key: true,
                unique: true,
                nullable: false,
            }],
        }
    }

    #[test]
    fn review_states_require_production_confirm() {
        let mut model = Model::default();
        model.data.table = editable_table();
        model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
        model.data.target = QualifiedName::new(Some("db"), Some("public"), "items");
        model.data.environment = Environment::Production;
        model
            .data
            .changes
            .insert(vec![("id".into(), DbValue::I64(1))]);
        update(&mut model, Action::OpenReview);
        update(&mut model, Action::ApplyChanges);
        assert_eq!(
            model.data.review.as_ref().unwrap().status,
            ReviewStatus::Pending
        );
        model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
        update(&mut model, Action::ConfirmProduction);
        let effects = update(&mut model, Action::ApplyChanges);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, crate::Effect::ApplyMutations { .. }))
        );
        assert_eq!(
            model.data.review.as_ref().unwrap().status,
            ReviewStatus::Pending
        );
        let generation = model.session_generation;
        update(
            &mut model,
            Action::MutationsApplied {
                generation,
                session: uuid::Uuid::from_u128(1).to_string(),
            },
        );
        assert_eq!(
            model.data.review.as_ref().unwrap().status,
            ReviewStatus::Applied
        );
        assert!(model.data.changes.pending().is_empty());
    }

    #[test]
    fn failed_changes_stay_editable() {
        let mut model = Model::default();
        model.data.table = editable_table();
        model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
        model.data.changes.update(
            RowIdentity {
                columns: vec!["id".into()],
                values: vec![DbValue::I64(1)],
            },
            vec![("id".into(), DbValue::I64(1))],
            vec![("id".into(), DbValue::I64(2))],
        );
        update(&mut model, Action::OpenReview);
        update(&mut model, Action::FailApply);
        assert_eq!(
            model.data.review.as_ref().unwrap().status,
            ReviewStatus::Failed
        );
        assert_eq!(model.data.changes.pending().len(), 1);
        assert!(model.data.failed_still_editable());
        update(&mut model, Action::RevertChanges);
        assert_eq!(
            model.data.review.as_ref().unwrap().status,
            ReviewStatus::Reverted
        );
        assert!(model.data.changes.pending().is_empty());
    }

    /// `data_nav_back` popped the crumb but never the pushed tab title, so walking
    /// foreign keys leaked a strip entry per hop. Documents are reused by target, so
    /// going back and forth has to stay flat.
    #[test]
    fn related_navigation_does_not_leak_per_hop() {
        let mut model = Model::default();
        model.data.related_fk = Some(ForeignKey {
            local: vec!["user_id".into()],
            referenced_table: QualifiedName::new(Some("db"), Some("public"), "users"),
            referenced: vec!["id".into()],
        });
        model.data.related_row = vec![("user_id".into(), Some(DbValue::I64(9)))];

        update(&mut model, Action::OpenRelated);
        let after_first = model.documents.len();
        update(&mut model, Action::DataNavBack);
        update(&mut model, Action::OpenRelated);
        update(&mut model, Action::DataNavBack);

        assert_eq!(
            model.documents.len(),
            after_first,
            "each hop left something behind"
        );
    }

    #[test]
    fn open_related_opens_a_document() {
        let mut model = Model::default();
        let before = model.documents.len();
        model.data.related_fk = Some(ForeignKey {
            local: vec!["user_id".into()],
            referenced_table: QualifiedName::new(Some("db"), Some("public"), "users"),
            referenced: vec!["id".into()],
        });
        model.data.related_row = vec![("user_id".into(), Some(DbValue::I64(9)))];
        update(&mut model, Action::OpenRelated);
        assert_eq!(model.documents.len(), before + 1);
        assert!(model.active_document().kind.is_table());
        assert_eq!(model.data.related_open, vec!["db.public.users"]);
    }
}
