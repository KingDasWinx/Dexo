use dexo_app::{Project, ProjectId};
use dexo_storage::{
    Database, DocumentRepository, LayoutRepository, ProjectRepository, WorkbenchLayout,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExpectedWorkspace {
    project: String,
    sql: String,
    explorer_width: u16,
    focused_panel: String,
    active_document_id: String,
    active_connection_id: String,
}

fn expected_workspace() -> ExpectedWorkspace {
    ExpectedWorkspace {
        project: "demo".into(),
        sql: "select 42".into(),
        explorer_width: 40,
        focused_panel: "results".into(),
        active_document_id: "doc-1".into(),
        active_connection_id: "local-pg".into(),
    }
}

fn seed_and_shutdown(home: &std::path::Path, expected: &ExpectedWorkspace) {
    let db = Database::open(home.join("dexo.db")).unwrap();
    let project = Project {
        id: ProjectId(uuid::Uuid::new_v4()),
        name: expected.project.clone(),
        created_at: "1".into(),
    };
    let repo = ProjectRepository::new(db.connection());
    repo.save(&project).unwrap();
    DocumentRepository::new(db.connection())
        .save(
            &expected.active_document_id,
            Some(&project.id.0.to_string()),
            "scratch.sql",
            &expected.sql,
            None,
            None,
        )
        .unwrap();
    let layout = WorkbenchLayout {
        explorer_width: expected.explorer_width,
        focused_panel: expected.focused_panel.clone(),
        active_document_id: Some(expected.active_document_id.clone()),
        active_connection_id: Some(expected.active_connection_id.clone()),
        document_ids: vec![expected.active_document_id.clone()],
        ..WorkbenchLayout::default()
    };
    LayoutRepository::new(db.connection())
        .save(&project.id.0.to_string(), &layout)
        .unwrap();
}

fn bootstrap(home: &std::path::Path) -> ExpectedWorkspace {
    let db = Database::open(home.join("dexo.db")).unwrap();
    let project = ProjectRepository::new(db.connection())
        .get_by_name("demo")
        .unwrap()
        .unwrap();
    let documents = DocumentRepository::new(db.connection())
        .list_for_project(&project.id.0.to_string())
        .unwrap();
    let layout = LayoutRepository::new(db.connection())
        .load(&project.id.0.to_string())
        .unwrap()
        .unwrap();
    ExpectedWorkspace {
        project: project.name,
        sql: documents[0].content.clone(),
        explorer_width: layout.explorer_width,
        focused_panel: layout.focused_panel,
        active_document_id: layout.active_document_id.unwrap_or_default(),
        active_connection_id: layout.active_connection_id.unwrap_or_default(),
    }
}

#[test]
fn restart_restores_project_documents_layout_and_active_items() {
    let home = tempfile::tempdir().unwrap();
    let expected = expected_workspace();
    seed_and_shutdown(home.path(), &expected);
    let restored = bootstrap(home.path());
    assert_eq!(restored, expected);
    let compact = WorkbenchLayout {
        explorer_width: 200,
        inspector_width: 200,
        results_height: 80,
        ..WorkbenchLayout::default()
    }
    .clamp(50, 18);
    assert!(!compact.explorer_visible);
}
