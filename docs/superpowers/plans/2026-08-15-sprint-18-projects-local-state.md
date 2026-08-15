# Dexo Sprint 18: Projects and Local State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make projects, documents, snippets, layouts, recents, history, and portable configuration durable and manageable from the TUI.

**Architecture:** Keep SQLite ownership in `StorageWorker`, add migration 9 and complete repositories, then model project switching as a coordinated save-close-load action. External SQL files remain user-owned; project deletion previews and removes only Dexo metadata unless separately confirmed.

**Tech Stack:** rusqlite, TOML/Serde, SHA-256 fingerprints, Tokio filesystem, Ratatui forms, tempfile.

---

## File map

Create `crates/dexo-tui/src/screens/projects.rs`, `crates/dexo-tui/src/runtime/project_manager.rs`, `crates/dexo-tui/src/screens/config_transfer.rs`, `crates/dexo-tui/tests/projects_flow.rs`, and `crates/dexo/tests/project_restart.rs`.

Modify `crates/dexo-storage/src/{migrations.rs,project.rs,document.rs,snippet.rs,history.rs,layout.rs,connection.rs}`, `crates/dexo-tui/src/runtime/{mod.rs,storage_worker.rs}`, and TUI `action.rs`, `model.rs`, `update.rs`, `palette.rs`, `render.rs`.

Requires Sprints 16–17 green.

### Task 1: Add migration 9 for project-scoped workbench state

**Files:** storage migrations/repositories and `crates/dexo-storage/tests/migration.rs`.

- [ ] **Step 1: Write the failing upgrade test**

```rust
#[test]
fn migration_9_scopes_snippets_history_and_recent_items() {
    let db = database_at_version(8);
    apply_pending(db.connection()).unwrap();
    assert_eq!(read_schema_version(db.connection()), 9);
    assert!(column_exists(db.connection(), "snippets", "project_id"));
    assert!(column_exists(db.connection(), "sql_history", "project_id"));
    assert!(table_exists(db.connection(), "recent_items"));
}
```

- [ ] **Step 2: Run and see version 8**

Run: `cargo test -p dexo-storage --test migration migration_9_scopes_snippets_history_and_recent_items`

Expected: FAIL.

- [ ] **Step 3: Add migration 9**

```sql
BEGIN;
ALTER TABLE snippets ADD COLUMN project_id TEXT;
ALTER TABLE sql_history ADD COLUMN project_id TEXT;
CREATE TABLE recent_items(
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  item_id TEXT NOT NULL,
  opened_at TEXT NOT NULL,
  PRIMARY KEY(project_id,kind,item_id),
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE TABLE project_state(
  project_id TEXT PRIMARY KEY,
  active_document_id TEXT,
  active_connection_id TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
INSERT INTO schema_migrations(version,applied_at) VALUES(9,datetime('now'));
COMMIT;
```

Backfill legacy snippets/history into the persisted Default project.

- [ ] **Step 4: Run all migration tests and commit**

Run: `cargo test -p dexo-storage --test migration --test schema_fixtures`

Expected: every released schema upgrades to 9.

```powershell
git add crates/dexo-storage/src/migrations.rs crates/dexo-storage/tests
git commit -m "feat(storage): scope local workbench state by project"
```

### Task 2: Complete project/document/snippet/history CRUD

**Files:** `crates/dexo-storage/src/project.rs`, `document.rs`, `snippet.rs`, `history.rs`; tests `crates/dexo-storage/tests/project_repository.rs`, `workbench.rs`.

- [ ] **Step 1: Add failing repository contracts**

```rust
#[test]
fn project_resources_can_be_listed_moved_cleared_and_deleted() {
    let db = Database::open_in_memory().unwrap();
    let (a, b) = seed_two_projects(&db);
    let docs = DocumentRepository::new(db.connection());
    docs.save("d1", Some(&a), "scratch", "select 1", None, None).unwrap();
    docs.move_to_project("d1", &b).unwrap();
    assert_eq!(docs.list_for_project(&b).unwrap().len(), 1);
    ProjectRepository::new(db.connection()).delete(parse_project(a)).unwrap();
    assert!(ProjectRepository::new(db.connection()).get(parse_project(a)).unwrap().is_none());
}
```

- [ ] **Step 2: Run and verify missing methods**

Run: `cargo test -p dexo-storage --test project_repository project_resources_can_be_listed_moved_cleared_and_deleted`

Expected: compile FAIL.

- [ ] **Step 3: Implement exact repository methods**

Add project `get_by_name`, `rename`, `delete`; document `get`, `list_for_project`, `move_to_project`, `delete`; snippet `list_for_project`, `rename`, `delete`; history `list_for_project`, `clear_for_project`, `clear_for_connection`; recents `touch`, `list`, `clear`. Use SQL transactions for project deletion previews/apply.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-storage --test project_repository --test workbench`

Expected: PASS.

```powershell
git add crates/dexo-storage/src crates/dexo-storage/tests/project_repository.rs crates/dexo-storage/tests/workbench.rs
git commit -m "feat(storage): complete project resource repositories"
```

### Task 3: Implement coordinated project switching

**Files:** `crates/dexo-tui/src/runtime/project_manager.rs`, runtime/storage worker, `action.rs`, `model.rs`, `update.rs`, tests `projects_flow.rs`.

- [ ] **Step 1: Write a failing switch-order test**

```rust
#[tokio::test]
async fn switching_flushes_old_project_before_loading_new_project() {
    let harness = ProjectHarness::new().await;
    harness.dirty_active_document("select 42").await;
    harness.switch_to("Project B").await.unwrap();
    assert_eq!(harness.stored_document("Project A").await, "select 42");
    assert_eq!(harness.model().project.name, "Project B");
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test projects_flow switching_flushes_old_project_before_loading_new_project`

Expected: FAIL.

- [ ] **Step 3: Implement the state machine**

```rust
pub enum ProjectSwitchStage { ConfirmDirty, FlushDocuments, PersistLayout, CloseProjectSessions, LoadTarget, Complete }
```

`SwitchProject` creates one operation and advances only on successful completion actions. Failure leaves the old project active. Active transactions require commit/rollback/cancel choice; never close silently.

- [ ] **Step 4: Run and commit**

Run: `cargo test -p dexo-tui --test projects_flow switch`

Expected: success, dirty, transaction, and storage-failure cases PASS.

```powershell
git add crates/dexo-tui/src/runtime/project_manager.rs crates/dexo-tui/src/runtime crates/dexo-tui/src/action.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/projects_flow.rs
git commit -m "feat(tui): switch projects without losing state"
```

### Task 4: Build project CRUD and deletion preview UX

**Files:** `screens/projects.rs`, TUI action/model/update/palette/render, tests `projects_flow.rs` and snapshots.

- [ ] **Step 1: Add failing create/rename/delete UI tests**

Test duplicate names, validation, resource counts, connection detach/delete choice, external file preservation, and recent-project ordering.

Run: `cargo test -p dexo-tui --test projects_flow project_crud`

Expected: FAIL.

- [ ] **Step 2: Add screen state**

```rust
pub struct ProjectDeletePreview {
    pub project: dexo_app::ProjectId,
    pub connections: usize,
    pub documents: usize,
    pub snippets: usize,
    pub external_paths: Vec<std::path::PathBuf>,
    pub delete_connections: bool,
}
```

- [ ] **Step 3: Wire real effects and rendering**

Create/rename/delete/open actions emit storage effects. Delete never unlinks `external_paths`; it clears Dexo metadata/recovery only after typed project-name confirmation. Connections default to detach, with explicit delete/keychain decisions delegated to Sprint 17 handlers.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-tui --test projects_flow && cargo test -p dexo-tui --test snapshots`

Expected: PASS with intentional snapshots.

```powershell
git add crates/dexo-tui/src/screens/projects.rs crates/dexo-tui/src/{action.rs,model.rs,update.rs,palette.rs,render.rs} crates/dexo-tui/tests
git commit -m "feat(tui): manage projects and deletion impact"
```

### Task 5: Persist and restore layout, tabs, focus, and recents

**Files:** storage `layout.rs`, runtime/storage worker, TUI model/layout/update, tests `projects_flow.rs`, `crates/dexo/tests/project_restart.rs`.

- [ ] **Step 1: Write a restart test**

```rust
#[test]
fn restart_restores_project_documents_layout_and_active_items() {
    let home = tempfile::tempdir().unwrap();
    seed_and_shutdown(home.path(), expected_workspace());
    let restored = bootstrap(home.path());
    assert_eq!(restored, expected_workspace());
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo --test project_restart restart_restores_project_documents_layout_and_active_items`

Expected: FAIL because `PersistLayout` is ignored and tabs are not stored.

- [ ] **Step 3: Version the persisted workspace**

Extend `WorkbenchLayout` with document tab IDs, focused panel, active result/editor tabs, panes, and selected connection/profile IDs. Debounce resize persistence; flush immediately on project switch and shutdown. Clamp restored sizes to terminal dimensions.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-storage layout && cargo test -p dexo --test project_restart`

Expected: PASS across compact/full dimensions.

```powershell
git add crates/dexo-storage/src/layout.rs crates/dexo-tui/src/runtime crates/dexo-tui/src/{model.rs,layout.rs,update.rs} crates/dexo/tests/project_restart.rs
git commit -m "feat(tui): persist complete project workspace state"
```

### Task 6: Add portable config import/export in the TUI

**Files:** `screens/config_transfer.rs`, storage `connection.rs`, runtime/storage worker, TUI action/model/update/palette/render, tests `projects_flow.rs`.

- [ ] **Step 1: Write conflict and secret-ref tests**

```rust
#[tokio::test]
async fn config_import_previews_conflicts_and_generates_fresh_secret_refs() {
    let preview = preview_import(existing_store(), portable_config()).await.unwrap();
    assert_eq!(preview.conflicts, vec!["local-pg"]);
    let report = apply_import(preview.with_resolution("local-pg", Resolution::Rename("local-pg-2".into()))).await.unwrap();
    assert_eq!(report.connections_needing_secret, vec!["local-pg-2"]);
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test projects_flow config_import_previews_conflicts_and_generates_fresh_secret_refs`

Expected: FAIL.

- [ ] **Step 3: Implement preview/apply/export effects**

Export uses atomic file writing and contains projects, non-secret profiles, policy/groups, layouts optionally, snippets optionally, and no keychain IDs. Import requires per-name replace/rename/skip choices and lists every connection needing a secret prompt.

- [ ] **Step 4: Test sentinels and commit**

Run: `cargo test -p dexo --test config_roundtrip && cargo test -p dexo-tui --test projects_flow config`

Expected: PASS and secret sentinel absent.

```powershell
git add crates/dexo-tui/src/screens/config_transfer.rs crates/dexo-tui/src/runtime crates/dexo-tui/src/{action.rs,model.rs,update.rs,palette.rs,render.rs} crates/dexo-storage/src/connection.rs crates/dexo-tui/tests/projects_flow.rs
git commit -m "feat(tui): import and export local configuration"
```

### Task 7: Run the project/local-state gate

- [ ] **Step 1: Run gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo test -p dexo --test project_restart --nocapture
cargo test -p dexo-storage --test recovery_crash --nocapture
```

Expected: PASS.

- [ ] **Step 2: Manually inspect a temporary data home**

Run: `$env:DEXO_DATA_HOME = (Join-Path $env:TEMP 'dexo-sprint18'); cargo run -p dexo`

Expected: create/open/rename/delete, restart restoration, recents clear, and config transfer operate without fixture data; no external `.sql` is deleted with a project.

- [ ] **Step 3: Commit verified state**

```powershell
git add .
git commit -m "test(projects): verify durable local project state"
```

## Sprint 18 exit checklist

- [ ] Project CRUD and switching are durable and transaction-safe.
- [ ] Connections, docs, snippets, history, recents, and layout are project-scoped.
- [ ] `PersistLayout` performs real storage I/O.
- [ ] Portable config import/export is available in the TUI and secret-free.
- [ ] Restart and crash recovery acceptance tests pass.
