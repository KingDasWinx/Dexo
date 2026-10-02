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
    /// What each field's column holds, by field: the type and whether a value is needed.
    pub columns: Vec<InsertColumn>,
    /// Why the last Insert was refused; the form stays open for it to be fixed.
    pub error: Option<String>,
}

/// A column as the insert form shows it beside its name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertColumn {
    pub type_name: String,
    /// NOT NULL: a value or a default is needed.
    pub required: bool,
}

impl InsertRowForm {
    pub fn open_for(&mut self, table: &TableMeta, columns: &[dexo_driver_api::ColumnMeta]) {
        self.open = true;
        self.focus = 0;
        self.error = None;
        self.fields = table
            .columns
            .iter()
            .map(|column| crate::screens::schema_editor::FormField {
                label: column.name.clone(),
                value: crate::widgets::text_input::TextInput::default(),
                secret: false,
            })
            .collect();
        self.columns = table
            .columns
            .iter()
            .map(|column| {
                columns
                    .iter()
                    .find(|meta| meta.name == column.name)
                    .map(|meta| InsertColumn {
                        type_name: meta.type_name.clone(),
                        required: !meta.nullable,
                    })
                    .unwrap_or_default()
            })
            .collect();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.fields.clear();
        self.columns.clear();
        self.error = None;
        self.focus = 0;
    }

    /// The start of field `index`'s line, up to where its value begins: the marker, the
    /// column, its type, and a `*` where a value is needed.
    pub fn prefix(&self, index: usize) -> String {
        let marker = if index == self.focus { ">" } else { " " };
        let label = self
            .fields
            .get(index)
            .map_or("", |field| field.label.as_str());
        match self.columns.get(index) {
            Some(column) if !column.type_name.is_empty() => format!(
                "{marker} {label} ({}){}: ",
                column.type_name,
                if column.required { " *" } else { "" }
            ),
            _ => format!("{marker} {label}: "),
        }
    }

    /// Refuses a value the column's type cannot take, naming the column: the server would
    /// refuse it later, at Apply, without saying which field it was.
    pub fn check(&self) -> Result<(), String> {
        for (field, column) in self.fields.iter().zip(&self.columns) {
            let text = field.value.trim();
            if text.is_empty() {
                continue;
            }
            let kind = column.type_name.to_ascii_lowercase();
            let refuse = |what: &str| Err(format!("{}: `{text}` is not {what}.", field.label));
            if kind.contains("bool") {
                let words = [
                    "true", "false", "t", "f", "1", "0", "yes", "no", "y", "n", "on", "off",
                ];
                if !words.contains(&text.to_ascii_lowercase().as_str()) {
                    return refuse("true or false");
                }
            } else if kind.contains("int") || kind.contains("serial") {
                if text.parse::<i128>().is_err() {
                    return refuse("a whole number");
                }
            } else if ["numeric", "decimal", "float", "double", "real", "money"]
                .iter()
                .any(|name| kind.contains(name))
                && text.parse::<f64>().is_err()
            {
                return refuse("a number");
            }
        }
        Ok(())
    }

    /// The fields, then Insert, then Cancel: the ring Tab and the arrows walk, the same
    /// one the connection form uses.
    fn slots(&self) -> usize {
        self.fields.len() + 2
    }

    pub fn focus_next(&mut self) {
        self.focus = (self.focus + 1) % self.slots();
    }

    pub fn focus_prev(&mut self) {
        self.focus = (self.focus + self.slots() - 1) % self.slots();
    }

    /// Left and Right step between the two buttons once one of them has the focus.
    pub fn toggle_button(&mut self) {
        self.focus = match self.footer_focus() {
            FooterFocus::Submit => self.fields.len() + 1,
            FooterFocus::Cancel => self.fields.len(),
            FooterFocus::Input => self.focus,
        };
    }

    pub fn footer_focus(&self) -> FooterFocus {
        match self.focus.checked_sub(self.fields.len()) {
            None => FooterFocus::Input,
            Some(0) => FooterFocus::Submit,
            Some(_) => FooterFocus::Cancel,
        }
    }

    pub fn focused_field_mut(&mut self) -> Option<&mut crate::screens::schema_editor::FormField> {
        self.fields.get_mut(self.focus)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| format!("{}{}", self.prefix(index), field.value.as_str()))
            .collect();
        lines.push(String::new());
        lines.push("A field left empty is not sent: the column's default applies.".into());
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Insert", self.footer_focus()));
        lines
    }

    /// Empty fields are omitted entirely rather than sent as `Null`, so a
    /// left-blank auto-increment/serial or defaulted column falls through to
    /// the database's own default instead of an explicit NULL overriding it.
    pub fn values(&self) -> Vec<(String, DbValue)> {
        self.fields
            .iter()
            .filter(|field| !field.value.is_empty())
            .map(|field| {
                (
                    field.label.clone(),
                    DbValue::Text(field.value.as_str().to_string()),
                )
            })
            .collect()
    }
}

/// What has the keys in the edit-cell dialog: the value, then the four buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellFocus {
    Value,
    Null,
    Editor,
    Save,
    Cancel,
}

impl CellFocus {
    const RING: [Self; 5] = [
        Self::Value,
        Self::Null,
        Self::Editor,
        Self::Save,
        Self::Cancel,
    ];

    fn at(self) -> usize {
        Self::RING
            .iter()
            .position(|slot| *slot == self)
            .unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::RING[(self.at() + 1) % Self::RING.len()]
    }

    pub fn prev(self) -> Self {
        Self::RING[(self.at() + Self::RING.len() - 1) % Self::RING.len()]
    }

    /// Left and Right step among the buttons once one has the focus.
    pub fn step_button(self, forward: bool) -> Self {
        let buttons = &Self::RING[1..];
        let at = buttons.iter().position(|slot| *slot == self).unwrap_or(0);
        let next = if forward {
            (at + 1) % buttons.len()
        } else {
            (at + buttons.len() - 1) % buttons.len()
        };
        buttons[next]
    }
}

/// F2 on a cell: the column and its type, the value to change in one line, and the
/// buttons -- Save, Cancel, and the two ways to set a value that typing does not: NULL,
/// and `$EDITOR` for a long one.
#[derive(Clone, Debug, PartialEq)]
pub struct CellEditForm {
    pub row: usize,
    pub column: usize,
    pub table: String,
    pub name: String,
    pub type_name: String,
    pub value: crate::widgets::text_input::TextInput,
    /// The value was NULL when the dialog opened.
    pub was_null: bool,
    pub focus: CellFocus,
}

impl CellEditForm {
    /// The value's text on one line, a line break as `↵`; the text itself is what is kept.
    pub fn lines(&self) -> Vec<String> {
        let mark = |button: &str, at: CellFocus| {
            format!("{}[{button}]", if self.focus == at { ">" } else { " " })
        };
        let shown = self.value.as_str().replace(['\n', '\r'], "↵");
        let mut lines = vec![
            format!("{} · {} ({})", self.table, self.name, self.type_name),
            if self.was_null && self.value.is_empty() {
                "> value: (NULL)".to_string()
            } else {
                format!("> value: {shown}")
            },
            String::new(),
            format!(
                "{}  {}  {}  {}",
                mark("Save", CellFocus::Save),
                mark("Cancel", CellFocus::Cancel),
                mark("NULL", CellFocus::Null),
                mark("Editor", CellFocus::Editor),
            ),
            "Ctrl+N sets NULL  Ctrl+E opens $EDITOR  Esc cancels".to_string(),
        ];
        // Typed in a field that is drawn with its marker on the line the cursor is on.
        if self.focus != CellFocus::Value {
            lines[1] = lines[1].replacen("> ", "  ", 1);
        }
        lines
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClauseBar {
    Where,
    Order,
}

/// `f` on a row: the foreign keys from and to its table, to open the rows on the
/// other end.
#[derive(Clone, Debug, PartialEq)]
pub struct RelatedPicker {
    pub table: QualifiedName,
    /// `None` while the keys are asked for.
    pub links: Option<Vec<RelatedLink>>,
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelatedLink {
    /// `→ customers (customer_id)`, `← order_items (order_id)`.
    pub label: String,
    /// Read from the row's side: `local` are this table's columns.
    pub key: ForeignKey,
}

/// The exact row count `t` asked for, and the statement it counts: shown only while the
/// grid still pages through that statement.
#[derive(Clone, Debug, PartialEq)]
pub struct RowCount {
    /// What it counts. The grid shows the count only while it shows these rows.
    pub key: CountKey,
    pub state: CountState,
}

/// What a count counts: the rows' source -- a table, or the statement a result came
/// from -- and what narrows them, values included. Compared rather than rendered: the
/// count's SQL, rendered on every frame, cost ~11 ms on a long statement.
#[derive(Clone, Debug, PartialEq)]
pub struct CountKey {
    pub source: String,
    pub filter: Option<dexo_driver_api::Filter>,
    pub clauses: dexo_driver_api::RawClauses,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CountState {
    Running(crate::runtime::OperationId),
    Exact(u64),
}

/// The WHERE and ORDER BY bars over the grid: what is typed, what was last sent, and
/// what last came back with rows -- which a failed clause falls back to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClauseBars {
    pub where_input: crate::widgets::text_input::TextInput,
    pub order_input: crate::widgets::text_input::TextInput,
    pub focus: Option<ClauseBar>,
    pub applied: dexo_driver_api::RawClauses,
    pub good: dexo_driver_api::RawClauses,
    /// The text typed was sent and refused: the grid shows what ran before it, and the bars
    /// are drawn as failed until they are edited, applied or put back.
    pub failed: bool,
}

impl ClauseBars {
    /// What the bars hold now, as clauses: blank text is no clause.
    pub fn typed(&self) -> dexo_driver_api::RawClauses {
        let text = |input: &crate::widgets::text_input::TextInput| {
            Some(input.trim().to_string()).filter(|text| !text.is_empty())
        };
        dexo_driver_api::RawClauses {
            where_sql: text(&self.where_input),
            order_by: text(&self.order_input),
        }
    }

    pub fn input_mut(&mut self, bar: ClauseBar) -> &mut crate::widgets::text_input::TextInput {
        match bar {
            ClauseBar::Where => &mut self.where_input,
            ClauseBar::Order => &mut self.order_input,
        }
    }

    /// Esc: the bars read what last ran again, and let go of the keys.
    pub fn revert(&mut self) {
        self.where_input
            .set_text(self.applied.where_sql.clone().unwrap_or_default());
        self.order_input
            .set_text(self.applied.order_by.clone().unwrap_or_default());
        self.focus = None;
        self.failed = false;
    }

    /// Whether `bar`'s text is one the server refused: it differs from what last ran.
    pub fn refused(&self, bar: ClauseBar) -> bool {
        let (typed, ran) = match bar {
            ClauseBar::Where => (self.where_input.trim(), &self.applied.where_sql),
            ClauseBar::Order => (self.order_input.trim(), &self.applied.order_by),
        };
        self.failed && !typed.is_empty() && Some(typed) != ran.as_deref()
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
    pub status: ReviewStatus,
    pub error: Option<String>,
    /// Which button has the keys: Apply, or Cancel, which closes the review and leaves the
    /// changes pending.
    pub footer: FooterFocus,
    /// Lines of the statements scrolled off the top.
    pub scroll: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataScreen {
    pub table: TableMeta,
    pub target: QualifiedName,
    pub changes: ChangeSet,
    pub review: Option<ReviewModal>,
    pub viewer: Option<ValueView>,
    /// Lines the value modal is scrolled down.
    pub viewer_scroll: u16,
    pub clipboard: String,
    /// What the grid's last copy took, for the toast once the clipboard has it.
    pub copy_note: Option<String>,
    pub dialect: SqlDialect,
    pub environment: Environment,
    pub related_picker: Option<RelatedPicker>,
    pub related_row: Vec<(String, Option<DbValue>)>,
    pub page_offset: u64,
    pub page_limit: u32,
    pub has_more: bool,
    /// The table's row count as the server's statistics have it, when the page asked.
    pub estimated_total: Option<u64>,
    pub count: Option<RowCount>,
    pub loading: bool,
    pub filter: Option<dexo_driver_api::Filter>,
    pub last_error: Option<String>,
    /// The WHERE and ORDER BY bars over the grid.
    pub bars: ClauseBars,
    pub target_document: Option<String>,
    /// The page and the columns last asked for: an answer to any other request -- an
    /// older page, another document's -- is not this grid's.
    pub page_ticket: Option<crate::runtime::OperationId>,
    pub columns_ticket: Option<crate::runtime::OperationId>,
    pub request_started: Option<std::time::Instant>,
    pub row_changes: std::collections::BTreeMap<usize, RowEditState>,
    pub insert_form: InsertRowForm,
    /// The edit-cell dialog, while it is open.
    pub cell_edit: Option<CellEditForm>,
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
            viewer_scroll: 0,
            clipboard: String::new(),
            copy_note: None,
            dialect: SqlDialect::Postgres,
            environment: Environment::Local,
            related_picker: None,
            related_row: Vec::new(),
            page_offset: 0,
            page_limit: 100,
            has_more: false,
            estimated_total: None,
            count: None,
            loading: false,
            filter: None,
            last_error: None,
            bars: ClauseBars::default(),
            target_document: None,
            page_ticket: None,
            columns_ticket: None,
            request_started: None,
            row_changes: std::collections::BTreeMap::new(),
            insert_form: InsertRowForm::default(),
            cell_edit: None,
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
        swap(&mut self.related_row, &mut parked.related_row);
        swap(&mut self.page_offset, &mut parked.page_offset);
        swap(&mut self.has_more, &mut parked.has_more);
        swap(&mut self.estimated_total, &mut parked.estimated_total);
        swap(&mut self.count, &mut parked.count);
        swap(&mut self.loading, &mut parked.loading);
        swap(&mut self.filter, &mut parked.filter);
        swap(&mut self.last_error, &mut parked.last_error);
        swap(&mut self.target_document, &mut parked.target_document);
        swap(&mut self.page_ticket, &mut parked.page_ticket);
        swap(&mut self.columns_ticket, &mut parked.columns_ticket);
        swap(&mut self.request_started, &mut parked.request_started);
        swap(&mut self.row_changes, &mut parked.row_changes);
        swap(&mut self.bars, &mut parked.bars);
    }

    pub fn has_pending_edits(&self) -> bool {
        !self.changes.pending().is_empty() || !self.row_changes.is_empty()
    }

    pub fn open_review(&mut self) {
        self.review = Some(ReviewModal {
            target: self.target.display_unquoted(),
            preview_sql: preview_sql(&self.target, &self.changes, self.dialect),
            operations: self.changes.pending().len(),
            production: self.environment == Environment::Production,
            status: ReviewStatus::Pending,
            error: None,
            footer: FooterFocus::Submit,
            scroll: 0,
        });
    }

    pub fn apply(&mut self) {
        if let Some(review) = &mut self.review {
            review.status = ReviewStatus::Applied;
        }
        // Applied from the palette, with no review open, the changes are done all the same.
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

/// What the review shows at `width` columns: the lines that stay on top, and the
/// statements below them, each wrapped under itself, which scroll.
pub struct ReviewView {
    pub header: Vec<String>,
    pub body: Vec<String>,
}

pub fn review_view(modal: &ReviewModal, width: usize) -> ReviewView {
    let width = width.max(8);
    let mut header = vec![format!(
        "{} to {}",
        match modal.operations {
            1 => "1 change".to_string(),
            count => format!("{count} changes"),
        },
        modal.target
    )];
    if modal.production {
        header.push("On production, Apply asks for the connection's name.".into());
    }
    if let Some(error) = &modal.error {
        header.extend(crate::model::wrap_display_text(
            &format!("Not applied: {error}"),
            width,
        ));
    }
    let mut body = Vec::new();
    for statement in modal.preview_sql.lines() {
        // A statement continues under itself, indented, rather than off the border.
        for (index, line) in crate::model::wrap_display_text(statement, width - 2)
            .into_iter()
            .enumerate()
        {
            body.push(if index == 0 {
                line
            } else {
                format!("  {line}")
            });
        }
    }
    ReviewView { header, body }
}

#[cfg(test)]
mod tests {
    use super::{RelatedLink, RelatedPicker, ReviewStatus};
    use crate::action::Action;
    use crate::model::Model;
    use crate::update;
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

    /// The palette's Apply Changes arrives with no review open; on production it opens
    /// the review instead of applying.
    #[test]
    fn palette_apply_on_production_opens_the_review_first() {
        let mut model = Model::default();
        model.data.table = editable_table();
        model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
        model.data.target = QualifiedName::new(Some("db"), Some("public"), "items");
        model.connection.environment = "production".into();
        model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
        model
            .data
            .changes
            .insert(vec![("id".into(), DbValue::I64(1))]);
        let effects = update(&mut model, Action::ApplyChanges);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, crate::Effect::ApplyMutations { .. })),
            "applied on production without a confirmed review"
        );
        assert!(
            model
                .data
                .review
                .as_ref()
                .is_some_and(|review| review.production)
        );
    }

    /// On production the review applies only once the connection's name is typed: a
    /// click on its production line, or a hidden `y`, used to be enough.
    #[test]
    fn review_states_require_production_confirm() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let applies = |effects: &[crate::Effect]| {
            effects
                .iter()
                .any(|effect| matches!(effect, crate::Effect::ApplyMutations { .. }))
        };
        let mut model = Model::default();
        model.data.table = editable_table();
        model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
        model.data.target = QualifiedName::new(Some("db"), Some("public"), "items");
        model.connection.name = "shop-prod".into();
        model.connection.environment = "production".into();
        model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
        model
            .data
            .changes
            .insert(vec![("id".into(), DbValue::I64(1))]);
        update(&mut model, Action::OpenReview);
        assert!(model.data.review.as_ref().unwrap().production);
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(!applies(&update(&mut model, key(KeyCode::Char('y')))));
        assert!(!applies(&update(&mut model, key(KeyCode::Enter))));
        assert!(model.production_prompt.is_some());
        for ch in "shop".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        assert!(!applies(&update(&mut model, key(KeyCode::Enter))));
        assert!(model.production_prompt.as_ref().unwrap().error.is_some());
        for ch in "-prod".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        assert!(applies(&update(&mut model, key(KeyCode::Enter))));
        assert!(model.production_prompt.is_none());
        assert!(!model.production_cleared);
        let generation = model.session_generation;
        update(
            &mut model,
            Action::MutationsApplied {
                generation,
                session: uuid::Uuid::from_u128(1).to_string(),
            },
        );
        // Applied: the review has nothing left to show and closes, saying what was done.
        assert!(model.data.review.is_none());
        assert!(model.data.changes.pending().is_empty());
        assert_eq!(
            model.messages.last().map(|entry| entry.message.as_str()),
            Some("Applied 1 change.")
        );
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

    /// Follows `fk` from the row in `related_row` as Enter in the picker does.
    pub(crate) fn follow(model: &mut Model, fk: ForeignKey) -> Vec<crate::Effect> {
        model.data.related_picker = Some(RelatedPicker {
            table: model.data.target.clone(),
            links: Some(vec![RelatedLink {
                label: "→ users (user_id)".into(),
                key: fk,
            }]),
            selected: 0,
        });
        update(
            model,
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
        )
    }

    fn users_key() -> ForeignKey {
        ForeignKey {
            local: vec!["user_id".into()],
            referenced_table: QualifiedName::new(Some("db"), Some("public"), "users"),
            referenced: vec!["id".into()],
        }
    }

    /// Each hop is a document of its own, and Back closes it: walking foreign keys back
    /// and forth leaves the strip as it was.
    #[test]
    fn related_navigation_does_not_leak_per_hop() {
        let mut model = Model::default();
        let before = model.documents.len();
        for _ in 0..2 {
            model.data.related_row = vec![("user_id".into(), Some(DbValue::I64(9)))];
            follow(&mut model, users_key());
            assert_eq!(model.documents.len(), before + 1);
            update(&mut model, Action::DataNavBack);
        }
        assert_eq!(
            model.documents.len(),
            before,
            "each hop left something behind"
        );
    }

    #[test]
    fn open_related_opens_a_document() {
        let mut model = Model::default();
        let origin = model.active_document().id.clone();
        model.data.related_row = vec![("user_id".into(), Some(DbValue::I64(9)))];
        follow(&mut model, users_key());
        assert!(model.active_document().kind.is_table());
        assert_eq!(model.active_document().related_from, Some(origin));
        assert!(model.data.filter.is_some());
    }
}
