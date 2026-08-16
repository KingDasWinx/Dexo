use dexo_driver_api::{CatalogList, CatalogObject, DbValue, ObjectId, ObjectKind, QualifiedName};
use dexo_storage::{CatalogCache, Database};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::runtime::OperationId;
use dexo_tui::update;
use uuid::Uuid;

fn object(
    id: &str,
    kind: ObjectKind,
    qualified: (&str, &str, &str),
    parent: Option<&str>,
) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(id),
        kind,
        QualifiedName::new(Some(qualified.0), Some(qualified.1), qualified.2),
        parent.map(ObjectId::new),
    )
}

fn catalog_fixture() -> CatalogList {
    CatalogList {
        objects: vec![object(
            "schema:public",
            ObjectKind::Schema,
            ("db", "public", "public"),
            Some("catalog:db"),
        )],
        restrictions: vec![],
    }
}

struct CatalogHarness {
    model: Model,
    calls: Vec<String>,
    pending: Vec<(OperationId, Option<ObjectId>, bool)>,
}

impl CatalogHarness {
    async fn new(_fixture: CatalogList) -> Self {
        let mut model = Model::default();
        model.connection.ready = true;
        model.session_generation = 1;
        model.active_session = Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1)));
        model.explorer.replace_roots(CatalogList {
            objects: vec![object(
                "catalog:db",
                ObjectKind::Catalog,
                ("db", "db", "db"),
                None,
            )],
            restrictions: vec![],
        });
        model
            .explorer
            .apply_children(&ObjectId::new("catalog:db"), catalog_fixture());
        model.explorer.select(ObjectId::new("schema:public"));
        Self {
            model,
            calls: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn reader_calls(&self) -> Vec<String> {
        self.calls.clone()
    }

    fn model(&self) -> &Model {
        &self.model
    }

    fn capture(&mut self, effects: Vec<dexo_tui::Effect>, complete: bool) -> Option<OperationId> {
        let mut last = None;
        for effect in effects {
            if let dexo_tui::Effect::LoadCatalogChildren {
                parent,
                operation,
                replace_roots,
                ..
            } = effect
            {
                let label = parent
                    .as_ref()
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_else(|| "root".into());
                self.calls.push(label);
                self.pending
                    .push((operation, parent.clone(), replace_roots));
                last = Some(operation);
                if complete {
                    self.finish(operation);
                }
            }
        }
        last
    }

    fn finish(&mut self, operation: OperationId) {
        let Some(index) = self.pending.iter().position(|(id, _, _)| *id == operation) else {
            return;
        };
        let (_, parent, replace_roots) = self.pending.remove(index);
        let session = self
            .model
            .active_session
            .map(|id| id.0.to_string())
            .unwrap_or_default();
        let generation = self.model.session_generation;
        let list = if parent.as_ref().map(|id| id.as_str()) == Some("schema:public") {
            CatalogList {
                objects: vec![object(
                    "table:orders",
                    ObjectKind::Table,
                    ("db", "public", "orders"),
                    Some("schema:public"),
                )],
                restrictions: vec![],
            }
        } else {
            catalog_fixture()
        };
        let _ = update(
            &mut self.model,
            Action::CatalogLoaded {
                operation,
                session,
                generation,
                parent,
                list,
                replace_roots,
            },
        );
    }

    async fn expand(&mut self, id: &str) {
        self.model.explorer.select(ObjectId::new(id));
        let effects = update(&mut self.model, Action::ExplorerExpand);
        self.capture(effects, true);
    }

    async fn start_refresh(&mut self, id: &str) -> OperationId {
        self.model.explorer.select(ObjectId::new(id));
        let effects = update(&mut self.model, Action::RefreshCatalogNode);
        self.capture(effects, false).expect("refresh effect")
    }

    async fn switch_connection(&mut self) {
        let generation = self.model.session_generation + 1;
        let _ = update(
            &mut self.model,
            Action::ConnectionChanged {
                name: "other".into(),
                ready: true,
                environment: "local".into(),
                session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(2))),
                generation,
                token: 0,
                read_only: false,
            },
        );
    }

    async fn complete(&mut self, old: OperationId) {
        let session = self
            .model
            .active_session
            .map(|id| id.0.to_string())
            .unwrap_or_default();
        let _ = update(
            &mut self.model,
            Action::CatalogLoaded {
                operation: old,
                session: Uuid::from_u128(1).to_string(),
                generation: 1,
                parent: Some(ObjectId::new("schema:public")),
                list: catalog_fixture(),
                replace_roots: false,
            },
        );
        let _ = session;
    }
}

#[tokio::test]
async fn expanding_loads_only_selected_subtree_and_ignores_old_refresh() {
    let mut harness = CatalogHarness::new(catalog_fixture()).await;
    harness.expand("schema:public").await;
    assert_eq!(harness.reader_calls(), vec!["schema:public"]);
    let old = harness.start_refresh("schema:public").await;
    harness.switch_connection().await;
    harness.complete(old).await;
    assert!(harness.model().explorer.nodes().is_empty());
}

#[tokio::test]
async fn inspector_loads_properties_ddl_dependencies_and_privileges() {
    let mut model = Model {
        session_generation: 1,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        ..Model::default()
    };
    let _ = update(
        &mut model,
        Action::InspectorLoaded {
            generation: 1,
            session: Uuid::from_u128(1).to_string(),
            qualified_name: "db.public.orders".into(),
            object: Some(object(
                "table:orders",
                ObjectKind::Table,
                ("db", "public", "orders"),
                None,
            )),
            ddl: Some("CREATE TABLE orders (id int)".into()),
            dependencies: vec![ObjectId::new("table:customers")],
            dependents: vec![ObjectId::new("view:orders_v")],
            effective_privileges: vec!["SELECT".into()],
            restrictions: vec![],
        },
    );
    assert_eq!(model.inspector.qualified_name, "db.public.orders");
    assert!(
        model
            .inspector
            .ddl
            .as_deref()
            .unwrap()
            .contains("CREATE TABLE")
    );
    assert!(!model.inspector.dependencies.is_empty());
    assert!(
        model
            .inspector
            .effective_privileges
            .contains(&"SELECT".into())
    );
}

#[test]
fn clipboard_failure_is_not_success() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Action::ClipboardFailed {
            message: "clipboard unavailable".into(),
        },
    );
    assert!(model.explorer.copied.is_none());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.contains("clipboard"))
    );
}

#[test]
fn goto_selects_catalog_object() {
    let mut model = Model::default();
    model.explorer.replace_roots(CatalogList {
        objects: vec![object(
            "table:orders",
            ObjectKind::Table,
            ("db", "public", "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model
        .active_document_mut()
        .sql
        .insert(0, "select o.id from public.orders o")
        .unwrap();
    model.active_document_mut().sql.set_cursor(9).unwrap();
    let _ = update(&mut model, Action::GoToDefinition);
    assert_eq!(
        model.explorer.selected.as_ref().map(|id| id.as_str()),
        Some("table:orders")
    );
}

#[test]
fn offline_snapshot_rejects_incomplete_and_keeps_complete() {
    let db = Database::open_in_memory().unwrap();
    let cache = CatalogCache::new(db.connection());
    let objects = vec![object(
        "table:orders",
        ObjectKind::Table,
        ("db", "public", "orders"),
        None,
    )];
    cache.replace_snapshot("c1", "db", &objects).unwrap();
    db.connection()
        .execute(
            "INSERT INTO catalog_snapshots(id, connection_id, database_name, complete, created_at)
             VALUES ('incomplete', 'c1', 'db', 0, datetime('now'))",
            [],
        )
        .unwrap();
    assert!(cache.latest_metadata("c1", "db").unwrap().unwrap().complete);
    assert_eq!(cache.load_latest("c1", "db").unwrap().len(), 1);
}

#[test]
fn open_object_data_requests_a_table_page() {
    let mut model = Model {
        session_generation: 1,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        ..Model::default()
    };
    model.explorer.replace_roots(CatalogList {
        objects: vec![object(
            "table:orders",
            ObjectKind::Table,
            ("db", "public", "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new("table:orders"));
    let effects = update(&mut model, Action::OpenObjectData);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadTableData { .. }))
    );
}

#[test]
fn data_page_fills_the_active_grid() {
    let mut model = Model {
        session_generation: 1,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        ..Model::default()
    };
    let _ = update(
        &mut model,
        Action::DataPageLoaded {
            generation: 1,
            session: Uuid::from_u128(1).to_string(),
            page: dexo_driver_api::DataPage {
                columns: vec![dexo_driver_api::ColumnMeta {
                    name: "id".into(),
                    type_name: "int".into(),
                    nullable: false,
                }],
                rows: vec![vec![DbValue::I64(1)]],
                offset: 0,
                has_more: true,
                estimated_total: None,
            },
        },
    );
    assert_eq!(model.results.row_count(), 1);
    assert!(model.data.has_more);
}

#[test]
fn replace_roots_requests_snapshot_capture() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    model.connection.ready = true;
    model.connection.name = "local".into();
    let effects = update(
        &mut model,
        Action::CatalogLoaded {
            operation: OperationId::new(),
            session: Uuid::from_u128(1).to_string(),
            generation: 1,
            parent: None,
            list: catalog_fixture(),
            replace_roots: true,
        },
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::CaptureCatalogSnapshot { .. }))
    );
}

#[test]
fn explorer_up_down_moves_selection() {
    let mut model = Model::default();
    model.explorer.replace_roots(CatalogList {
        objects: vec![
            object("catalog:db", ObjectKind::Catalog, ("db", "db", "db"), None),
            object(
                "schema:public",
                ObjectKind::Schema,
                ("db", "public", "public"),
                Some("catalog:db"),
            ),
        ],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new("catalog:db"));
    update(&mut model, Action::ExplorerDown);
    assert_eq!(
        model.explorer.selected.as_ref().map(|id| id.as_str()),
        Some("schema:public")
    );
    update(&mut model, Action::ExplorerUp);
    assert_eq!(
        model.explorer.selected.as_ref().map(|id| id.as_str()),
        Some("catalog:db")
    );
    let lines = dexo_tui::widgets::object_tree::render_lines(&model.explorer);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with('>') && line.contains("db")),
        "{lines:?}"
    );
}
