//! Completion only knew the columns of tables whose node had been expanded in the
//! sidebar, so `where |` on a table nobody had opened offered nothing of it.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_tui::action::Action;
use dexo_tui::runtime::SessionId;
use dexo_tui::{Effect, Focus, Model, update};
use uuid::Uuid;

fn type_text(model: &mut Model, text: &str) -> Vec<Effect> {
    let mut effects = Vec::new();
    for ch in text.chars() {
        effects = update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
        );
    }
    effects
}

fn connected() -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        active_session: Some(SessionId(Uuid::from_u128(1))),
        session_generation: 3,
        ..Model::default()
    };
    model.connection.name = "local".into();
    model.connection.ready = true;
    model
}

fn table(name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(format!("table:{name}")),
        ObjectKind::Table,
        QualifiedName::new(None::<String>, Some("public"), name),
        None,
    )
}

fn column(table: &str, name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(format!("column:{table}.{name}")),
        ObjectKind::Column,
        QualifiedName::new(None::<String>, Some("public"), format!("{table}.{name}")),
        Some(ObjectId::new(format!("table:{table}"))),
    )
}

fn offered(model: &Model) -> Vec<&str> {
    model
        .editor
        .completions
        .iter()
        .map(|item| item.label.as_str())
        .collect()
}

#[test]
fn the_captured_snapshot_answers_columns_nobody_expanded() {
    let mut model = connected();
    update(
        &mut model,
        Action::CompletionCatalogLoaded {
            generation: 3,
            objects: vec![table("orders"), column("orders", "total")],
            complete: false,
        },
    );
    type_text(&mut model, "select * from orders where ");
    assert!(model.editor.completion_open);
    assert!(offered(&model).contains(&"total"), "{:?}", offered(&model));
}

#[test]
fn a_table_nothing_knows_is_asked_of_the_session_and_the_popup_opens_on_the_answer() {
    let mut model = connected();
    let effects = type_text(&mut model, "select * from orders where ");
    let target = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCompletionColumns {
                target, generation, ..
            } => Some((target.clone(), *generation)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no column lookup: {effects:?}"));
    assert_eq!(target.0.object(), "orders");
    assert_eq!(target.1, 3);

    update(
        &mut model,
        Action::CompletionColumnsLoaded {
            generation: 3,
            target: target.0,
            columns: vec!["id".into(), "total".into()],
        },
    );
    assert!(model.editor.completion_open, "the answer did not open it");
    assert!(offered(&model).contains(&"total"), "{:?}", offered(&model));

    // Asked once: the next key finds the columns in memory.
    let effects = type_text(&mut model, "t");
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadCompletionColumns { .. })),
        "{effects:?}"
    );
}

/// `select venda.` is typed before the FROM exists: the qualifier alone names the
/// table whose columns are wanted.
#[test]
fn a_table_named_only_before_the_dot_is_asked_about_too() {
    let mut model = connected();
    model.absorb_catalog(&[table("venda")]);
    let effects = type_text(&mut model, "select venda.");
    let target = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCompletionColumns { target, .. } => Some(target.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no column lookup: {effects:?}"));
    assert_eq!(target.object(), "venda");
    assert_eq!(target.schema(), Some("public"));

    update(
        &mut model,
        Action::CompletionColumnsLoaded {
            generation: 3,
            target,
            columns: vec!["id".into(), "cliente_id".into(), "total".into()],
        },
    );
    assert!(model.editor.completion_open, "the answer did not open it");
    assert_eq!(offered(&model), ["id", "cliente_id", "total"]);

    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert_eq!(model.active_document().text(), "select venda.id");
}

#[test]
fn an_answer_for_another_connection_is_dropped() {
    let mut model = connected();
    update(
        &mut model,
        Action::CompletionColumnsLoaded {
            generation: 2,
            target: QualifiedName::new(None::<String>, Some("public"), "orders"),
            columns: vec!["total".into()],
        },
    );
    assert!(model.catalog_objects.is_empty());
}

#[test]
fn switching_connections_forgets_the_previous_catalog() {
    let mut model = connected();
    model.absorb_catalog(&[table("orders")]);
    model.connection.name = "other".into();
    model.absorb_catalog(&[table("invoices")]);
    let names: Vec<_> = model
        .catalog_objects
        .iter()
        .map(|object| object.qualified_name.object())
        .collect();
    assert_eq!(names, ["invoices"]);
}

#[test]
fn accepting_a_function_puts_the_cursor_between_its_parentheses() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    type_text(&mut model, "select coun");
    model.editor.completion_selected = model
        .editor
        .completions
        .iter()
        .position(|item| item.label == "count")
        .expect("count is not offered");
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert_eq!(model.active_document().text(), "select count()");
    assert_eq!(model.active_document().cursor(), "select count(".len());
}
