# Row Delete/Insert/Commit Implementation Plan

> **For agentic workers:** Execute task-by-task, in order. Each task ends with `cargo build` (and `cargo clippy` where noted) passing clean — that is the acceptance bar for this plan, not a test suite. Per explicit user preference for this repo, do not write `#[test]` functions as part of implementing these tasks; the user writes their own tests. Only add a test if there is truly no other way to validate a step.

**Goal:** In a table document's grid (opened per `2026-09-03-table-tab-grid-primary-layout.md`), the user can mark a row for deletion (`Delete` key, red highlight, pressing it again restores the row), create a new row through a small modal (`Ctrl+N`), and either commit all pending inserts/deletes to the database (`Ctrl+S`, reusing the existing Review-modal confirmation flow) or discard everything (`Ctrl+Shift+R`). Cell editing is explicitly out of scope.

**Architecture:** The pending-change engine (`dexo_app::data::ChangeSet`, `RowIdentity`, `mutations_for`, and the TUI's `ReviewModal`/`apply_changes`/`Action::ApplyChanges` flow) already exists and already works end-to-end for `Update`/`Delete`/`Insert` — it is simply never fed from a real table today, because nothing populates `model.data.table` (the column/primary-key metadata `ChangeSet` needs to know a row's identity). This plan (a) adds the missing metadata fetch as a new driver capability, then (b) adds a `model.data.row_changes: BTreeMap<usize, RowEditState>` map that is purely a UI-side cache of "which visible grid row is pending delete/insert" — never stores an index into `ChangeSet`'s internal pending list (that list can shift when an entry is reverted), and instead re-locates the matching `PendingChange` by identity/value whenever something needs to be undone. This keeps the two data structures loosely coupled and avoids index-invalidation bugs.

**Tech Stack:** Rust, `dexo-driver-api`/`dexo-driver-postgres`/`dexo-driver-mysql` (tokio-postgres / mysql_async), ratatui (TUI), existing `dexo-tui` crate conventions.

**Spec:** None — scoped and approved section-by-section in chat during brainstorming, same as the prior plan in this directory. Key facts a future reader needs, found during brainstorming:
- `RowEditState`/`EditableRow` are defined in `dexo-app` and re-exported but **never constructed anywhere** before this plan — they were pure scaffolding.
- `ChangeSet::insert`/`update`/`delete` all silently no-op (pushing to `changes.errors()`) when the table has no primary key and no non-nullable unique column (`EditMode::ReadOnly`). This plan does not change that rule — a keyless table simply stays read-only for editing, same as today.
- `Ctrl+Enter` in the Results context is already `results.toggle_pick` (a real, existing feature) — it cannot be reused for "commit changes", hence `Ctrl+S` was chosen instead (approved in chat).

## Non-goals (explicitly out of scope)

- Cell-level editing (`RowEditState::Edited` / `PendingChange::Update` never gets constructed by this plan's UI — the machinery exists in `dexo_app::data` but wiring it up is a future increment).
- Type-aware coercion of the insert modal's text input. Every field value is captured as `DbValue::Text(input)` (empty input → `DbValue::Null`), exactly like the existing query-parameter prompt in `crates/dexo-tui/src/screens/editor.rs:270` (`submit_parameters`) already does — the driver/database handles implicit casting on bind, same as that existing code path relies on today.
- Relaxing `ChangeSet`'s read-only rule for keyless tables.
- Any change to `docs/superpowers/specs/2026-08-31-workspace-database-first-design.md` or the reverted branch.

---

### Task 1: `table_columns` capability on `DataMutator` (driver-api)

**Files:**
- Modify: `crates/dexo-driver-api/src/mutation.rs` (the `DataMutator` trait, next to `fetch`/`apply`)

**Interfaces:**
- Produces: `dexo_driver_api::ColumnKeyInfo { name: String, primary_key: bool, unique: bool }`.
- Produces: `DataMutator::table_columns(&self, target: &QualifiedName) -> Result<Vec<ColumnKeyInfo>, DriverError>` (new trait method, no default body — both drivers must implement it; this mirrors how `fetch`/`apply` have no default either).

- [ ] **Step 1: Add the struct and trait method**

In `crates/dexo-driver-api/src/mutation.rs`, above `pub trait DataMutator`, add:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnKeyInfo {
    pub name: String,
    pub primary_key: bool,
    pub unique: bool,
}
```

Add the method to the trait:

```rust
pub trait DataMutator: Send + Sync {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError>;
    async fn fetch_value(
        &self,
        value: &RemoteValueRef,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, DriverError>;
    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError>;
    async fn table_columns(&self, target: &QualifiedName) -> Result<Vec<ColumnKeyInfo>, DriverError>;
}
```

- [ ] **Step 2: Validate**

Run: `cargo build -p dexo-driver-api 2>&1 | tail -40`
Expected: this crate itself still compiles (it only defines the trait). Then run `cargo build --workspace 2>&1 | tail -80` — expect **compile errors** in `dexo-driver-postgres` and `dexo-driver-mysql` ("not all trait items implemented, missing `table_columns`") — that confirms the trait change landed and Tasks 2/3 have real work to do. Use the `LSP` tool (hover over `table_columns` in `mutation.rs`) to sanity-check the signature before moving on.

- [ ] **Step 3: Commit**

```bash
git add crates/dexo-driver-api/src/mutation.rs
git commit -m "feat(driver-api): add a table_columns capability to DataMutator"
```

---

### Task 2: Implement `table_columns` for Postgres

**Files:**
- Modify: `crates/dexo-driver-postgres/src/mutation.rs` (inside `impl DataMutator for PostgresSession`)

**Interfaces:**
- Consumes: `dexo_driver_api::ColumnKeyInfo` from Task 1; the existing `qualify`/`quote`/`map_error` helpers already used by `fetch`/`apply` in this same file.

- [ ] **Step 1: Implement the method**

Add to `impl DataMutator for PostgresSession` in `crates/dexo-driver-postgres/src/mutation.rs`, next to `fetch`:

```rust
    async fn table_columns(
        &self,
        target: &dexo_driver_api::QualifiedName,
    ) -> Result<Vec<dexo_driver_api::ColumnKeyInfo>, DriverError> {
        let sql = "
            SELECT
                a.attname AS name,
                EXISTS (
                    SELECT 1 FROM pg_constraint c
                    WHERE c.contype = 'p'
                      AND c.conrelid = a.attrelid
                      AND a.attnum = ANY (c.conkey)
                ) AS primary_key,
                EXISTS (
                    SELECT 1 FROM pg_constraint c
                    WHERE c.contype IN ('p', 'u')
                      AND c.conrelid = a.attrelid
                      AND array_length(c.conkey, 1) = 1
                      AND a.attnum = ANY (c.conkey)
                ) AS is_unique
            FROM pg_attribute a
            JOIN pg_class t ON t.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            WHERE t.relname = $1
              AND n.nspname = $2
              AND a.attnum > 0
              AND NOT a.attisdropped
            ORDER BY a.attnum
        ";
        let schema = target.schema().unwrap_or("public").to_string();
        let object = target.object().to_string();
        let refs: Vec<&(dyn ToSql + Sync)> = vec![&object, &schema];
        let rows = self.client.query(sql, &refs).await.map_err(map_error)?;
        Ok(rows
            .iter()
            .map(|row| dexo_driver_api::ColumnKeyInfo {
                name: row.get::<_, String>("name"),
                primary_key: row.get::<_, bool>("primary_key"),
                unique: row.get::<_, bool>("is_unique"),
            })
            .collect())
    }
```

If `ToSql` is not already imported at the top of this file, check the existing `use` list — `fetch`'s body already builds `Vec<&(dyn ToSql + Sync)>` (see the `refs` variable a few lines above in `fetch`), so the import is already present; do not add a duplicate.

- [ ] **Step 2: Validate**

Run: `cargo build -p dexo-driver-postgres 2>&1 | tail -60`
Expected: clean build. Use the `LSP` tool's `hover` on `row.get::<_, bool>` and on `self.client.query` if anything looks off, to confirm the inferred types before trusting the build alone.

- [ ] **Step 3: Commit**

```bash
git add crates/dexo-driver-postgres/src/mutation.rs
git commit -m "feat(driver-postgres): implement table_columns via pg_constraint"
```

---

### Task 3: Implement `table_columns` for MySQL

**Files:**
- Modify: `crates/dexo-driver-mysql/src/mutation.rs` (inside `impl DataMutator for MysqlSession`)

**Interfaces:**
- Consumes: `dexo_driver_api::ColumnKeyInfo` from Task 1; the existing `Params`, `map_error`, `self.conn.lock().await` pattern already used by `fetch`/`apply` in this same file.

- [ ] **Step 1: Implement the method**

Add to `impl DataMutator for MysqlSession` in `crates/dexo-driver-mysql/src/mutation.rs`, next to `fetch`:

```rust
    async fn table_columns(
        &self,
        target: &dexo_driver_api::QualifiedName,
    ) -> Result<Vec<dexo_driver_api::ColumnKeyInfo>, DriverError> {
        let sql = "
            SELECT
                c.COLUMN_NAME AS name,
                c.COLUMN_KEY = 'PRI' AS is_primary,
                (
                    SELECT COUNT(*) FROM information_schema.KEY_COLUMN_USAGE k
                    WHERE k.TABLE_SCHEMA = c.TABLE_SCHEMA
                      AND k.TABLE_NAME = c.TABLE_NAME
                      AND k.COLUMN_NAME = c.COLUMN_NAME
                      AND k.CONSTRAINT_NAME IN (
                          SELECT tc.CONSTRAINT_NAME FROM information_schema.TABLE_CONSTRAINTS tc
                          WHERE tc.TABLE_SCHEMA = c.TABLE_SCHEMA
                            AND tc.TABLE_NAME = c.TABLE_NAME
                            AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE')
                      )
                ) > 0 AS is_unique
            FROM information_schema.COLUMNS c
            WHERE c.TABLE_SCHEMA = ?
              AND c.TABLE_NAME = ?
            ORDER BY c.ORDINAL_POSITION
        ";
        let schema = target.schema().unwrap_or_default().to_string();
        let object = target.object().to_string();
        let mut conn = self.conn.lock().await;
        let rows: Vec<mysql_async::Row> = conn
            .exec(sql, (schema, object))
            .await
            .map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|mut row| dexo_driver_api::ColumnKeyInfo {
                name: row.take::<String, _>("name").unwrap_or_default(),
                primary_key: row.take::<i64, _>("is_primary").unwrap_or(0) != 0,
                unique: row.take::<i64, _>("is_unique").unwrap_or(0) != 0,
            })
            .collect())
    }
```

`row.take::<i64, _>(...)` is used for the two boolean expressions because MySQL evaluates boolean expressions as `TINYINT`/`BIGINT`, not a native bool — mirror this pattern rather than `row.take::<bool, _>`, which mysql_async does not reliably support for computed boolean columns.

- [ ] **Step 2: Validate**

Run: `cargo build -p dexo-driver-mysql 2>&1 | tail -60`
Expected: clean build. Then run `cargo build --workspace 2>&1 | tail -80` — this should now be fully clean across the workspace (Tasks 1-3 close the trait-implementation gap opened in Task 1).

- [ ] **Step 3: Commit**

```bash
git add crates/dexo-driver-mysql/src/mutation.rs
git commit -m "feat(driver-mysql): implement table_columns via information_schema"
```

---

### Task 4: Fetch column metadata when a table document opens, populate `model.data.table`

**Files:**
- Modify: `crates/dexo-tui/src/action.rs` (new `Effect::LoadTableColumns`, new `Action::TableColumnsLoaded`/`TableColumnsFailed`)
- Modify: `crates/dexo-tui/src/runtime/mod.rs` (dispatch the new effect)
- Modify: `crates/dexo-tui/src/runtime/data_manager.rs` (the async fetch + send-action glue, next to `fetch_page`)
- Modify: `crates/dexo-tui/src/update.rs` (`load_table_document` dispatches the new effect; new `Action::TableColumnsLoaded` handler populates `model.data.table`/`model.data.changes`)

**Interfaces:**
- Consumes: `dexo_driver_api::ColumnKeyInfo` from Task 1.
- Produces: `model.data.table: dexo_app::data::TableMeta` now genuinely reflects the open table's real primary-key/unique columns; `model.data.changes: dexo_app::data::ChangeSet` is rebuilt from it (was previously always `EditMode::ReadOnly` for every real table).

- [ ] **Step 1: Add the effect and actions**

In `crates/dexo-tui/src/action.rs`, next to `LoadTableData`:

```rust
    LoadTableColumns {
        target: dexo_driver_api::QualifiedName,
        session: SessionId,
        generation: u64,
    },
```

Next to `DataPageLoaded`/`DataPageFailed`:

```rust
    TableColumnsLoaded {
        generation: u64,
        columns: Vec<dexo_driver_api::ColumnKeyInfo>,
    },
    TableColumnsFailed {
        generation: u64,
        message: String,
    },
```

- [ ] **Step 2: Dispatch the effect from the runtime**

In `crates/dexo-tui/src/runtime/mod.rs`, next to the `Effect::LoadTableData { .. }` arm:

```rust
            crate::Effect::LoadTableColumns {
                target,
                session,
                generation,
            } => {
                if let Some(active) = self.sessions.get(session) {
                    data_manager::fetch_table_columns(
                        Arc::clone(&active.session),
                        target,
                        generation,
                        self.action_tx.clone(),
                    )
                    .await;
                }
            }
```

In `crates/dexo-tui/src/runtime/data_manager.rs`, next to `fetch_page`:

```rust
pub async fn fetch_table_columns(
    session: Arc<dyn Session>,
    target: dexo_driver_api::QualifiedName,
    generation: u64,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(data) = session.data() else {
        let _ = action_tx
            .send(Action::TableColumnsFailed {
                generation,
                message: "data capability unavailable".into(),
            })
            .await;
        return;
    };
    match data.table_columns(&target).await {
        Ok(columns) => {
            let _ = action_tx
                .send(Action::TableColumnsLoaded { generation, columns })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::TableColumnsFailed {
                    generation,
                    message: error.to_string(),
                })
                .await;
        }
    }
}
```

- [ ] **Step 3: Dispatch from `load_table_document` and handle the response**

In `crates/dexo-tui/src/update.rs`, in `load_table_document` (added by the prior plan), after the existing `match crate::runtime::data_manager::table_request(...) { Ok(request) => { ... vec![Effect::LoadTableData { .. }] } ... }` arm, change it to also push the columns fetch. Replace:

```rust
        Ok(request) => {
            model.documents[index].console_log.push(format!(
                "[{}] {}> SELECT * FROM {} LIMIT {}",
                format_clock(),
                target.display_unquoted(),
                target.display_unquoted(),
                model.data.page_limit
            ));
            vec![Effect::LoadTableData {
                request,
                session,
                generation: model.session_generation,
            }]
        }
```

with:

```rust
        Ok(request) => {
            model.documents[index].console_log.push(format!(
                "[{}] {}> SELECT * FROM {} LIMIT {}",
                format_clock(),
                target.display_unquoted(),
                target.display_unquoted(),
                model.data.page_limit
            ));
            vec![
                Effect::LoadTableData {
                    request,
                    session,
                    generation: model.session_generation,
                },
                Effect::LoadTableColumns {
                    target,
                    session,
                    generation: model.session_generation,
                },
            ]
        }
```

Add the two new action handlers in the big `match action` in `update.rs`, next to `Action::DataPageFailed`:

```rust
        Action::TableColumnsLoaded { generation, columns } => {
            if generation == model.session_generation {
                model.data.table = dexo_app::data::TableMeta {
                    columns: columns
                        .into_iter()
                        .map(|column| dexo_app::data::ColumnDef {
                            name: column.name,
                            primary_key: column.primary_key,
                            unique: column.unique,
                            nullable: true,
                        })
                        .collect(),
                };
                model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
                model.data.row_changes.clear();
            }
            Vec::new()
        }
        Action::TableColumnsFailed { generation, message } => {
            if generation == model.session_generation {
                model.messages.push(message);
            }
            Vec::new()
        }
```

`nullable: true` is a deliberate simplification: `ColumnDef.nullable` is only consulted by `RowIdentity::from_table`'s *fallback* path (unique-and-not-nullable, when there is no primary key at all) — Task 1's query does not fetch nullability, and getting this wrong only ever makes the fallback path *more conservative* (treats a genuinely-not-null unique column as ineligible), never *less safe* (it can never manufacture a false identity column). If the table has a primary key, this field is not consulted at all. Revisit only if a keyless-but-unique-and-not-null table needs to become editable and this proves to be why it isn't.

- [ ] **Step 4: Add the `row_changes` field this task's handler already references**

In `crates/dexo-tui/src/screens/data.rs`, add to `DataScreen` (this is also consumed by Tasks 5-8, but must exist now for Step 3 to compile):

```rust
    pub row_changes: std::collections::BTreeMap<usize, dexo_app::data::RowEditState>,
```

and in `Default for DataScreen`:

```rust
            row_changes: std::collections::BTreeMap::new(),
```

- [ ] **Step 5: Validate**

Run: `cargo build -p dexo-tui 2>&1 | tail -100`
Expected: clean build. Use the `LSP` tool (`hover`/`findReferences`) on `TableColumnsLoaded` and `row_changes` to confirm both new pieces are wired to exactly one producer and one consumer each before moving on — a second, accidental producer here would silently overwrite `model.data.table`.

- [ ] **Step 6: Commit**

```bash
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/runtime/data_manager.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/screens/data.rs
git commit -m "feat(tui): fetch and apply real primary-key metadata when a table opens"
```

---

### Task 5: Row-identity helpers and `GridModel::remove_row`

**Files:**
- Modify: `crates/dexo-tui/src/model.rs` (new `GridModel::remove_row`, next to `append_rows`/`clear`)
- Modify: `crates/dexo-tui/src/update.rs` (new private helpers `row_identity_at`, `row_original_at`, `find_pending_index`)

**Interfaces:**
- Produces: `GridModel::remove_row(&mut self, index: usize)`.
- Produces: `fn row_identity_at(model: &Model, row_index: usize) -> Option<dexo_app::data::RowIdentity>`.
- Produces: `fn row_original_at(model: &Model, row_index: usize) -> Option<Vec<(String, dexo_driver_api::DbValue)>>`.
- Produces: `fn find_pending_index(changes: &dexo_app::data::ChangeSet, predicate: impl Fn(&dexo_app::data::PendingChange) -> bool) -> Option<usize>`.

- [ ] **Step 1: `GridModel::remove_row`**

In `crates/dexo-tui/src/model.rs`, in `impl GridModel`, next to `append_rows`/`clear` (the ones at lines 418-451 today), add a matching method that delegates to `ResultBuffer`:

```rust
    pub fn remove_row(&mut self, index: usize) {
        self.buffer.remove_row(index);
    }
```

In `impl ResultBuffer` (next to its own `append_rows`/`clear`), add:

```rust
    pub fn remove_row(&mut self, index: usize) {
        let storage = Arc::make_mut(&mut self.rows);
        if index >= storage.len() {
            return;
        }
        let removed = storage.remove(index);
        self.estimated_bytes = self
            .estimated_bytes
            .saturating_sub(estimated_row_bytes(&removed));
    }
```

- [ ] **Step 2: Row-identity/original helpers**

In `crates/dexo-tui/src/update.rs`, next to `activate_document`, add:

```rust
fn row_identity_at(model: &Model, row_index: usize) -> Option<dexo_app::data::RowIdentity> {
    let identity_cols = dexo_app::data::RowIdentity::from_table(&model.data.table)?;
    let row = model.results.rows().get(row_index)?;
    let columns = model.results.columns();
    let mut values = Vec::with_capacity(identity_cols.len());
    for name in &identity_cols {
        let position = columns.iter().position(|column| &column.name == name)?;
        values.push(row.get(position)?.clone());
    }
    Some(dexo_app::data::RowIdentity {
        columns: identity_cols,
        values,
    })
}

fn row_original_at(
    model: &Model,
    row_index: usize,
) -> Option<Vec<(String, dexo_driver_api::DbValue)>> {
    let row = model.results.rows().get(row_index)?;
    let columns = model.results.columns();
    Some(
        columns
            .iter()
            .zip(row.iter())
            .map(|(column, value)| (column.name.clone(), value.clone()))
            .collect(),
    )
}

fn find_pending_index(
    changes: &dexo_app::data::ChangeSet,
    predicate: impl Fn(&dexo_app::data::PendingChange) -> bool,
) -> Option<usize> {
    changes.pending().iter().position(predicate)
}
```

- [ ] **Step 3: Validate**

Run: `cargo build -p dexo-tui 2>&1 | tail -80`
Expected: clean build (these helpers have no callers yet, so this only checks they type-check in isolation — `cargo build` still reports unused-function warnings until Task 6 calls them; that's expected and fine, don't silence it with `#[allow(dead_code)]`, Task 6 removes the warning by using them).

- [ ] **Step 4: Commit**

```bash
git add crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs
git commit -m "feat(tui): add row-identity lookup and GridModel::remove_row"
```

---

### Task 6: `Delete` key marks/restores/removes a row, red highlight in the grid

**Files:**
- Modify: `crates/dexo-tui/src/keymap.rs` (`[results]` section, all three profiles: `"delete" = "data.toggle_delete"`)
- Modify: `crates/dexo-tui/src/action.rs` (new `Action::ToggleRowDelete`)
- Modify: `crates/dexo-tui/src/palette/registry.rs` (new command id `data.toggle_delete`)
- Modify: `crates/dexo-tui/src/update.rs` (new `toggle_row_delete` handler)
- Modify: `crates/dexo-tui/src/widgets/grid.rs` (red row style)
- Modify: `crates/dexo-tui/src/theme.rs` (if it does not already expose a role suitable for "pending delete" — check first)

**Interfaces:**
- Consumes: `row_identity_at`, `row_original_at`, `find_pending_index`, `GridModel::remove_row` from Task 5; `model.data.row_changes` from Task 4.
- Produces: pressing `Delete` on a grid row while browsing a table document cycles Clean → Deleted → Clean, or removes a not-yet-applied Inserted row outright.

- [ ] **Step 1: Keybinding**

In `crates/dexo-tui/src/keymap.rs`, add `"delete" = "data.toggle_delete"` to the `[results]` section in `DEFAULT_TOML`, `VIM_TOML`, and `EMACS_TOML` (three occurrences — the same `[results]` block edited by the earlier hide-panel/results work; add this line alongside the existing `"r" = "results.select_row"` line in each).

- [ ] **Step 2: Action and palette entry**

In `crates/dexo-tui/src/action.rs`, next to `Action::RevertChanges`, add:

```rust
    ToggleRowDelete,
```

In `crates/dexo-tui/src/palette/registry.rs`, next to the `"data.revert"` entry (or wherever the `data.*` commands are grouped), add:

```rust
        CommandSpec {
            id: "data.toggle_delete",
            title: "Toggle Row Delete",
            keywords: &["remove", "restore", "row"],
            shortcut: Some("Delete"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::ToggleRowDelete),
        },
```

`requirements: &[]` matches the existing `data.apply`/`data.revert`/`data.review` entries a few lines above this one in the same file (verified: none of them gate on `Requirement::PendingChanges` or anything else today) — use `&[]` here too rather than inventing a requirement. Also add `"data.toggle_delete"` to `crates/dexo-tui/tests/command_palette_contract.rs`'s `COMMAND_IDS` list and bump both hardcoded array-length/`assert_eq!` counts there and in `crates/dexo-tui/src/palette.rs`'s `every_current_action_is_in_palette` test (`+1` each, exactly like the pattern already used for the two `layout.hide_*` commands added earlier).

- [ ] **Step 3: Implement `toggle_row_delete`**

In `crates/dexo-tui/src/update.rs`, dispatch it in the big `match action`:

```rust
        Action::ToggleRowDelete => toggle_row_delete(model),
```

Add the function next to `row_identity_at`:

```rust
fn toggle_row_delete(model: &mut Model) -> Vec<Effect> {
    let Some(row_index) = model.results.cursor_row() else {
        return Vec::new();
    };
    match model.data.row_changes.get(&row_index).copied() {
        Some(dexo_app::data::RowEditState::Deleted) => {
            if let Some(identity) = row_identity_at(model, row_index) {
                if let Some(position) = find_pending_index(&model.data.changes, |change| {
                    matches!(change, dexo_app::data::PendingChange::Delete { identity: existing, .. } if existing == &identity)
                }) {
                    model.data.changes.revert(position);
                }
            }
            model.data.row_changes.remove(&row_index);
        }
        Some(dexo_app::data::RowEditState::Inserted) => {
            let Some(original) = row_original_at(model, row_index) else {
                return Vec::new();
            };
            if let Some(position) = find_pending_index(&model.data.changes, |change| {
                matches!(change, dexo_app::data::PendingChange::Insert { values } if values == &original)
            }) {
                model.data.changes.revert(position);
            }
            model.results.remove_row(row_index);
            model.data.row_changes.remove(&row_index);
            let shifted: std::collections::BTreeMap<usize, dexo_app::data::RowEditState> = model
                .data
                .row_changes
                .iter()
                .map(|(&index, &state)| {
                    if index > row_index {
                        (index - 1, state)
                    } else {
                        (index, state)
                    }
                })
                .collect();
            model.data.row_changes = shifted;
        }
        _ => {
            let Some(identity) = row_identity_at(model, row_index) else {
                model.messages.push(
                    "this table has no primary key or unique column, so rows cannot be deleted"
                        .into(),
                );
                return Vec::new();
            };
            let Some(original) = row_original_at(model, row_index) else {
                return Vec::new();
            };
            model.data.changes.delete(identity, original);
            model
                .data
                .row_changes
                .insert(row_index, dexo_app::data::RowEditState::Deleted);
        }
    }
    Vec::new()
}
```

`model.results.remove_row(row_index)` works through `ResultsState`'s existing `DerefMut<Target = GridModel>` (see `model.rs:353-357`), so no new accessor on `ResultsState` itself is needed.

- [ ] **Step 4: Red row style in the grid**

Check `crates/dexo-tui/src/theme.rs` for an existing `Role` suitable for "destructive/pending delete" (the codebase already has `Role::Error` used for failed transactions elsewhere — reuse that rather than inventing a new role, unless `Role::Error`'s actual rendered color is not red in every theme variant; verify by reading `theme.rs`'s definition of `Role::Error` across the light/dark/no-color branches before deciding).

In `crates/dexo-tui/src/widgets/grid.rs`, in `preview_lines`, change the `row_style` computation:

```rust
        let is_active = cursor_row == Some(row.source_index);
        let is_sel = grid.row_selected(row.source_index);
        let is_pending_delete = matches!(
            model.data.row_changes.get(&row.source_index),
            Some(dexo_app::data::RowEditState::Deleted)
        );
        let row_style = if is_pending_delete {
            model.theme.style(Role::Error, model.capabilities)
        } else if is_active {
            active_style
        } else if is_sel {
            selected_style
        } else {
            model
                .theme
                .zebra(row.source_index % 2 == 1, model.capabilities)
        };
```

Pending-delete takes priority over the active/selected styles so the red is always visible even on the cursor row — this is a deliberate choice (the whole point of the color is "you are about to lose this row"), not an oversight; do not reorder it below `is_active`.

- [ ] **Step 5: Validate**

Run: `cargo build -p dexo-tui 2>&1 | tail -100` then `cargo clippy -p dexo-tui --all-targets 2>&1 | tail -60`
Expected: clean build, no new warnings beyond the pre-existing baseline (`update.rs:270`, `widgets/document_tabs.rs`, `widgets/object_tree.rs:164`, `widgets/row_detail.rs:130`, `widgets/status.rs:108`, `tests/document_tabs_flow.rs:154-155` — noted in the prior plan; if clippy's line numbers for these shift because of your edits, confirm by content, not line number, that they're the same pre-existing findings).

- [ ] **Step 6: Manual verification**

Build the real binary (`cargo build -p dexo --bin dexo`) and, in a tmux session, open a table with a primary key, press `Delete` on a row (confirm it turns red), press `Delete` again (confirm it returns to normal). Kill the tmux session when done.

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-tui/src/keymap.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/widgets/grid.rs crates/dexo-tui/tests/command_palette_contract.rs crates/dexo-tui/src/palette.rs
git commit -m "feat(tui): mark/restore a grid row for deletion with the Delete key"
```

---

### Task 7: `Ctrl+N` insert-row modal

**Files:**
- Create: nothing new — the modal state lives on `DataScreen` (`crates/dexo-tui/src/screens/data.rs`), reusing `crate::screens::schema_editor::FormField` (already a public, standalone `{label, value, secret}` struct used by `ConnectionForm` too).
- Modify: `crates/dexo-tui/src/screens/data.rs` (new `InsertRowForm` struct + `open`/`submit`/`cancel` methods)
- Modify: `crates/dexo-tui/src/keymap.rs` (`[results]`: `"ctrl+n" = "data.insert_row"`, all three profiles — this intentionally shadows the `[global]` `"ctrl+n" = "document.new"` binding only while focus is Results)
- Modify: `crates/dexo-tui/src/action.rs` (`Action::OpenInsertRow`, `Action::SubmitInsertRow`, `Action::CancelInsertRow`)
- Modify: `crates/dexo-tui/src/palette/registry.rs` + `crates/dexo-tui/tests/command_palette_contract.rs` + `crates/dexo-tui/src/palette.rs` (register `data.insert_row`, bump the two hardcoded counts again, same as Task 6 Step 2)
- Modify: `crates/dexo-tui/src/update.rs` (key routing while the modal is open, the three new handlers)
- Modify: `crates/dexo-tui/src/render.rs` (new `render_insert_row_form`)

**Interfaces:**
- Consumes: `dexo_app::data::ColumnDef` (via `model.data.table.columns`) from Task 4; `crate::screens::schema_editor::FormField` (existing type).
- Produces: `DataScreen.insert_form: InsertRowForm { open: bool, fields: Vec<FormField>, focus: usize }`.

- [ ] **Step 1: `InsertRowForm` state**

In `crates/dexo-tui/src/screens/data.rs`, add:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertRowForm {
    pub open: bool,
    pub fields: Vec<crate::screens::schema_editor::FormField>,
    pub focus: usize,
}

impl InsertRowForm {
    pub fn open_for(&mut self, table: &TableMeta) {
        self.open = true;
        self.focus = 0;
        self.fields = table
            .columns
            .iter()
            .map(|column| crate::screens::schema_editor::FormField {
                label: column.name.clone(),
                value: String::new(),
                secret: false,
            })
            .collect();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.fields.clear();
        self.focus = 0;
    }

    pub fn values(&self) -> Vec<(String, DbValue)> {
        self.fields
            .iter()
            .map(|field| {
                let value = if field.value.is_empty() {
                    DbValue::Null
                } else {
                    DbValue::Text(field.value.clone())
                };
                (field.label.clone(), value)
            })
            .collect()
    }
}
```

Add `pub insert_form: InsertRowForm,` to `DataScreen` and `insert_form: InsertRowForm::default(),` to its `Default` impl.

- [ ] **Step 2: Keybinding, actions, palette**

In `crates/dexo-tui/src/keymap.rs`, add `"ctrl+n" = "data.insert_row"` to the `[results]` section in all three profiles.

In `crates/dexo-tui/src/action.rs`, next to `ToggleRowDelete`:

```rust
    OpenInsertRow,
    SubmitInsertRow,
    CancelInsertRow,
```

In `crates/dexo-tui/src/palette/registry.rs`, next to `data.toggle_delete`:

```rust
        CommandSpec {
            id: "data.insert_row",
            title: "Insert Row",
            keywords: &["new", "create", "row"],
            shortcut: Some("Ctrl+N"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::OpenInsertRow),
        },
```

Update the two hardcoded palette-count assertions and the `COMMAND_IDS` list again (Task 6, Step 2's pattern — one more entry this time).

- [ ] **Step 3: Handlers and key routing**

In `crates/dexo-tui/src/update.rs`, in the big `match action`:

```rust
        Action::OpenInsertRow => {
            model.data.insert_form.open_for(&model.data.table);
            Vec::new()
        }
        Action::CancelInsertRow => {
            model.data.insert_form.close();
            Vec::new()
        }
        Action::SubmitInsertRow => submit_insert_row(model),
```

Add `submit_insert_row` next to `toggle_row_delete`:

```rust
fn submit_insert_row(model: &mut Model) -> Vec<Effect> {
    let values = model.data.insert_form.values();
    model.data.insert_form.close();
    if model.data.table.columns.is_empty() {
        return Vec::new();
    }
    model.data.changes.insert(values.clone());
    if !model.data.changes.errors().is_empty() {
        for error in model.data.changes.errors() {
            model.messages.push(error.clone());
        }
        return Vec::new();
    }
    let row_index = model.results.row_count();
    let row_values: Vec<DbValue> = model
        .results
        .columns()
        .iter()
        .map(|column| {
            values
                .iter()
                .find(|(name, _)| name == &column.name)
                .map(|(_, value)| value.clone())
                .unwrap_or(DbValue::Null)
        })
        .collect();
    model.results.append_rows(vec![row_values]);
    model
        .data
        .row_changes
        .insert(row_index, dexo_app::data::RowEditState::Inserted);
    Vec::new()
}
```

Now add key routing. In `handle_key` (the function that already special-cases `model.data.review.is_some()` and `model.help.open` before the normal keymap dispatch — see `update.rs:2820` area), add an equivalent block **before** that `model.data.review.is_some()` check:

```rust
    if model.data.insert_form.open {
        return match key.code {
            KeyCode::Esc => update(model, Action::CancelInsertRow),
            KeyCode::Enter => update(model, Action::SubmitInsertRow),
            KeyCode::Up => {
                model.data.insert_form.focus = model
                    .data
                    .insert_form
                    .focus
                    .checked_sub(1)
                    .unwrap_or(model.data.insert_form.fields.len().saturating_sub(1));
                Vec::new()
            }
            KeyCode::Down | KeyCode::Tab => {
                model.data.insert_form.focus =
                    (model.data.insert_form.focus + 1) % model.data.insert_form.fields.len().max(1);
                Vec::new()
            }
            KeyCode::Backspace => {
                if let Some(field) = model
                    .data
                    .insert_form
                    .fields
                    .get_mut(model.data.insert_form.focus)
                {
                    field.value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
                if let Some(field) = model
                    .data
                    .insert_form
                    .fields
                    .get_mut(model.data.insert_form.focus)
                {
                    field.value.push(ch);
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
```

Place it before the `review.is_some()` check so the two modals can never both intercept keys at once (only one can realistically be open at a time in practice, but this ordering makes that an invariant rather than an accident).

- [ ] **Step 4: Render the modal**

In `crates/dexo-tui/src/render.rs`, add a call in the same place `model.results_menu.open`/`model.data.review.is_some()` (whichever renders last, an overlay) are checked in the top-level `render` function — add:

```rust
    if model.data.insert_form.open {
        render_insert_row_form(frame, model, hits);
    }
```

Add the function itself, modeled on `render_recovery`'s simplicity (a centered popup, no mouse hit registration needed for v1):

```rust
fn render_insert_row_form(frame: &mut Frame, model: &Model, hits: &mut HitMap) {
    let form = &model.data.insert_form;
    let area = frame.area();
    let popup = centered(area, 60, (form.fields.len() as u16 + 4).min(area.height));
    let mut lines = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let marker = if index == form.focus { ">" } else { " " };
        lines.push(format!("{marker} {}: {}", field.label, field.value));
    }
    lines.push(String::new());
    lines.push("Enter submit  Esc cancel".into());
    paint_popup(
        frame,
        popup,
        overlay_block(model, "New row"),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
}
```

- [ ] **Step 5: Validate**

Run: `cargo build -p dexo-tui 2>&1 | tail -120` then `cargo clippy -p dexo-tui --all-targets 2>&1 | tail -60`.
Expected: clean, same baseline-warnings-only bar as Task 6.

- [ ] **Step 6: Manual verification**

In the real binary, open a table, press `Ctrl+N`, type into a couple of fields, `Tab`/`Down` between them, `Enter` to submit — confirm a new row appears at the bottom of the grid. Press `Delete` on that new row and confirm it disappears entirely (not marked red — actually removed) since it was never applied. Kill tmux when done.

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-tui/src/screens/data.rs crates/dexo-tui/src/keymap.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-tui/tests/command_palette_contract.rs crates/dexo-tui/src/palette.rs
git commit -m "feat(tui): add a Ctrl+N modal to insert a new row locally"
```

---

### Task 8: Commit (`Ctrl+S`) and discard-all (`Ctrl+Shift+R`)

**Files:**
- Modify: `crates/dexo-tui/src/keymap.rs` (`[results]`: `"ctrl+s" = "data.review"`, `"ctrl+shift+r" = "data.discard_all"`, all three profiles)
- Modify: `crates/dexo-tui/src/action.rs` (`Action::DiscardAllChanges`)
- Modify: `crates/dexo-tui/src/palette/registry.rs` + the two count-bump files again (this task adds one new command, `data.discard_all` — `data.review` already exists in the registry from before this plan, per the brainstorm research, so only bind its keymap entry, don't re-register it)
- Modify: `crates/dexo-tui/src/update.rs` (`Action::DiscardAllChanges` handler; `Action::MutationsApplied` clears `row_changes` and switches to `load_table_document`)

**Interfaces:**
- Consumes: `Action::OpenReview` (already exists), `model.data.row_changes`/`GridModel::remove_row` from earlier tasks, `load_table_document` from the prior plan.

- [ ] **Step 1: Keybindings**

`"data.review"` is a real, already-registered command id (`registry.rs:347-354`) whose `invocation` is `PaletteInvocation::OpenFlow(FlowIntent::DataReview)`, which `invoke_palette` already maps to `update(model, Action::OpenReview)` (`update.rs:6194`, verified) — a keymap chord can bind to it exactly like any `Dispatch`-invocation command id; no change to the registry entry itself is needed, only the new keymap binding below.

Add to the `[results]` section in all three keymap profiles:

```
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
```

- [ ] **Step 2: `Action::DiscardAllChanges`**

In `crates/dexo-tui/src/action.rs`, next to `RevertChanges`:

```rust
    DiscardAllChanges,
```

Register it in the palette next to the existing `data.revert` entry (`registry.rs:299-306`, `title: "Revert Changes"`, `requirements: &[]`):

```rust
        CommandSpec {
            id: "data.discard_all",
            title: "Discard All Pending Changes",
            keywords: &["revert", "cancel", "rows"],
            shortcut: Some("Ctrl+Shift+R"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::DiscardAllChanges),
        },
```

Bump the two hardcoded palette-count assertions and `COMMAND_IDS` one more time.

- [ ] **Step 3: Implement discard-all**

In `crates/dexo-tui/src/update.rs`:

```rust
        Action::DiscardAllChanges => {
            discard_all_pending(model);
            Vec::new()
        }
```

```rust
fn discard_all_pending(model: &mut Model) {
    let mut inserted_rows: Vec<usize> = model
        .data
        .row_changes
        .iter()
        .filter(|(_, state)| matches!(state, dexo_app::data::RowEditState::Inserted))
        .map(|(&index, _)| index)
        .collect();
    inserted_rows.sort_unstable_by(|a, b| b.cmp(a));
    for index in inserted_rows {
        model.results.remove_row(index);
    }
    model.data.row_changes.clear();
    model.data.changes.discard();
}
```

(Removing highest-index-first means each removal never invalidates a still-pending lower index in the same batch — no reindexing pass needed here, unlike the single-row case in Task 6 which had to shift keys because *other, unrelated* pending rows could sit above it.)

- [ ] **Step 4: Clear `row_changes` after a successful commit, and keep the console log consistent**

In `crates/dexo-tui/src/update.rs`, change the `Action::MutationsApplied` handler from:

```rust
        Action::MutationsApplied {
            generation,
            session,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply();
                return reload_object_data(model);
            }
            Vec::new()
        }
```

to:

```rust
        Action::MutationsApplied {
            generation,
            session,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply();
                model.data.row_changes.clear();
                return load_table_document(model, model.active_document);
            }
            Vec::new()
        }
```

This is safe because `Action::MutationsApplied` can only fire while a table document is active (nothing else dispatches `Effect::ApplyMutations`) — `load_table_document` already no-ops (returns `Vec::new()`) if `model.documents[model.active_document].kind` is not `Table`, same guard it already has from the prior plan.

- [ ] **Step 5: Validate**

Run: `cargo build -p dexo-tui 2>&1 | tail -120` then `cargo clippy -p dexo-tui --all-targets 2>&1 | tail -60`.
Expected: clean, baseline-only warnings.

- [ ] **Step 6: Manual verification (the full loop)**

In the real binary: open a table with a primary key, `Delete` one existing row (turns red), `Ctrl+N` a new row, then `Ctrl+S` — confirm the existing Review modal opens showing both pending operations, `Enter` to apply, confirm the grid refreshes from the database and the deleted row is gone / the inserted row now has whatever the database assigned it. Separately, repeat but press `Ctrl+Shift+R` instead of `Ctrl+S` — confirm both pending operations vanish and the locally-inserted row disappears from the grid while the marked-for-delete row returns to normal (still present, no longer red). Kill tmux when done.

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-tui/src/keymap.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/command_palette_contract.rs crates/dexo-tui/src/palette.rs
git commit -m "feat(tui): commit or discard pending row changes with Ctrl+S / Ctrl+Shift+R"
```

---

## Self-Review Notes

- **Coverage:** Task 1-3 close the "no PK metadata exists" gap found during brainstorming (the actual reason nothing could be edited before this plan). Task 4 wires that metadata into `model.data.table`/`ChangeSet`. Task 5 is the shared row-identity/removal plumbing. Task 6 covers the Delete-key mark/restore/local-remove three-way behavior exactly as the user described it. Task 7 covers the Ctrl+N modal. Task 8 covers commit (reusing the existing, already-working Review modal) and discard-all. Cell editing is explicitly excluded per the approved non-goals.
- **Index-safety is the running theme:** Task 5's design note (never store a `ChangeSet` pending-list index; always re-locate by identity/value) is threaded through Task 6 and Task 8 consistently — neither ever assumes a stored index is still valid.
- **Type consistency:** `dexo_app::data::RowEditState` (only `Deleted`/`Inserted` are ever constructed — `Clean`/`Edited` are never built by this plan, matching the non-goals) is the single per-row vocabulary used in `model.data.row_changes` everywhere from Task 4 through Task 8.
