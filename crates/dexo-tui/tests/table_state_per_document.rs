use dexo_app::data::RowEditState;
use dexo_driver_api::{DataRequest, QualifiedName};
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Model};
use dexo_tui::{Effect, update};

fn orders() -> QualifiedName {
    dexo_app::parse_qualified("public.orders")
}

fn customers() -> QualifiedName {
    dexo_app::parse_qualified("public.customers")
}

/// Two open tables, `orders` active; each has loaded once, as opening them does.
fn two_tables() -> Model {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    model
        .documents
        .push(EditorDocument::new_table(orders(), None));
    model
        .documents
        .push(EditorDocument::new_table(customers(), None));
    model.set_active_document(1);
    model.set_active_document(2);
    model.set_active_document(1);
    model
}

fn requests(effects: &[Effect]) -> Vec<&DataRequest> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::LoadTableData { request, .. } => Some(request),
            _ => None,
        })
        .collect()
}

/// The paging state was one for the whole workbench, so after switching tabs Next
/// paged whichever table had loaded last into the table on screen.
#[test]
fn paging_after_a_tab_switch_pages_the_table_on_screen() {
    let mut model = two_tables();
    let limit = u64::from(model.data.page_limit);
    update(&mut model, Action::NextDataPage);
    update(&mut model, Action::NextDataPage);
    assert_eq!(model.data.page_offset, 2 * limit);

    model.set_active_document(2);
    let effects = update(&mut model, Action::NextDataPage);

    let sent = requests(&effects);
    assert_eq!(sent.len(), 1, "{effects:?}");
    assert_eq!(sent[0].object, customers());
    assert_eq!(sent[0].page.offset, limit, "customers took orders' page");

    model.set_active_document(1);
    assert_eq!(model.data.target, orders());
    assert_eq!(model.data.page_offset, 2 * limit, "orders lost its page");
}

#[test]
fn pending_edits_stay_with_their_table_across_a_tab_switch() {
    let mut model = two_tables();
    model.data.row_changes.insert(0, RowEditState::Inserted);

    model.set_active_document(2);
    assert!(
        !model.data.has_pending_edits(),
        "customers shows orders' edits"
    );

    model.set_active_document(1);
    assert!(model.data.has_pending_edits(), "orders lost its edits");
}

/// Row edits are keyed by row index; a page load swaps the rows under them.
#[test]
fn paging_refuses_while_edits_are_pending() {
    let mut model = two_tables();
    model.data.row_changes.insert(0, RowEditState::Inserted);
    let offset = model.data.page_offset;

    let effects = update(&mut model, Action::NextDataPage);

    assert!(requests(&effects).is_empty(), "{effects:?}");
    assert_eq!(model.data.page_offset, offset);
    assert!(model.data.has_pending_edits());
}

/// Back used to load the origin table into the current document, under its table.
#[test]
fn back_from_a_foreign_key_returns_to_the_document_it_came_from() {
    let mut model = two_tables();
    let origin = model.active_document().id.clone();
    model.data.related_fk = Some(dexo_app::data::ForeignKey {
        local: vec!["customer_id".into()],
        referenced_table: customers(),
        referenced: vec!["id".into()],
    });
    model.data.related_row = vec![("customer_id".into(), Some(dexo_driver_api::DbValue::I64(3)))];

    let effects = update(&mut model, Action::OpenRelated);
    assert_eq!(requests(&effects)[0].object, customers());
    assert_eq!(model.data.target, customers());

    let effects = update(&mut model, Action::DataNavBack);

    assert!(requests(&effects).is_empty(), "{effects:?}");
    assert_eq!(model.active_document().id, origin);
    assert_eq!(model.data.target, orders());
}
