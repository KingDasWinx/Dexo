use dexo_app::diagnostic_service::{
    DiagnosticBundle, SECRET_SENTINEL, contains_sentinel, redact_text,
};
use dexo_app::mcp::audit::{AuditEvent, SqlAuditMode};
use dexo_storage::{Database, Preferences, ProjectRepository};
use dexo_app::{Project, ProjectId};

#[test]
fn sentinel_absent_from_db_toml_logs_audit_and_zip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("dexo.db")).unwrap();
    let id = uuid::Uuid::new_v4();
    ProjectRepository::new(db.connection())
        .save(&Project {
            id: ProjectId(id),
            name: "p".into(),
            created_at: "now".into(),
        })
        .unwrap();
    let db_bytes = std::fs::read(dir.path().join("dexo.db")).unwrap();
    assert!(!contains_sentinel(&db_bytes));

    let prefs = Preferences::from_toml(&format!(
        "theme = \"dark\"\npassword = \"{SECRET_SENTINEL}\"\n"
    ))
    .unwrap();
    let toml = redact_text(&prefs.to_toml().unwrap());
    assert!(!toml.contains(SECRET_SENTINEL));

    let log = redact_text(&format!(
        "connected postgres://u:{SECRET_SENTINEL}@localhost/db param={SECRET_SENTINEL}"
    ));
    assert!(!log.contains(SECRET_SENTINEL));

    let event = AuditEvent {
        timestamp: 1,
        request: "tools/call".into(),
        operation_id: Some("op".into()),
        profile: "p".into(),
        client: "c".into(),
        target: "db.public.items".into(),
        decision: "allow".into(),
        grant_id: None,
        duration_ms: 1,
        rows: 0,
        bytes: 0,
        status: "ok".into(),
        sql: None,
    }
    .sanitize(SqlAuditMode::Sanitized, Some(&format!("select '{SECRET_SENTINEL}'")));
    assert!(!event.export_line().contains(SECRET_SENTINEL));

    let bundle = DiagnosticBundle::assemble(
        "0.1.0".into(),
        "none".into(),
        format!("password={SECRET_SENTINEL}"),
        format!("panic {SECRET_SENTINEL}"),
    );
    let zip = dir.path().join("diag.zip");
    bundle.write_zip(&zip).unwrap();
    assert!(!contains_sentinel(&std::fs::read(zip).unwrap()));
    assert!(!bundle.preview.contains(SECRET_SENTINEL));
}
