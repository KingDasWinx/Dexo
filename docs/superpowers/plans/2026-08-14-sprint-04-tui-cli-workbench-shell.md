# Dexo Sprint 04: TUI and CLI Workbench Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar uma TUI responsiva com layout IDE, command palette e grade streaming, sem regressão da CLI.

**Architecture:** A pure `Model + update(Action)` state machine is separated from Ratatui rendering and Crossterm I/O. Database events enter through bounded runtime channels; rendering never performs I/O.

**Tech Stack:** Ratatui 0.30.2, Crossterm 0.29.0, Tokio 1.53 event stream, unicode-width, Insta 1.48 snapshots.

---

## File map

- Create: `crates/dexo-tui/src/{terminal.rs,event.rs,model.rs,update.rs,render.rs,layout.rs,action.rs,palette.rs,lib.rs}`
- Create: `crates/dexo-tui/src/widgets/{mod.rs,status.rs,grid.rs,tabs.rs}`
- Test: `crates/dexo-tui/tests/{snapshots.rs,event_loop.rs}`
- Modify: `crates/dexo/src/main.rs`, `crates/dexo-cli/src/run.rs`

### Task 1: Restore the terminal on success, error and panic

**Files:** `terminal.rs`, `event_loop.rs`.

- [x] **Step 1: Write a failing lifecycle test**

```rust
#[test]
fn terminal_guard_restores_once() {
    let backend = RecordingTerminal::default();
    { let _guard = TerminalGuard::start(backend.clone()).unwrap(); }
    assert_eq!(backend.calls(), vec!["enter", "raw_on", "raw_off", "leave", "cursor_show"]);
}
```

- [x] **Step 2:** Run `cargo test -p dexo-tui terminal_guard_restores_once`; expect unresolved types.

- [x] **Step 3: Implement an idempotent guard**

```rust
pub struct TerminalGuard<B: TerminalControl> { backend: B, restored: bool }
impl<B: TerminalControl> TerminalGuard<B> {
    pub fn start(backend: B) -> Result<Self, TuiError> { backend.enter()?; backend.raw(true)?; Ok(Self { backend, restored: false }) }
    pub fn restore(&mut self) { if !self.restored { let _ = self.backend.raw(false); let _ = self.backend.leave(); let _ = self.backend.show_cursor(); self.restored = true; } }
}
impl<B: TerminalControl> Drop for TerminalGuard<B> { fn drop(&mut self) { self.restore(); } }
```

- [x] **Step 4:** Run `cargo test -p dexo-tui terminal_guard_restores_once`; expect PASS.

- [ ] **Step 5:** Commit with `git commit -m "feat(tui): restore terminal safely"`.

### Task 2: Add deterministic model/update actions

**Files:** `model.rs`, `action.rs`, `update.rs`.

- [x] **Step 1: Write failing reducer test**

```rust
#[test]
fn query_events_do_not_change_editor_focus() {
    let mut model = Model::fixture(Focus::Editor);
    update(&mut model, Action::QueryRows { task: TaskId(uuid::Uuid::nil()), rows: vec![vec![DbValue::I64(1)]] });
    assert_eq!(model.focus, Focus::Editor);
    assert_eq!(model.results.row_count(), 1);
}
```

- [x] **Step 2:** Run `cargo test -p dexo-tui query_events_do_not_change_editor_focus`; expect FAIL.

- [x] **Step 3: Implement `Model`, `Focus`, `Action`, and pure `update`** with actions for key/mouse/resize, connection state, query metadata/rows/messages, transaction state, palette and quit. `update` returns `Vec<Effect>`; it never calls a driver.

```rust
pub enum Effect { StartQuery(QueryRequest), CancelQuery(QueryId), PersistLayout, Quit }
```

- [x] **Step 4:** Run `cargo test -p dexo-tui`; expect reducer tests PASS.

- [ ] **Step 5:** Commit with `git commit -m "feat(tui): add pure application reducer"`.

### Task 3: Render responsive IDE layouts

**Files:** `layout.rs`, `render.rs`, widget status/tabs, snapshots.

- [x] **Step 1: Add snapshots** for 160x50 full layout, 100x30 reduced inspector and 60x20 compact single-panel mode using `ratatui::backend::TestBackend`.

```rust
#[test_case(160, 50, LayoutMode::Full)]
#[test_case(60, 20, LayoutMode::Compact)]
fn layout_matches_terminal(width: u16, height: u16, expected: LayoutMode) {
    assert_eq!(LayoutPlan::for_area(Rect::new(0, 0, width, height)).mode, expected);
}
```

- [x] **Step 2:** Run `cargo insta test -p dexo-tui`; expect new/failing snapshots.

- [x] **Step 3:** Implement breakpoints full >=120x35, reduced >=80x24, compact otherwise; render top context, explorer, tabs, content, results/inspector and status without panics for areas down to 20x8.

- [x] **Step 4:** Review snapshots with `cargo insta review`, accept only expected geometry, then rerun tests.

- [ ] **Step 5:** Commit with `git commit -m "feat(tui): render responsive workbench layouts"`.

### Task 4: Virtualize the streaming result grid

**Files:** `widgets/grid.rs`, `model.rs`, grid tests.

- [x] **Step 1: Write failing viewport test**

```rust
#[test]
fn renders_only_visible_rows() {
    let grid = GridModel::fixture_rows(100_000).with_viewport(50_000, 20);
    let rendered = grid.visible_rows();
    assert_eq!(rendered.len(), 20);
    assert_eq!(rendered[0].source_index, 50_000);
}
```

- [x] **Step 2:** Run targeted test; expect missing grid model.

- [x] **Step 3:** Implement bounded `ResultBuffer` measured by rows and estimated bytes, a `GridViewport { row_offset, column_offset, height, width }`, selection, column widths, horizontal/vertical scroll and truncation markers.

- [x] **Step 4:** Run grid tests and a 100k-row benchmark smoke test; expect no allocation proportional to rendered offscreen rows.

- [ ] **Step 5:** Commit with `git commit -m "feat(tui): virtualize streaming result grid"`.

### Task 5: Implement searchable command palette

**Files:** `palette.rs`, `action.rs`, tests.

- [x] **Step 1: Write failing context test**

```rust
#[test]
fn palette_explains_disabled_commit() {
    let entries = palette_entries(&Model::fixture(TransactionState::Idle));
    let commit = entries.iter().find(|e| e.id == "transaction.commit").unwrap();
    assert_eq!(commit.disabled_reason.as_deref(), Some("no active transaction"));
}
```

- [x] **Step 2:** Run targeted test; expect FAIL.

- [x] **Step 3:** Define stable command IDs, title, keywords, shortcut, availability and action factory. Fuzzy score prefix > word-start > subsequence, with stable alphabetical tie-break.

- [x] **Step 4:** Run palette tests; expect search and disabled explanations PASS.

- [ ] **Step 5:** Commit with `git commit -m "feat(tui): add context-aware command palette"`.

### Task 6: Dispatch TUI only when no CLI command is present

**Files:** `dexo/src/main.rs`, `dexo-cli/src/run.rs`, smoke tests.

- [x] **Step 1: Add failing dispatch tests** asserting `dexo doctor --json` never enters raw mode and `dexo` invokes a fake TUI runner.

- [x] **Step 2:** Run `cargo test -p dexo-cli -p dexo`; expect FAIL.

- [x] **Step 3:** Introduce `LaunchMode::{Tui,Cli(Command)}` from parsed args; inject `TuiRunner` in tests; initialize tracing before dispatch and preserve stdout purity for CLI.

- [x] **Step 4:** Run `cargo test --workspace`; expect PASS without Docker.

- [ ] **Step 5:** Commit with `git commit -m "feat: launch TUI or CLI from one binary"`.

## Sprint exit

- [x] Terminal restoration is tested on every exit path.
- [x] 160x50, 100x30 and 60x20 snapshots are approved.
- [x] Streaming rows appear without moving editor focus.
- [x] Grid rendering cost follows viewport size, not total rows.
- [x] Every current action is available in the command palette.
