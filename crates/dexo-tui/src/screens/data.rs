use dexo_app::Environment;
use dexo_app::data::{
    ChangeSet, EditMode, ForeignKey, SqlDialect, TableMeta, ValueView, preview_sql,
};
use dexo_driver_api::{DbValue, QualifiedName};

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
    pub page_offset: u64,
    pub page_limit: u32,
    pub has_more: bool,
    pub loading: bool,
    pub filter: Option<dexo_driver_api::Filter>,
    pub sort: Vec<dexo_driver_api::Sort>,
    pub last_error: Option<String>,
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
            page_offset: 0,
            page_limit: 100,
            has_more: false,
            loading: false,
            filter: None,
            sort: Vec::new(),
            last_error: None,
        }
    }
}

impl DataScreen {
    pub fn open_review(&mut self) {
        self.review = Some(ReviewModal {
            target: self.target.display_unquoted(),
            preview_sql: preview_sql(&self.target, &self.changes),
            operations: self.changes.pending().len(),
            production: self.environment == Environment::Production,
            confirmed: false,
            status: ReviewStatus::Pending,
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

    pub fn fail_apply(&mut self) {
        if let Some(review) = &mut self.review {
            review.status = ReviewStatus::Failed;
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
    vec![
        format!("target: {}", modal.target),
        format!("ops: {}", modal.operations),
        format!("status: {:?}", modal.status),
        if modal.production && !modal.confirmed {
            "confirm production to apply".into()
        } else {
            "ready".into()
        },
        modal.preview_sql.clone(),
    ]
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
        update(&mut model, Action::ConfirmProduction);
        update(&mut model, Action::ApplyChanges);
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

    #[test]
    fn open_related_adds_tab() {
        let mut model = Model::default();
        let before = model.tabs.titles.len();
        model.data.related_fk = Some(ForeignKey {
            local: vec!["user_id".into()],
            referenced_table: QualifiedName::new(Some("db"), Some("public"), "users"),
            referenced: vec!["id".into()],
        });
        model.data.related_row = vec![("user_id".into(), Some(DbValue::I64(9)))];
        update(&mut model, Action::OpenRelated);
        assert_eq!(model.tabs.titles.len(), before + 1);
        assert_eq!(model.data.related_open, vec!["db.public.users"]);
    }
}
