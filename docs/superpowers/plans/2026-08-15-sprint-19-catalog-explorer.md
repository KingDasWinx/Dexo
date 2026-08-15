# Dexo Sprint 19: Live Catalog Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the explorer fixture with a lazy, searchable, cached PostgreSQL/MySQL catalog that opens real inspectors and supports offline work.

**Architecture:** The runtime invokes the active session's `CatalogReader`; every node carries loading/error/stale state and is correlated to a catalog generation. Complete snapshots are written atomically to SQLite, while favorites/recency are project-scoped metadata layered over live or offline objects.

**Tech Stack:** Driver catalog capabilities, rusqlite, dexo-sql completion, arboard system clipboard, Tokio, Ratatui.

---

## File map

Create `crates/dexo-tui/src/runtime/catalog_manager.rs`, `crates/dexo-tui/src/screens/object_inspector.rs`, `crates/dexo-tui/src/runtime/clipboard.rs`, `crates/dexo-tui/tests/catalog_flow.rs`, and `crates/dexo/tests/tui_catalog_live.rs`.

Modify driver catalogs, `dexo-app/src/catalog_service.rs`, `search_service.rs`, storage migrations/cache, TUI explorer/action/model/update/widgets/palette, and SQL completion/navigation.

Requires Sprints 16–18 green.

### Task 1: Persist project favorites, recency, and snapshot metadata

**Files:** `crates/dexo-storage/src/migrations.rs`, `catalog_cache.rs`, new `object_usage.rs`, `lib.rs`, migration/cache tests.

- [ ] **Step 1: Add a failing migration-10 test**

```rust
#[test]
fn migration_10_adds_project_object_usage() {
    let db = database_at_version(9);
    apply_pending(db.connection()).unwrap();
    assert!(table_exists(db.connection(), "object_usage"));
    assert_eq!(read_schema_version(db.connection()), 10);
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cargo test -p dexo-storage --test migration migration_10_adds_project_object_usage`

Expected: FAIL at version 9.

- [ ] **Step 3: Add migration and repository**

```sql
CREATE TABLE object_usage(
  project_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  object_id TEXT NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  opened_count INTEGER NOT NULL DEFAULT 0,
  last_opened_at TEXT,
  PRIMARY KEY(project_id,connection_id,object_id)
);
```

Implement `set_favorite`, `touch`, `list_for_connection`, and cleanup on profile/project deletion. Add `CatalogCache::latest_metadata` and reject incomplete snapshots for offline use.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-storage catalog_cache object_usage`

Expected: PASS.

```powershell
git add crates/dexo-storage
git commit -m "feat(storage): persist catalog favorites and recency"
```

### Task 2: Make dependencies and restrictions truthful in both drivers

**Files:** PostgreSQL/MySQL catalog modules and their Docker tests.

- [ ] **Step 1: Add dependency graph contracts**

Create schema objects with a table, view, foreign key, trigger, and routine. Assert `dependencies` and `dependents` return stable `ObjectId`s. Run as a least-privilege user and assert a `CatalogRestriction` or permission error rather than an empty success.

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test catalog -- --ignored --nocapture`

Expected: MySQL dependency cases FAIL because they return `Vec::new()`.

- [ ] **Step 2: Implement MySQL relations**

Query `information_schema.KEY_COLUMN_USAGE`, `VIEW_TABLE_USAGE`, `ROUTINES`, and trigger metadata, normalize IDs using the same constructors as `list_children`, and return restrictions when metadata access is denied.

- [ ] **Step 3: Tighten capability reporting**

If a server version or privilege set cannot implement a catalog operation, return `DriverError::unsupported`/restriction with the exact reason. Never interpret permission failure as “no children/dependencies.”

- [ ] **Step 4: Run and commit**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test catalog -- --ignored --nocapture`

Expected: all catalog contracts PASS.

```powershell
git add crates/dexo-driver-postgres/src/catalog crates/dexo-driver-postgres/tests/catalog.rs crates/dexo-driver-mysql/src/catalog crates/dexo-driver-mysql/tests/catalog.rs
git commit -m "feat(catalog): expose real dependencies and restrictions"
```

### Task 3: Load and refresh the explorer lazily

**Files:** `runtime/catalog_manager.rs`, TUI explorer/action/model/update/object_tree, tests `catalog_flow.rs`.

- [ ] **Step 1: Write lazy loading, retry, and stale-generation tests**

```rust
#[tokio::test]
async fn expanding_loads_only_selected_subtree_and_ignores_old_refresh() {
    let harness = CatalogHarness::new(catalog_fixture()).await;
    harness.expand("schema:public").await;
    assert_eq!(harness.reader_calls(), vec!["schema:public"]);
    let old = harness.start_refresh("schema:public").await;
    harness.switch_connection().await;
    harness.complete(old).await;
    assert!(harness.model().explorer.nodes.is_empty());
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test catalog_flow expanding_loads_only_selected_subtree_and_ignores_old_refresh`

Expected: FAIL because `ExplorerExpand` only toggles local state.

- [ ] **Step 3: Add real effects and node states**

```rust
pub enum NodeState { Collapsed, Loading(OperationId), Expanded, Error { message: String, retryable: bool }, Stale }
```

`ExpandCatalogNode`, `RefreshCatalogNode`, `RefreshCatalogSubtree`, and `RefreshCatalogAll` call `CatalogService`; completions include session/catalog generation. Replace roots on full refresh and children only on node refresh.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-tui --test catalog_flow lazy refresh`

Expected: PASS.

```powershell
git add crates/dexo-tui/src/runtime/catalog_manager.rs crates/dexo-tui/src/screens/explorer.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/widgets/object_tree.rs crates/dexo-tui/tests/catalog_flow.rs
git commit -m "feat(tui): load and refresh catalog nodes lazily"
```

### Task 4: Connect search, filters, favorites, and autocomplete

**Files:** app search/catalog service, runtime/storage worker/catalog manager, TUI explorer/editor/palette, tests.

- [ ] **Step 1: Write a failing project-aware search test**

```rust
#[test]
fn search_ranks_favorite_then_recent_without_returning_denied_objects() {
    let hits = search_with_usage(objects(), usage(), restrictions(), "ord");
    assert_eq!(hits[0].object.qualified_name.object(), "orders");
    assert!(hits.iter().all(|hit| hit.object.qualified_name.object() != "secret_orders"));
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-app search_ranks_favorite_then_recent_without_returning_denied_objects`

Expected: FAIL because usage/restrictions are not composed.

- [ ] **Step 3: Compose the index**

Build `SearchDocument` from live/offline objects plus `ObjectUsage`; exclude restricted objects before indexing. Explorer filters include text, kind, schema, system objects, favorites only, and offline/stale. Feed the same snapshot into `SnapshotCatalog` for editor completion.

- [ ] **Step 4: Test performance and commit**

Run: `cargo test -p dexo-app search && cargo bench -p dexo-app catalog_search`

Expected: exact/prefix/fuzzy ranking PASS and 100k-object budget remains within the documented baseline.

```powershell
git add crates/dexo-app/src/{search_service.rs,catalog_service.rs} crates/dexo-tui/src crates/dexo-storage/src crates/dexo-tui/tests/catalog_flow.rs
git commit -m "feat(catalog): connect search favorites and completion"
```

### Task 5: Open real object inspectors and copy through the OS clipboard

**Files:** `screens/object_inspector.rs`, `runtime/catalog_manager.rs`, `runtime/clipboard.rs`, Cargo dependency, TUI widgets/update/palette, tests.

- [ ] **Step 1: Write inspector and clipboard tests**

```rust
#[tokio::test]
async fn inspector_loads_properties_ddl_dependencies_and_privileges() {
    let inspector = harness().open_object("table:orders").await;
    assert_eq!(inspector.qualified_name, "db.public.orders");
    assert!(inspector.ddl.as_deref().unwrap().contains("CREATE TABLE"));
    assert!(!inspector.dependencies.is_empty());
    assert!(inspector.effective_privileges.contains(&"SELECT".into()));
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test catalog_flow inspector_loads_properties_ddl_dependencies_and_privileges`

Expected: FAIL.

- [ ] **Step 3: Implement inspector loading and clipboard adapter**

Call `CatalogService::{object,ddl}`, reader dependencies/dependents, and `SecurityAdmin::effective_privileges`. Render each partial result or typed restriction independently.

```rust
pub fn copy_text(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}
```

Copy simple name, qualified name, and DDL only reports success after `arboard` returns `Ok`.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-tui --test catalog_flow inspector clipboard`

Expected: PASS; headless clipboard failure is rendered as a safe error, not success.

```powershell
git add Cargo.toml crates/dexo-tui/Cargo.toml crates/dexo-tui/src/screens/object_inspector.rs crates/dexo-tui/src/runtime/{catalog_manager.rs,clipboard.rs} crates/dexo-tui/src crates/dexo-tui/tests/catalog_flow.rs
git commit -m "feat(tui): inspect and copy live catalog objects"
```

### Task 6: Add go-to-definition and open data/DDL actions

**Files:** `dexo-sql` navigation module, TUI editor/explorer/actions, tests.

- [ ] **Step 1: Write a token-resolution test**

```rust
#[test]
fn goto_definition_resolves_qualified_and_aliased_names() {
    let target = definition_at("select o.id from public.orders o", 9, &catalog()).unwrap();
    assert_eq!(target.display_unquoted(), "db.public.orders.id");
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-sql goto_definition_resolves_qualified_and_aliased_names`

Expected: FAIL.

- [ ] **Step 3: Implement navigation and effects**

Resolve the cursor token using parsed aliases and catalog names. `GoToDefinition` selects/reveals the object, loading missing ancestors. `OpenObjectData` emits the data-load effect defined for Sprint 20; until Sprint 20 begins it may be disabled with the explicit reason `data tabs require Sprint 20`, never return fixture rows. `OpenObjectDdl` opens the real inspector DDL immediately.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-sql -p dexo-tui --test catalog_flow goto`

Expected: PASS.

```powershell
git add crates/dexo-sql/src crates/dexo-tui/src crates/dexo-tui/tests/catalog_flow.rs
git commit -m "feat(editor): navigate SQL tokens to catalog objects"
```

### Task 7: Persist complete offline snapshots and fallback safely

**Files:** runtime/catalog manager/storage worker, storage cache, explorer/model/render, live tests.

- [ ] **Step 1: Write online-capture/offline-reopen tests**

Capture a complete live tree, close the session, reopen the project, assert the same visible object IDs/DDL/search completion, and mark the timestamp/stale status. Interrupt capture and assert the old complete snapshot remains active.

Run: `cargo test -p dexo-tui --test catalog_flow offline_snapshot`

Expected: FAIL.

- [ ] **Step 2: Implement atomic snapshot capture**

Walk children with bounded concurrency, collect restrictions, write `complete=0`, insert objects transactionally, then set `complete=1`. Cancellation deletes the incomplete snapshot. Startup chooses live catalog when connected and latest complete cache otherwise.

- [ ] **Step 3: Add live acceptance tests**

Run: `cargo test -p dexo --test tui_catalog_live -- --ignored --nocapture`

Expected: PostgreSQL/MySQL tree, refresh, inspector, dependencies, privileges, and offline reopen PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/dexo-tui/src/runtime crates/dexo-tui/src/screens/explorer.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/render.rs crates/dexo-storage/src/catalog_cache.rs crates/dexo-tui/tests/catalog_flow.rs crates/dexo/tests/tui_catalog_live.rs
git commit -m "feat(catalog): persist and reopen offline snapshots"
```

### Task 8: Run the catalog sprint gate

- [ ] **Step 1: Run all gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test catalog -- --ignored --nocapture
cargo test -p dexo --test tui_catalog_live -- --ignored --nocapture
```

Expected: PASS.

- [ ] **Step 2: Confirm no production explorer fixture path**

Run: `rg -n "ExplorerState::fixture|fixture\(" crates/dexo-tui/src`

Expected: no explorer production call; fixture builders may remain under `#[cfg(test)]` only.

- [ ] **Step 3: Commit gate state**

```powershell
git add .
git commit -m "test(catalog): verify live and offline explorer"
```

## Sprint 19 exit checklist

- [ ] Tree, refresh, filters, search, favorites, inspectors, DDL, dependencies, privileges, copy, and go-to-definition are real.
- [ ] Offline snapshots are complete, atomic, searchable, and visibly stale.
- [ ] Permission restrictions never masquerade as empty results.
- [ ] PostgreSQL/MySQL catalog acceptance tests pass.
