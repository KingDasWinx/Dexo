# Table Tab Grid-Primary Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a user opens a table from the Explorer sidebar, it opens in its own document tab (DataGrip-style) where the grid is the big/primary area and a small read-only console log (`[HH:MM:SS] Connected` / `[HH:MM:SS] schema.table> SELECT ...` / `[HH:MM:SS] N rows retrieved starting from M in Xms`) takes the place of the old small Results pane. Running SQL manually from the SQL console tab is completely unchanged.

**Architecture:** `EditorDocument` gains a `kind: DocumentKind` (`Console` | `Table(QualifiedName)`) and its own `console_log: Vec<String>`. The actual row *data* keeps using the existing shared `model.results` grid (deliberately **not** isolated per document in this increment — see Non-goals) so the already-shared, multi-purpose `widgets/grid.rs` renderer needs zero changes to its internals. Activating a Table-kind document (via sidebar open, or switching document tabs) always re-runs its `SELECT * FROM table LIMIT n` fresh against the shared grid, so the visible rows are always correct for whichever document tab is active. `render.rs` swaps *which rect* gets the grid vs. the new small log panel only when the active document is Table-kind and its "Data" sub-tab is selected; every other case (SQL console, DDL/Properties/Explain sub-tabs) renders exactly as it does today.

**Tech Stack:** Rust, ratatui (TUI), existing `dexo-tui` crate conventions (Model/Action/Effect update loop).

**Spec:** None — this plan was scoped and approved section-by-section in chat during brainstorming (no separate spec file was written, per explicit user request). Context worth preserving for future readers: a much larger "database-first workspace" rewrite (typed workspace tabs, per-tab `ChangeSetService`, project/navigator overhaul — see the reverted history on branch `backup-before-plan-revert-20260831-201913`, ~91 commits) was built and then deliberately reverted from `development` because it became unstable. This plan is intentionally scoped far smaller and touches far fewer shared files, per the explicit decision to "restart smaller and controlled."

## Global Constraints

- Do not modify `crates/dexo-tui/src/widgets/grid.rs` internals. Reuse `crate::widgets::grid::render(frame, area, model, hits)` unchanged, just called with a different `Rect`.
- Do not add per-document `GridModel`/`ResultsState` isolation in this increment. `model.results` stays the single shared grid buffer, exactly as today.
- No new crate dependencies (no `chrono`/`time`) — format the console-log clock manually from `std::time::SystemTime`.
- Every task must leave `cargo test -p dexo-tui` fully green and `cargo clippy -p dexo-tui --all-targets` free of new warnings before moving to the next task.
- Follow TDD: write the failing test first, watch it fail, then implement.

---

## Non-goals (explicitly out of scope for this plan)

- Per-document/per-tab isolated grid state (two simultaneously-open table tabs do not each keep their own independent page/filter/sort — switching to a table tab always re-queries it fresh).
- Row edit/delete/insert/commit actions (a separate future plan; the backend for this — `dexo_app::data::ChangeSet` — already exists and is untouched here).
- Export/import wiring to the current SELECT (a separate future plan; `screens/transfer.rs` already exists and is untouched here).
- Execution/fetch timing breakdown in the log line (`execution: Xms, fetching: Yms`) — the driver layer has no such split today; the log shows one total elapsed time only.
- Any change to `docs/superpowers/specs/2026-08-31-workspace-database-first-design.md` or resurrecting anything from the reverted branch.

---

### Task 1: `DocumentKind` and per-document console log

**Files:**
- Modify: `crates/dexo-tui/src/model.rs:979-1064` (the `EditorDocument` struct, its manual `PartialEq` impl, and its `impl EditorDocument` block)
- Test: `crates/dexo-tui/src/model.rs` (the existing `mod editor_document_tests` block at the end of the file, line ~1405)

**Interfaces:**
- Produces: `dexo_tui::model::DocumentKind` enum with variants `Console` and `Table(dexo_driver_api::QualifiedName)`, and `DocumentKind::is_table(&self) -> bool`.
- Produces: `EditorDocument.kind: DocumentKind` (defaults to `Console` for every existing constructor).
- Produces: `EditorDocument.console_log: Vec<String>` (defaults to empty).
- Produces: `EditorDocument::new_table(target: dexo_driver_api::QualifiedName) -> Self`.
- Produces: `EditorDocument::is_dirty(&self) -> bool` now always returns `false` when `self.kind.is_table()`.

- [ ] **Step 1: Write the failing tests**

Add to the `editor_document_tests` module at the bottom of `crates/dexo-tui/src/model.rs`:

```rust
#[test]
fn new_table_document_is_never_dirty() {
    let target = dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "events");
    let doc = EditorDocument::new_table(target.clone());
    assert_eq!(doc.kind, super::DocumentKind::Table(target));
    assert!(!doc.is_dirty());
    assert!(doc.console_log.is_empty());
}

#[test]
fn table_document_title_is_the_bare_object_name() {
    let target = dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "events");
    let doc = EditorDocument::new_table(target);
    assert_eq!(doc.title, "events");
}

#[test]
fn table_document_sql_shows_the_generated_select() {
    let target = dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "events");
    let doc = EditorDocument::new_table(target);
    assert_eq!(doc.text(), "SELECT * FROM db.public.events LIMIT 501");
}

#[test]
fn console_documents_default_to_console_kind() {
    let doc = EditorDocument::scratch();
    assert_eq!(doc.kind, super::DocumentKind::Console);
    assert!(!doc.kind.is_table());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dexo-tui --lib editor_document_tests -- --nocapture`
Expected: compile error — `DocumentKind` and `EditorDocument::new_table` don't exist yet.

- [ ] **Step 3: Implement `DocumentKind` and the new fields**

In `crates/dexo-tui/src/model.rs`, immediately above `pub struct EditorDocument {` (line 979), add:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentKind {
    Console,
    Table(dexo_driver_api::QualifiedName),
}

impl DocumentKind {
    pub fn is_table(&self) -> bool {
        matches!(self, DocumentKind::Table(_))
    }
}
```

Add two fields to the `EditorDocument` struct (after `pub anchor: Option<usize>,`):

```rust
    pub kind: DocumentKind,
    pub console_log: Vec<String>,
```

Update the manual `impl PartialEq for EditorDocument` block to also compare the new fields — change the `&&`-chain to end with:

```rust
            && self.anchor == other.anchor
            && self.kind == other.kind
            && self.console_log == other.console_log
    }
}
```

In `EditorDocument::scratch()`, `EditorDocument::new_unique()`, and `EditorDocument::with_text()`, add to each struct literal:

```rust
            kind: DocumentKind::Console,
            console_log: Vec::new(),
```

Add a new constructor right after `with_text`:

```rust
    pub fn new_table(target: dexo_driver_api::QualifiedName) -> Self {
        let title = target.object().to_string();
        let sql_text = format!("SELECT * FROM {} LIMIT 501", target.display_unquoted());
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            path: None,
            connection_id: None,
            sql: SqlDocument::new(&sql_text),
            saved_revision: 0,
            session: None,
            viewport_line: 0,
            viewport_column: 0,
            typing: false,
            anchor: None,
            kind: DocumentKind::Table(target),
            console_log: Vec::new(),
        }
    }
```

Update `is_dirty`:

```rust
    pub fn is_dirty(&self) -> bool {
        if self.kind.is_table() {
            return false;
        }
        self.sql.revision() != self.saved_revision
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p dexo-tui --lib editor_document_tests`
Expected: PASS (all 4 new tests plus the pre-existing `new_documents_get_unique_ids_and_connection`).

- [ ] **Step 5: Run the full crate test suite and clippy**

Run: `cargo test -p dexo-tui && cargo clippy -p dexo-tui --all-targets`
Expected: everything green — this task only added new, unused-so-far fields/constructors, nothing existing should change behavior.

- [ ] **Step 6: Commit**

```bash
git add crates/dexo-tui/src/model.rs
git commit -m "feat(tui): add a Table document kind with its own console log"
```

---

### Task 2: Opening a table from the sidebar creates (or reuses) its own document tab

**Files:**
- Modify: `crates/dexo-tui/src/screens/data.rs:69-91` (the `DataScreen` struct + its `Default` impl)
- Modify: `crates/dexo-tui/src/update.rs:4253-4452` (`open_selected_table`, `open_object_data`, `reload_object_data`)
- Test: `crates/dexo-tui/tests/data_flow.rs`

**Interfaces:**
- Consumes: `EditorDocument::new_table` and `DocumentKind` from Task 1.
- Produces: `DataScreen.target_document: Option<String>` and `DataScreen.request_started: Option<std::time::Instant>` (both `None` by default).
- Produces: `fn document_index_for_table(model: &Model, target: &dexo_driver_api::QualifiedName) -> Option<usize>` (private, in `update.rs`, next to the existing `document_index_for_path`).
- Produces: `fn load_table_document(model: &mut Model, index: usize) -> Vec<Effect>` (private, in `update.rs`) — later tasks (3 and 4) will call this too.

- [ ] **Step 1: Write the failing test**

Add these imports to the top of `crates/dexo-tui/tests/data_flow.rs` (alongside the existing ones):

```rust
use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_tui::model::Focus;
```

Then add this fixture helper and the two tests. This mirrors the exact, already-working fixture used by `open_object_data_requests_a_table_page` in `crates/dexo-tui/tests/catalog_flow.rs:308-330` (same `replace_roots`/`ObjectId`/`ObjectKind::Table` pattern, and the same `Action::OpenObjectData` — not `ExplorerExpand`, which only adds unrelated expand/collapse branching):

```rust
fn model_ready_to_browse(node_id: &str, qualified: (&str, &str, &str)) -> Model {
    let mut model = Model {
        session_generation: 1,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        focus: Focus::Explorer,
        ..Model::default()
    };
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new(node_id),
            ObjectKind::Table,
            QualifiedName::new(Some(qualified.0), Some(qualified.1), qualified.2),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new(node_id));
    model
}

#[test]
fn opening_a_table_creates_its_own_document_tab() {
    let mut model = model_ready_to_browse("table:events", ("db", "public", "events"));
    let before = model.documents.len();

    let effects = update(&mut model, Action::OpenObjectData);

    assert_eq!(model.documents.len(), before + 1);
    let doc = model.active_document();
    assert!(doc.kind.is_table());
    assert_eq!(model.tabs.active, 1);
    assert!(
        doc.console_log.iter().any(|line| line.contains("Connected")),
        "{:?}",
        doc.console_log
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadTableData { .. }))
    );
}

#[test]
fn reopening_the_same_table_focuses_the_existing_tab_instead_of_duplicating() {
    let mut model = model_ready_to_browse("table:events", ("db", "public", "events"));
    update(&mut model, Action::OpenObjectData);
    let count_after_first_open = model.documents.len();
    let first_id = model.active_document().id.clone();

    model.active_document = 0;
    model.explorer.select(ObjectId::new("table:events"));
    update(&mut model, Action::OpenObjectData);

    assert_eq!(model.documents.len(), count_after_first_open);
    assert_eq!(model.active_document().id, first_id);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui --test data_flow opening_a_table_creates_its_own_document_tab -- --nocapture`
Expected: FAIL — `model.documents.len()` stays the same (today's code reuses the current document), and/or `doc.kind.is_table()` is false.

- [ ] **Step 3: Add the new `DataScreen` fields**

In `crates/dexo-tui/src/screens/data.rs`, add two fields to `DataScreen` (after `pub query_prompt: DataQueryPrompt,`):

```rust
    pub target_document: Option<String>,
    pub request_started: Option<std::time::Instant>,
```

And in its `Default` impl, after `query_prompt: DataQueryPrompt::default(),`:

```rust
            target_document: None,
            request_started: None,
```

- [ ] **Step 4: Add `document_index_for_table` and `load_table_document`, rewrite the table-open flow**

In `crates/dexo-tui/src/update.rs`, next to `document_index_for_path` (around line 5972), add:

```rust
fn document_index_for_table(model: &Model, target: &dexo_driver_api::QualifiedName) -> Option<usize> {
    model.documents.iter().position(|document| {
        matches!(&document.kind, crate::model::DocumentKind::Table(existing) if existing == target)
    })
}
```

Replace the body of `open_object_data` (update.rs:4296-4311) with:

```rust
fn open_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    if model.active_session.is_none() {
        model
            .messages
            .push("connect a session to browse table data".into());
        return Vec::new();
    }
    let target = dexo_app::parse_qualified(&node.qualified);
    let index = match document_index_for_table(model, &target) {
        Some(index) => index,
        None => {
            model
                .documents
                .push(crate::model::EditorDocument::new_table(target));
            model.documents.len() - 1
        }
    };
    model.active_document = index;
    model.tabs.active = 1;
    model.data.last_error = None;
    load_table_document(model, index)
}
```

Add `load_table_document` right after it (this generalizes what `reload_object_data` used to do inline, keyed off an explicit document index instead of implicit "current selection"):

```rust
fn load_table_document(model: &mut Model, index: usize) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    let crate::model::DocumentKind::Table(target) = model.documents[index].kind.clone() else {
        return Vec::new();
    };
    model.data.target = target.clone();
    model.data.loading = true;
    model.data.page_offset = 0;
    model.data.target_document = Some(model.documents[index].id.clone());
    model.data.request_started = Some(std::time::Instant::now());
    if model.documents[index].console_log.is_empty() {
        model.documents[index]
            .console_log
            .push(format!("[{}] Connected", format_clock()));
    }
    match crate::runtime::data_manager::table_request(
        target.clone(),
        Vec::new(),
        model.data.filter.clone(),
        model.data.sort.clone(),
        model.data.page_offset,
        model.data.page_limit,
    ) {
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
        Err(message) => {
            model.messages.push(message);
            Vec::new()
        }
    }
}

fn format_clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}
```

Leave `reload_object_data` (used by pagination/filter/sort actions) untouched for now — Task 3 will make it delegate through `load_table_document` too where relevant, but changing it is not required for this task's test to pass.

Also update `open_selected_table` (update.rs:4253-4271): it currently does `model.focus = Focus::Results;` when it sees a `LoadTableData` effect. Change that single line to keep `Focus::Results` (unchanged — the grid still receives keyboard focus the same way regardless of which Rect it's painted into; this is intentional, see the plan's Architecture section) — **no code change needed here**, only confirm by reading it that it still compiles against the new `open_object_data`. If it does not compile, the only likely fix is that `open_object_data`'s new return type/behavior is already compatible (`Vec<Effect>`), so this step should be a no-op read-through, not an edit.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p dexo-tui --test data_flow opening_a_table_creates_its_own_document_tab reopening_the_same_table_focuses_the_existing_tab_instead_of_duplicating`
Expected: PASS.

- [ ] **Step 6: Run the full suite and fix any regressions**

Run: `cargo test -p dexo-tui`
Expected: green. `open_object_data_requests_a_table_page` in `crates/dexo-tui/tests/catalog_flow.rs:308-330` calls `Action::OpenObjectData` directly today and only asserts a `LoadTableData` effect is present — it does not assert anything about which document is active, so it should keep passing unmodified. If it or any other existing test that calls `Action::OpenObjectData`/`Action::ExplorerExpand` on a *table* node now fails because it assumed the old "reuse current document" behavior, update that test's assertions to match the new document-per-table behavior — do not weaken the new behavior to satisfy an old assumption.

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-tui/src/screens/data.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(tui): open sidebar tables into their own document tab"
```

---

### Task 3: Switching document tabs keeps a Table document's data fresh

**Files:**
- Modify: `crates/dexo-tui/src/update.rs:565-592` (`Action::SelectDocument`, `Action::NextDocument`, `Action::PrevDocument`)
- Test: `crates/dexo-tui/tests/data_flow.rs`

**Interfaces:**
- Consumes: `load_table_document` from Task 2.
- Produces: `fn activate_document(model: &mut Model, index: usize) -> Vec<Effect>` (private, in `update.rs`) — the single place that changes `model.active_document` for these three actions.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn switching_back_to_a_table_document_reruns_its_query() {
    let mut model = model_ready_to_browse("table:events", ("db", "public", "events"));
    update(&mut model, Action::OpenObjectData); // opens "events" as a new document
    let events_index = model.active_document;

    // Open a second, different table so the shared grid/query target changes.
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("table:orders"),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), "orders"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(ObjectId::new("table:orders"));
    update(&mut model, Action::OpenObjectData);
    assert_ne!(model.active_document, events_index);

    let effects = update(&mut model, Action::SelectDocument { index: events_index });

    assert_eq!(model.active_document, events_index);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadTableData { .. })),
        "switching back to the events table tab should re-run its query, got {effects:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui --test data_flow switching_back_to_a_table_document_reruns_its_query -- --nocapture`
Expected: FAIL — today `Action::SelectDocument` returns `Vec::new()`, no `LoadTableData` effect.

- [ ] **Step 3: Implement `activate_document` and use it from the three actions**

In `crates/dexo-tui/src/update.rs`, add near `open_object_data`:

```rust
fn activate_document(model: &mut Model, index: usize) -> Vec<Effect> {
    if index >= model.documents.len() {
        return Vec::new();
    }
    model.active_document = index;
    if model.documents[index].kind.is_table() {
        return load_table_document(model, index);
    }
    Vec::new()
}
```

Replace the three handlers (update.rs:565-591):

```rust
        Action::SelectDocument { index } => {
            if index < model.documents.len() {
                model.document_tab_focus = crate::model::DocumentTabFocus::Document(index);
                model.focus = Focus::Editor;
                let effects = activate_document(model, index);
                model.sync_document_tabs_scroll();
                return effects;
            }
            Vec::new()
        }
        Action::NextDocument => {
            if !model.documents.is_empty() {
                let index = (model.active_document + 1) % model.documents.len();
                let effects = activate_document(model, index);
                model.focus_active_document_tab();
                model.focus = Focus::Editor;
                return effects;
            }
            Vec::new()
        }
        Action::PrevDocument => {
            if !model.documents.is_empty() {
                let index = model
                    .active_document
                    .checked_sub(1)
                    .unwrap_or(model.documents.len() - 1);
                let effects = activate_document(model, index);
                model.focus_active_document_tab();
                model.focus = Focus::Editor;
                return effects;
            }
            Vec::new()
        }
```

Note the `return effects;` inside each `if` — the trailing `Vec::new()` after the `if` block is the empty-documents fallback, unchanged.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p dexo-tui --test data_flow switching_back_to_a_table_document_reruns_its_query`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test -p dexo-tui`
Expected: green. Pay particular attention to any test asserting `Action::SelectDocument`/`NextDocument`/`PrevDocument` return `Vec::new()` for a Console document — those should still pass unchanged since `activate_document` only dispatches an effect for Table-kind documents.

- [ ] **Step 6: Commit**

```bash
git add crates/dexo-tui/src/update.rs crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(tui): re-run a table document's query whenever its tab becomes active"
```

---

### Task 4: Log the "rows retrieved" line when data arrives

**Files:**
- Modify: `crates/dexo-tui/src/update.rs:705-718` (`Action::DataPageLoaded` handler)
- Test: `crates/dexo-tui/tests/data_flow.rs`

**Interfaces:**
- Consumes: `DataScreen.target_document`/`request_started` from Task 2, `format_clock` from Task 2.
- Produces: no new public interface — purely additive behavior on an existing handler.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn data_page_loaded_appends_a_rows_retrieved_log_line_to_its_document() {
    let mut model = model_ready_to_browse("table:events", ("db", "public", "events"));
    update(&mut model, Action::OpenObjectData);
    let index = model.active_document;
    let session = model.active_session.unwrap().0.to_string();
    let generation = model.session_generation;

    update(
        &mut model,
        Action::DataPageLoaded {
            generation,
            session,
            page: dexo_driver_api::DataPage::from_fetched(
                vec![dexo_driver_api::ColumnMeta {
                    name: "id".into(),
                    type_name: "int".into(),
                    nullable: false,
                }],
                vec![vec![DbValue::I64(1)]],
                0,
                501,
            ),
        },
    );

    let log = &model.documents[index].console_log;
    assert!(
        log.iter().any(|line| line.contains("rows retrieved")),
        "{log:?}"
    );
    assert_eq!(model.results.row_count(), 1, "existing shared-grid write must be untouched");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui --test data_flow data_page_loaded_appends_a_rows_retrieved_log_line_to_its_document -- --nocapture`
Expected: FAIL — no "rows retrieved" line is appended anywhere yet.

- [ ] **Step 3: Implement the additive log append**

In `crates/dexo-tui/src/update.rs`, change the `Action::DataPageLoaded` handler (lines 705-718) from:

```rust
        Action::DataPageLoaded {
            generation,
            session,
            page,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply_page(page.clone());
                model.results.clear();
                model.results.set_columns(page.columns.clone());
                model.results.append_rows(page.rows);
                promote_remote_cells(model, &page.columns);
            }
            Vec::new()
        }
```

to:

```rust
        Action::DataPageLoaded {
            generation,
            session,
            page,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply_page(page.clone());
                model.results.clear();
                model.results.set_columns(page.columns.clone());
                let row_count = page.rows.len();
                model.results.append_rows(page.rows);
                promote_remote_cells(model, &page.columns);
                log_rows_retrieved(model, row_count);
            }
            Vec::new()
        }
```

Add the helper next to `format_clock`:

```rust
fn log_rows_retrieved(model: &mut Model, row_count: usize) {
    let Some(document_id) = model.data.target_document.clone() else {
        return;
    };
    let Some(document) = model
        .documents
        .iter_mut()
        .find(|document| document.id == document_id)
    else {
        return;
    };
    let elapsed_ms = model
        .data
        .request_started
        .map(|started| started.elapsed().as_millis())
        .unwrap_or(0);
    let offset = model.data.page_offset;
    document.console_log.push(format!(
        "[{}] {row_count} rows retrieved starting from {} in {elapsed_ms} ms",
        format_clock(),
        offset + 1
    ));
}
```

(`model.data.request_started`/`target_document` are read via `model.data...` before the mutable borrow of `model.documents` is taken, so there is no borrow conflict — call the two `.clone()`/`.map()` reads first, as written above.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p dexo-tui --test data_flow data_page_loaded_appends_a_rows_retrieved_log_line_to_its_document`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test -p dexo-tui`
Expected: green, including `paging_applies_only_matching_generation` (which never sets `target_document`, so `log_rows_retrieved` is a no-op for it — confirm this by reading the test, not just running it).

- [ ] **Step 6: Commit**

```bash
git add crates/dexo-tui/src/update.rs crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(tui): log rows-retrieved timing into the owning table document"
```

---

### Task 5: Render the grid big / console log small for a Table document's Data tab

**Files:**
- Modify: `crates/dexo-tui/src/render.rs:14-61` (`pub fn render`, the non-compact branch)
- Modify: `crates/dexo-tui/src/render.rs:301-360` (`render_editor_content`, `editor_tab_view`, `data_tab_body`)
- Modify: `crates/dexo-tui/src/model.rs:1372-1384` (`sync_grid_viewport`)
- Test: `crates/dexo-tui/src/render.rs` (its own `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `DocumentKind::is_table` (Task 1), `model.active_document()` (existing).
- Produces: `fn render_console_log(frame: &mut Frame, area: Rect, model: &Model)` (private, in `render.rs`).

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block at the bottom of `crates/dexo-tui/src/render.rs` (it already imports `render_to_string` and `Model`):

```rust
#[test]
fn table_document_data_tab_shows_the_grid_big_and_a_console_log_small() {
    let mut model = Model::default();
    model.apply_size(140, 40);
    model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
    model.session_generation = 1;
    model.focus = crate::model::Focus::Explorer;
    model.explorer.replace_roots(dexo_driver_api::CatalogList {
        objects: vec![dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new("table:events"),
            dexo_driver_api::ObjectKind::Table,
            dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "events"),
            None,
        )],
        restrictions: vec![],
    });
    model.explorer.select(dexo_driver_api::ObjectId::new("table:events"));
    crate::update::update(&mut model, crate::action::Action::OpenObjectData);
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![dexo_driver_api::DbValue::I64(1)]]);

    let view = render_to_string(&model, 140, 40);

    assert!(view.contains("Connected"), "{view}");
    assert!(view.contains("id"), "{view}");
}

#[test]
fn console_document_data_tab_is_unchanged_by_the_table_layout() {
    let mut model = Model::default();
    model.apply_size(140, 40);
    model.tabs.active = 1;

    let view = render_to_string(&model, 140, 40);

    assert!(view.contains("Open a table or run a query"), "{view}");
}
```

This mirrors the exact `model_ready_to_browse` fixture from Task 2's tests, just written with `crate::`-relative paths since this test lives inside the `dexo_tui` crate itself (`src/render.rs`) rather than in the external `tests/` integration-test crate.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dexo-tui --lib table_document_data_tab_shows_the_grid_big_and_a_console_log_small -- --nocapture`
Expected: FAIL — today the big area still shows the old text summary, the small area still shows the grid, and neither shows "Connected".

- [ ] **Step 3: Implement the render swap**

In `crates/dexo-tui/src/render.rs`, in `pub fn render` (the non-compact `_ =>` branch, lines 29-61), replace:

```rust
            render_editor_content(frame, plan.content, model, hits);
            if !overlay_blocks_workbench(model) {
                hits.register(HitTarget::Grid, plan.results);
                hits.register(HitTarget::Inspector, plan.inspector);
            }
            crate::widgets::grid::render(frame, plan.results, model, hits);
```

with:

```rust
            let table_data_active =
                model.active_document().kind.is_table() && model.tabs.active == 1;
            if table_data_active {
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Grid, plan.content);
                }
                crate::widgets::grid::render(frame, plan.content, model, hits);
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Inspector, plan.inspector);
                }
                render_console_log(frame, plan.results, model);
            } else {
                render_editor_content(frame, plan.content, model, hits);
                if !overlay_blocks_workbench(model) {
                    hits.register(HitTarget::Grid, plan.results);
                    hits.register(HitTarget::Inspector, plan.inspector);
                }
                crate::widgets::grid::render(frame, plan.results, model, hits);
            }
```

Add `render_console_log` next to `render_editor_content`:

```rust
fn render_console_log(frame: &mut Frame, area: Rect, model: &Model) {
    let log = &model.active_document().console_log;
    let rows = area.height.saturating_sub(2) as usize;
    let scroll = log.len().saturating_sub(rows.max(1)) as u16;
    render_panel_scrolled(
        frame,
        area,
        model,
        "Console",
        false,
        log.join("\n"),
        scroll,
    );
}
```

Now make the SQL sub-tab read-only for a Table document. In `render_editor_content` (render.rs:301-323), change:

```rust
fn render_editor_content(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if model.tabs.active == 0 {
        crate::widgets::editor::render(frame, area, model);
        return;
    }
```

to:

```rust
fn render_editor_content(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    if model.tabs.active == 0 && model.active_document().kind.is_table() {
        render_panel_scrolled(
            frame,
            area,
            model,
            "SQL (read-only)",
            model.focus == Focus::Editor,
            model.active_document().text(),
            0,
        );
        return;
    }
    if model.tabs.active == 0 {
        crate::widgets::editor::render(frame, area, model);
        return;
    }
```

- [ ] **Step 4: Size the grid viewport correctly for the big content area**

In `crates/dexo-tui/src/model.rs`, in `sync_grid_viewport` (lines 1372-1384), change:

```rust
    pub fn sync_grid_viewport(&mut self) {
        let plan = LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, self.width, self.height),
            Some(&self.panes),
            self.tabs.active == 0,
        );
        let width = plan.results.width.saturating_sub(2).max(1);
        let inner_h = plan.results.height.saturating_sub(2).max(1);
        // Match widgets/grid.rs: optional tab row, then one column-header row.
        let tab_h = if self.results.tabs.len() > 1 { 1u16 } else { 0 };
        let height = inner_h.saturating_sub(tab_h).saturating_sub(1).max(1);
        self.results.set_viewport_size(width, height);
    }
```

to:

```rust
    pub fn sync_grid_viewport(&mut self) {
        let plan = LayoutPlan::for_area_with_document_tabs(
            ratatui::layout::Rect::new(0, 0, self.width, self.height),
            Some(&self.panes),
            self.tabs.active == 0,
        );
        let table_data_active = self.active_document().kind.is_table() && self.tabs.active == 1;
        let pane = if table_data_active {
            plan.content
        } else {
            plan.results
        };
        let width = pane.width.saturating_sub(2).max(1);
        let inner_h = pane.height.saturating_sub(2).max(1);
        // Match widgets/grid.rs: optional tab row, then one column-header row.
        let tab_h = if self.results.tabs.len() > 1 { 1u16 } else { 0 };
        let height = inner_h.saturating_sub(tab_h).saturating_sub(1).max(1);
        self.results.set_viewport_size(width, height);
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dexo-tui --lib table_document_data_tab_shows_the_grid_big_and_a_console_log_small console_document_data_tab_is_unchanged_by_the_table_layout`
Expected: PASS.

- [ ] **Step 6: Run the full suite, including snapshot tests**

Run: `cargo test -p dexo-tui`
Expected: green. Pay attention to `tests/snapshots.rs` — if any existing snapshot opens a table and asserts on the old text-summary/small-grid layout, update that assertion to match the new behavior (do not revert the new behavior to keep an old snapshot string passing).

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-tui/src/render.rs crates/dexo-tui/src/model.rs
git commit -m "feat(tui): render the grid big and a console log small for table documents"
```

---

### Task 6: Closing a table tab never prompts to save, and full regression pass

**Files:**
- Test: `crates/dexo-tui/tests/data_flow.rs` (it already has the `model_ready_to_browse` fixture from Task 2 in scope)

**Interfaces:**
- Consumes: everything from Tasks 1-5. No new production code — this task is verification plus a targeted regression test for the one behavior not yet covered by a test (closing).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn closing_a_table_document_never_prompts_to_save() {
    let mut model = model_ready_to_browse("table:events", ("db", "public", "events"));
    update(&mut model, Action::OpenObjectData);
    let count_before_close = model.documents.len();

    let effects = update(&mut model, Action::CloseDocument);

    assert_eq!(model.documents.len(), count_before_close - 1);
    assert!(
        !model
            .messages
            .iter()
            .any(|message| message.contains("Save the untitled document")),
        "{:?}",
        model.messages
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::SaveDocument(_))),
        "{effects:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails or passes for the right reason**

Run: `cargo test -p dexo-tui closing_a_table_document_never_prompts_to_save -- --nocapture`
Expected: this should already PASS given Task 1's `is_dirty` guard (closing a never-dirty document takes the `remove_document` path directly, per `close_active_document`'s existing logic at update.rs:5009-5034). If it fails, the bug is that something is still marking the Table document dirty (e.g. its `SqlDocument` revision advancing on creation) — fix `EditorDocument::new_table` or `is_dirty`, don't weaken the test.

- [ ] **Step 3: Run the entire workspace-relevant test suite**

Run:
```bash
cargo fmt -p dexo-tui -- --check
cargo clippy -p dexo-tui --all-targets
cargo test -p dexo-tui
```
Expected: all green, zero new clippy warnings (compare the warning list against the baseline noted in prior work in this repo — `update.rs:270`, `widgets/document_tabs.rs` (several), `widgets/object_tree.rs:164`, `widgets/row_detail.rs:130`, `widgets/status.rs:108`, `tests/document_tabs_flow.rs:154-155` are pre-existing and not this plan's concern).

- [ ] **Step 4: Manually verify in the real binary**

```bash
cargo build -p dexo --bin dexo
```

Launch it in a tmux session (same technique used earlier in this project: `tmux new-session -d -s dexo_verify -x 140 -y 40 "./target/debug/dexo"`), connect to a real or fixture connection, open a table from the sidebar, and confirm with `tmux capture-pane -p`:
- the grid is big (in the old SQL-editor area) and shows real rows,
- the small pane below shows `Connected` / the generated `SELECT` / `N rows retrieved...`,
- switching to another document tab and back re-runs the query (log gets a fresh set of lines),
- opening the same table twice does not create a second tab,
- `Ctrl+W` closes the table tab with no save prompt,
- running SQL manually from the console tab (`tabs.active == 0` on a `Console` document) looks exactly as it did before this plan.

Kill the tmux session when done.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test(tui): verify closing a table document skips the unsaved-changes prompt"
```

---

## Self-Review Notes

- **Coverage:** Task 1 covers the data model; Task 2 covers sidebar-open creating/reusing a tab; Task 3 covers tab-switch freshness; Task 4 covers the "rows retrieved" log line; Task 5 covers the actual visual swap (the user's core ask) plus the read-only SQL tab; Task 6 covers the close-without-prompt behavior and full regression/manual verification. Every piece of the chat-approved design (sections 1–3 discussed with the user) has a task.
- **Non-goals are explicit** so a future reader (or the user) doesn't mistake the shared-grid simplification for an oversight — it was a deliberate, user-approved risk reduction after discovering `widgets/grid.rs`'s coupling to `model.results`.
- **Type consistency:** `DocumentKind::Table(QualifiedName)` (Task 1) is the single representation used everywhere later (`document_index_for_table`, `load_table_document`, `sync_grid_viewport`, `render.rs`) — no task introduces a competing `schema`/`table` string pair.
