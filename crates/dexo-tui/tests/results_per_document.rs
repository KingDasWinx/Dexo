//! The output pane belongs to the active document. A query run in one file used to
//! redraw every other file's grid, because `Model::results` was a single pane shared by
//! all of them.
use dexo_driver_api::{
    CatalogList, CatalogObject, ColumnMeta, DbValue, ObjectId, ObjectKind, QualifiedName,
};
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Model};
use dexo_tui::runtime::{OperationId, OperationKey, SessionId};
use dexo_tui::update::update;

const SESSION: u128 = 1;

fn model_with_session() -> Model {
    Model {
        session_generation: 1,
        active_session: Some(SessionId(uuid::Uuid::from_u128(SESSION))),
        ..Model::default()
    }
}

/// Feeds one single-column result set to whichever document is active.
fn deliver_results(model: &mut Model, label: &str) {
    let key = OperationKey::new(
        OperationId::new(),
        SessionId(uuid::Uuid::from_u128(SESSION)).0.to_string(),
        model.active_document().id.clone(),
        1,
    );
    update(
        model,
        Action::QueryResultSetStarted {
            key: key.clone(),
            index: 0,
        },
    );
    update(
        model,
        Action::QueryMeta {
            key: key.clone(),
            index: 0,
            columns: vec![ColumnMeta {
                name: label.into(),
                type_name: "text".into(),
                nullable: true,
            }],
        },
    );
    update(
        model,
        Action::QueryRows {
            key,
            index: 0,
            rows: vec![vec![DbValue::Text(label.into())]],
        },
    );
}

fn on_screen(model: &Model) -> Vec<String> {
    model
        .results
        .tabs
        .get(model.results.active)
        .map(|tab| tab.grid.columns().iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn each_document_keeps_its_own_results() {
    let mut model = model_with_session();
    deliver_results(&mut model, "from_scratch");

    model
        .documents
        .push(EditorDocument::new_unique("other.sql", None, None));
    update(&mut model, Action::SelectDocument { index: 1 });
    assert!(
        on_screen(&model).is_empty(),
        "a fresh document starts with an empty pane"
    );
    deliver_results(&mut model, "from_other_file");

    update(&mut model, Action::SelectDocument { index: 0 });
    assert_eq!(on_screen(&model), ["from_scratch"]);
    update(&mut model, Action::SelectDocument { index: 1 });
    assert_eq!(on_screen(&model), ["from_other_file"]);
}

/// The reported path: open a table from the sidebar, run a select in another file, come
/// back to the table tab.
#[test]
fn a_select_elsewhere_leaves_the_table_tab_alone() {
    let mut model = model_with_session();
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("table:orders"),
            ObjectKind::Table,
            QualifiedName::new(None::<String>, Some("public"), "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new("table:orders"));
    update(&mut model, Action::OpenObjectData);
    let table_tab = model.active_document;
    assert!(model.documents[table_tab].kind.is_table());
    deliver_results(&mut model, "orders_rows");

    update(&mut model, Action::SelectDocument { index: 0 });
    deliver_results(&mut model, "ad_hoc_select");
    assert_eq!(on_screen(&model), ["ad_hoc_select"]);

    update(&mut model, Action::SelectDocument { index: table_tab });
    assert_eq!(
        on_screen(&model),
        ["orders_rows"],
        "the table tab showed the other file's select"
    );
}
