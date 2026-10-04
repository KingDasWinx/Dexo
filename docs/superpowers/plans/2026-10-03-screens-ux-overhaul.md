# Screens UX Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Connections, Agents, Server, Compare and History usable without the docs:
actions as buttons where they apply, data as fields instead of prose, filters and search
on every list, a global history that records failures, and a Server with more than
sessions.

**Architecture:** Shared building blocks go in `crate::screen` (`mod.rs` plus a new
`widgets.rs`):
- a `Button` type and an action bar, whose clicks feed the same key path as the keyboard
  (`HitTarget::Press`);
- a toolbar of chips, search and buttons;
- a fields/sections renderer and a highlighted-SQL renderer.

Each screen's state struct gains its filters. Each screen's `render`/`hints` uses the
building blocks, and its key handler in `update.rs` gains the new actions. History gets a
storage migration. Server gets new runtime effects over `AdministrationProvider`.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.29, rusqlite (dexo-storage), tokio
runtime effects.

**Spec:** `docs/superpowers/specs/2026-10-03-screens-ux-overhaul-design.md`

## Global Constraints

- TUI, CLI and MCP go through dexo-app; drivers never import UI crates.
- No work on the loop that draws the screen: database and network reads go through
  `queue_read`; Dexo's own SQLite goes through `off_the_loop`; storage requests are
  spawned (see `runtime/mod.rs`).
- Secrets never go in SQLite, TOML, argv, logs or panic reports. Copy URL leaves the
  password out.
- Never touch the docker container `pg-commerce-lab`. Any QA run of the binary exports
  `DEXO_DATA_HOME`, `HOME` and `XDG_CONFIG_HOME` to a scratch folder.
- Colour never carries meaning alone: every glyph is paired with a word or letter.
- Code, commits and docs are in English; one commit per change; `cargo fmt --all` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings` stay clean.
- The `.claude/` folder and `docs/superpowers/plans/2026-09-23-mcp-hardening-and-rewrite.md`
  are never committed.

## Review Focus

1. **Typing into a search box** must not trigger the screen's letter actions: typing `x`
   while searching must not delete. *Test:* each screen's search task types its action
   letters into the search.
2. **A click on a button and its key do the same thing,** including a dimmed button,
   which says why instead of acting. *Test in Task 1.*
3. **Old databases:** history rows written before the migration read as ok with empty
   metadata, and the migration runs on a database that already has thousands of rows.
   *Test in Task 6.*
4. **Narrow terminals (80x24, 60 columns):** the action bar stays visible and the toolbar
   never pushes the list off screen. *Test in Task 15.*
5. **Filters that hide the picked row:** the pick moves to the first visible row and the
   detail follows; with nothing shown, the empty "Nothing matches" state appears. *Test
   in Task 2 (Connections) and Task 7 (History).*

---

## File map

| File | Responsibility |
| --- | --- |
| `crates/dexo-tui/src/screen/widgets.rs` (new) | `Button`, `action_bar`, `toolbar`, `Chip`, `fields`, `sql_lines`, `status glyph` helpers |
| `crates/dexo-tui/src/screen/mod.rs` | re-exports; `buttons(model)` per screen; `press_key`; detail button focus |
| `crates/dexo-tui/src/mouse.rs` | `HitTarget::Press(KeyCode, bool)` (bool = shift) |
| `crates/dexo-tui/src/model.rs` | `screen_button: usize` (focused button in the detail) |
| `crates/dexo-tui/src/screens/connections.rs` | search, filters, grouping, table rows, fields, test line |
| `crates/dexo-tui/src/screen/connections.rs` | render with toolbar, table, action bar, fields |
| `crates/dexo-storage/src/history.rs`, `migrations.rs` | outcome/duration/rows/error/database columns, delete, entries_all |
| `crates/dexo-tui/src/screen/history.rs`, `screens/editor.rs` (history state) | global history, filters, grouping, actions |
| `crates/dexo-tui/src/screen/agents.rs`, `screens/mcp_setup.rs`, `screens/mcp_profiles.rs`, `screens/mcp_audit.rs` | Setup brevity, detection, profile editing, filters |
| `crates/dexo-app/src/mcp/clients.rs` | `McpClient::installed` |
| `crates/dexo-driver-api/src/admin.rs` + postgres/mysql `admin.rs` | `SessionInfo { application, client }` |
| `crates/dexo-tui/src/screen/server.rs`, `screens/admin.rs` | views, filters, sort, cancel, you |
| `crates/dexo-tui/src/screen/compare.rs`, `screens/schema_diff.rs` | sources toolbar, swap, grouped list, chips |
| `crates/dexo-tui/src/update.rs`, `runtime/mod.rs`, `action.rs` | actions, effects |
| `docs/src/workbench.md`, `docs/src/mcp.md` | docs |

---

### Task 1: Buttons and the action bar

**Files:**
- Create: `crates/dexo-tui/src/screen/widgets.rs`
- Modify: `crates/dexo-tui/src/screen/mod.rs`, `crates/dexo-tui/src/mouse.rs`, `crates/dexo-tui/src/model.rs`, `crates/dexo-tui/src/update.rs` (`handle_screen_key`, `mouse_screen`)
- Test: `crates/dexo-tui/tests/screen_buttons.rs`

**Interfaces:**
- Produces:
  - `pub struct Button { pub key: KeyCode, pub shift: bool, pub label: String, pub enabled: Result<(), String> }`,
    with constructors `Button::new(key, label)` and `.shifted()` / `.disabled(why)`.
  - `pub fn action_bar(frame, area: Rect, model, hits, buttons: &[Button], focused: Option<usize>) -> u16`,
    which returns the rows used.
  - `HitTarget::Press(KeyCode, bool)`.
  - `pub fn buttons(model: &Model) -> Vec<Button>` in `screen/mod.rs`, dispatched per
    screen; each screen module gets `pub fn buttons(model) -> Vec<Button>`.
  - `Model::screen_button: usize`.
  - In `update.rs`, `fn screen_own_key(model, key) -> Vec<Effect>`: the per-screen
    handler followed by the keymap fallback, which `handle_screen_key` used to inline.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/screen_buttons.rs
//! A screen's actions are buttons: a click and the key printed in it do the same, Left
//! and Right walk them from the detail and Enter presses one, a dimmed one says why.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::{Action, Effect, Model, update};

fn paint(model: &mut Model) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| dexo_tui::render::render(frame, model, &mut hits)).unwrap();
    model.hits = hits;
}

fn connections() -> Model {
    let mut model = Model::default();
    model.connections.load_profiles(vec![dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::nil()), None, "pg-dev", "postgres", "development",
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        dexo_app::SecretRef::new("r".into()),
    )]);
    update(&mut model, Action::GoToScreen(dexo_tui::model::Screen::Connections));
    paint(&mut model);
    model
}

fn click(model: &mut Model, target: HitTarget) -> Vec<Effect> {
    let (column, row) = model.hits.center(target);
    assert_ne!((column, row), (0, 0), "{target:?} is not on screen");
    update(model, Action::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE,
    }))
}

#[test]
fn a_click_on_edit_opens_the_form_as_e_does() {
    let mut model = connections();
    click(&mut model, HitTarget::Press(KeyCode::Char('e'), false));
    assert!(model.connection_form.open);
}

#[test]
fn left_right_and_enter_press_the_detail_buttons() {
    let mut model = connections();
    update(&mut model, Action::Key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT)));
    paint(&mut model);
    let buttons = dexo_tui::screen::buttons(&model);
    let edit = buttons.iter().position(|b| b.key == KeyCode::Char('e')).unwrap();
    for _ in 0..edit {
        update(&mut model, Action::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)));
    }
    assert_eq!(model.screen_button, edit);
    update(&mut model, Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(model.connection_form.open);
}

#[test]
fn a_dimmed_button_says_why() {
    let mut model = connections();
    // `c Disconnect` on a connection that is not open.
    let effects = click(&mut model, HitTarget::Press(KeyCode::Char('c'), false));
    assert!(effects.is_empty());
    assert!(model.messages.iter().any(|m| m.message.contains("not connected")));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dexo-tui --test screen_buttons`
Expected: compile errors (`HitTarget::Press`, `screen::buttons`, `screen_button`).

- [ ] **Step 3: Implement**

`screen/widgets.rs`:
- `Button`.
- `key_label(code, shift) -> String`: `⏎` for Enter, `F2`, the letter in its case.
- `action_bar`: draws `[k Label]` chips left to right with the key in the
  `Role::Focus` style and the label in the foreground colour. A disabled button is all
  `Role::Muted`. The focused one is reversed and starts with `>`. It wraps to the next
  row when the next chip does not fit, never splitting a chip, and registers
  `HitTarget::Press(key, shift)` on each chip's cells. It returns the rows used.

`mouse_screen`: `Some(HitTarget::Press(code, shift))` → build
`KeyEvent::new(code, if shift { SHIFT } else { NONE })` and call `screen_press(model, key)`.

`screen_press(model, key)` looks the key up among `screen::buttons(model)`:
- a disabled button → `model.messages.info(why)`, and return no effects;
- otherwise → `screen_own_key(model, key)`.

`handle_screen_key`, when the section is Detail, the detail is not held and is not a
form, and the key has no modifiers:
- Left/Right move `model.screen_button` (clamped to `buttons(model).len()`);
- Enter → `screen_press` with the focused button's key.

Reset `screen_button` to 0 on a section change (`focus_section`), a pick change, and
`go_to_screen`.

Connections gets its `buttons(model)` in this task: Connect/Use/Open SQL, New SQL,
Browse, Disconnect, Edit, Duplicate, Test, Copy URL and Delete. Disconnect is disabled
when there is no session ("pg-dev is not connected."). Render the action bar at the top
of the detail pane and remove the `HINTS` footer lines and their `HitButton`
registrations.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p dexo-tui --test screen_buttons` → PASS; then `cargo test -p dexo-tui` → all PASS. Update snapshots only where the old footer row moved into the action bar.

- [ ] **Step 5: Commit** — `feat(tui): a screen's actions are buttons, pressed by a click, their key, or Left/Right and Enter from the detail`

### Task 2: Toolbar, search and Connections' filters

**Files:**
- Modify: `screen/widgets.rs`, `screen/connections.rs`, `screens/connections.rs`, `update.rs` (`connections_key`)
- Test: `crates/dexo-tui/tests/connections_screen.rs`

**Interfaces:**
- Produces:
  - `pub struct Chip { pub key: KeyCode, pub shift: bool, pub label: String, pub active: bool }`.
  - `pub fn toolbar(frame, area: Rect, model, hits, search: Option<(&TextInput, bool /*editing*/)>, chips: &[Chip], buttons: &[Button]) -> Rect`,
    which returns the rest of `area`.
  - `ConnectionsScreen { search: TextInput, searching: bool, connected_only: bool, env: Option<String> }`
    and `ConnectionsScreen::visible(&self) -> Vec<usize>`, the profile indices shown in
    order: grouped, then ungrouped, then Docker.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn search_narrows_the_list_and_typing_names_no_action() {
    let mut model = four_connections(); // pg-dev, pg-prod, my-dev, shop
    press(&mut model, KeyCode::Char('/'));
    for ch in "xprod".chars() { press(&mut model, KeyCode::Char(ch)); } // `x` would delete
    assert!(model.connections.delete_target.is_none());
    press(&mut model, KeyCode::Backspace); // drop the first letter? no: Home + Delete
    // ...
}
```

The full test set:
- **`/` then typing:** the search gets `prod`. Only `pg-prod` is visible, and the pick
  moves to it. `x` typed in the search is text: `delete_target` stays `None`.
- **Esc:** clears the search, then the filters, then goes back.
- **`o`:** shows only the connected; with none, the list shows "Nothing matches the
  filters." and an `[Esc Clear filters]` button.
- **`v`:** cycles Env through all → prod → staging → dev → local, and the chip's label
  follows.
- **Groups:** a group heading `▾ team (2)` folds with Left and unfolds with Right on the
  heading.

- [ ] **Step 2: Run** — `cargo test -p dexo-tui --test connections_screen` → FAIL (fields missing).
- [ ] **Step 3: Implement**
  - `toolbar`: search box on the left (`/ search` placeholder, cursor when editing), then
    chips (accent when active), and buttons flush right. Each chip and button registers
    `HitTarget::Press`. When everything does not fit, buttons drop first (they stay in
    the status line hints), then chips fold into one `Filters (n)` chip that cycles them.
  - `visible()`: matches the search, smart-case, over name, host, database, group and
    driver; applies the `connected_only` and `env` filters; then orders by group (named
    groups first, alphabetically) and keeps the Docker rows after.
  - Group folding uses `collapsed: HashSet<String>`.
  - `connections_key`:
    - `/` starts searching. While searching, `TextInput` owns the keys; Enter or Esc
      stops; Esc with empty text clears.
    - `o` toggles `connected_only`, `v` cycles `env`.
    - Up/Down move through `visible()`.
- [ ] **Step 4: Run** → PASS; full `cargo test -p dexo-tui`.
- [ ] **Step 5: Commit** — `feat(tui): Connections searches and filters its list, by environment and by connection`

### Task 3: Connections' table and fields

**Files:**
- Modify: `screens/connections.rs` (`rows` → table rows, `detail_lines` → `detail_fields`), `screen/connections.rs`, `screen/widgets.rs` (`fields`)
- Test: `tests/connections_screen.rs`

**Interfaces:**
- Produces:
  - `pub enum FieldRow { Field(&'static str, String), Section(&'static str), Blank }` in
    `screen/widgets.rs`.
  - `pub fn fields(frame, area, model, rows: &[FieldRow], scroll: usize) -> (usize /*max_scroll*/, u16 /*page*/)`.
  - `ConnectionsScreen::detail_fields(&self, active) -> Vec<FieldRow>`.
  - `pub fn env_word(environment: &str) -> (&'static str, Role)`, where `production` maps
    to (`prod`, `Role::Production`) and so on.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_list_is_a_table_with_status_driver_env_and_address() {
    let model = four_connections();
    let frame = dexo_tui::render::render_to_string(&model, 140, 40);
    assert!(frame.contains("NAME"), "{frame}");
    assert!(frame.contains("○ pg-prod"), "{frame}");
    assert!(frame.contains("PostgreSQL"), "{frame}");
    assert!(frame.contains("prod "), "{frame}");
    assert!(!frame.contains("offline"), "a word on every row marks nothing: {frame}");
}

#[test]
fn the_detail_is_fields_not_prose() {
    let model = four_connections();
    let frame = dexo_tui::render::render_to_string(&model, 140, 40);
    for label in ["Driver", "Address", "Database", "User", "Password"] {
        assert!(frame.contains(label), "{label}: {frame}");
    }
}
```

- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**
  - Table columns: glyph (`◉` in use, `●` connected, `○` offline), NAME, DRIVER,
    ENV, ADDRESS (`host[:port]/db` or the file path relative to `~`).
  - Column priority when narrow: ADDRESS drops first, then DRIVER.
  - The header row is drawn via `list_pane`'s header.
  - The detail fields follow the spec's Connections section. The `password_command`
    path is shown relative to `~`, cut in the middle when long.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): Connections lists a table and shows a connection as fields`

### Task 4: From URL

**Files:** `update.rs`, `screens/connections.rs`, `screen/connections.rs`, `screens/connection.rs` (form `URL` field)
**Test:** `tests/connections_screen.rs`

**Interfaces:**
- Produces:
  - `ConnectionsScreen::url_prompt: Option<TextInput>`.
  - `ConnectionForm::fill_from_url(&mut self, parsed: &dexo_app::connection_url::UrlConnection)`.

- [ ] **Step 1: Failing test**

```rust
#[test]
fn a_url_fills_the_form() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    for ch in "postgres://ana:s3cret@db.local:5433/shop".chars() { press(&mut model, KeyCode::Char(ch)); }
    press(&mut model, KeyCode::Enter);
    assert!(model.connection_form.open);
    let value = |label: &str| model.connection_form.fields.iter().find(|f| f.label == label).unwrap().value.as_str().to_string();
    assert_eq!(value("host"), "db.local");
    assert_eq!(value("port"), "5433");
    assert_eq!(value("database"), "shop");
    assert_eq!(value("username"), "ana");
    assert_eq!(value("password"), "s3cret");
}

#[test]
fn a_bad_url_stays_with_its_reason() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    for ch in "nonsense".chars() { press(&mut model, KeyCode::Char(ch)); }
    press(&mut model, KeyCode::Enter);
    assert!(!model.connection_form.open);
    assert!(model.connections.url_error.is_some());
}
```

- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**
  - The prompt is a one-line input drawn as the detail pane's first rows. `u` opens it,
    Enter parses, Esc closes.
  - On a parse error, `url_error` holds the parser's message.
  - `fill_from_url` sets driver, host, port, database, username and password, then calls
    `sync_descriptor_fields`.
  - The form also gets a top `url` field whose Enter or Down fills the rest.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): a connection is added from a URL pasted on Connections or into the form`

### Task 5: Connections' new actions and the live test line

**Files:** `update.rs`, `screens/connections.rs`, `action.rs` (no new effects: reuse `ConnectProfile`, `CopyToClipboard`, `NewDocument`, `CloseSelectedSession`, `TestSavedProfile`)
**Test:** `tests/connections_screen.rs`

**Interfaces:**
- Produces:
  - `ConnectionsScreen::test: Option<TestLine>`, where
    `enum TestLine { Running(Instant), Passed(u128 /*ms*/), Failed(String) }`, keyed to
    the profile id.
  - `pub fn url_of(profile: &ConnectionProfile) -> String`, without a password.

- [ ] **Step 1: Failing tests**
  - **`s` (New SQL) on pg-dev:** a document on pg-dev is active and the screen is the
    workbench. When pg-dev is offline, `ConnectProfile` is among the effects.
  - **`y`:** emits `CopyToClipboard { text: "postgres://u@h:5432/d" }` with no password
    in it.
  - **`b`:** goes to the workbench with `explorer.selected_connection_name() == Some("pg-dev")`.
  - **`t`:** sets `TestLine::Running`; `Action::ConnectionTested { ok: true, .. }` turns it
    into `Passed`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** the four actions in `connections_key` and the test line in
  the fields (`── Last test`). Time the test in the model: it starts at `t` and ends when
  the runtime answers. The line is redrawn by the existing running tick while
  `Running`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): Connections opens SQL on a connection, browses it, copies its URL and shows its test as it runs`

### Task 6: History records outcome, duration, rows and errors

**Files:**
- Modify: `crates/dexo-storage/src/migrations.rs` (new migration), `crates/dexo-storage/src/history.rs`, `crates/dexo-tui/src/runtime/storage_worker.rs`, `crates/dexo-tui/src/action.rs` (`PersistHistoryRequest`), `crates/dexo-tui/src/update.rs` (record per statement on finish and on failure)
- Test: `crates/dexo-storage/src/history.rs` tests, `crates/dexo-tui/tests/history_recording.rs`

**Interfaces:**
- Produces:
  - `HistoryRow { id, sql, connection_id, created_at, outcome: HistoryOutcome, duration_ms: Option<u64>, rows: Option<u64>, error: Option<String>, database: Option<String> }`.
  - `pub enum HistoryOutcome { Ok, Failed, Cancelled }`.
  - `HistoryRepository::record(&self, entry: &NewHistoryEntry) -> Result<()>`.
  - `entries_all(limit: usize) -> Vec<HistoryRow>`, `delete(ids: &[String])`.
  - `PersistHistoryRequest` gains `outcome`, `duration_ms`, `rows`, `error` and `database`.

- [ ] **Step 1: Failing tests**

```rust
// dexo-storage history tests
#[test]
fn a_failed_run_is_kept_with_its_error_and_old_rows_read_as_ok() {
    let db = Database::open_in_memory().unwrap();
    db.connection().execute(
        "INSERT INTO sql_history (id, connection_id, sql, created_at) VALUES ('old', 'pg', 'select 1', '2026-01-01 00:00:00')", []).unwrap();
    let repo = HistoryRepository::new(db.connection());
    repo.record(&NewHistoryEntry { id: "new".into(), project_id: None, connection_id: Some("pg".into()),
        sql: "select * from nope".into(), outcome: HistoryOutcome::Failed, duration_ms: Some(2),
        rows: None, error: Some("relation \"nope\" does not exist".into()), database: Some("qa0".into()) }).unwrap();
    let rows = repo.entries_all(100).unwrap();
    assert_eq!(rows[0].outcome, HistoryOutcome::Failed);
    assert_eq!(rows[0].error.as_deref(), Some("relation \"nope\" does not exist"));
    assert_eq!(rows[1].outcome, HistoryOutcome::Ok);
    assert_eq!(rows[1].duration_ms, None);
    repo.delete(&["old".to_string()]).unwrap();
    assert_eq!(repo.entries_all(100).unwrap().len(), 1);
}
```

In the TUI (`history_recording.rs`), feed `QueryResultSetFinished` and then
`ScriptFinished` for a two-statement script whose second statement fails (a
`QueryFailed` with index 1). The emitted `Effect::PersistHistory` requests must be two:
ok with rows for the first, failed with the error for the second.

- [ ] **Step 2: Run** — `cargo test -p dexo-storage history` and `cargo test -p dexo-tui --test history_recording` → FAIL.
- [ ] **Step 3: Implement**
  - The migration: `ALTER TABLE sql_history ADD COLUMN outcome TEXT; ... duration_ms INTEGER; row_count INTEGER; error TEXT; database TEXT;`.
  - `entries_all` reads `COALESCE(outcome,'ok')`.
  - The TUI records each statement of the run at its end:
    - SQL: the statement's text from the run's `ScriptRequest` statements, kept on the
      model as `model.run_statements: Vec<String>` keyed by operation.
    - Duration: the time between its start and its finish, measured with an `Instant`
      kept per result tab.
    - Rows: the rows the grid holds for it, or `rows_affected`.
    - Database: `model.schema` or the connection's database.
    - On `QueryFailed`: `Failed` with the message; when `cancelled`, `Cancelled`.
- [ ] **Step 4: Run** → PASS, then `cargo test --workspace`.
- [ ] **Step 5: Commit** — `feat(history): every statement run is kept with its outcome, time, rows, error and database`

### Task 7: History is global, filtered, grouped by day, and shows SQL in colour

**Files:**
- Modify: `screens/editor.rs` (history state: `history: Vec<HistoryRow>`, filters), `screen/history.rs`, `update.rs` (`history_screen_key`, `LoadHistory` without a connection), `runtime/storage_worker.rs` (`list_history` → `entries_all`), `screen/widgets.rs` (`sql_lines`), `widgets/editor.rs` (`pub(crate) fn highlighted(sql, dialect) -> Vec<Line<'static>>` reusing `highlight_spans`)
- Test: `crates/dexo-tui/tests/history_screen.rs`

**Interfaces:**
- Produces:
  - `HistoryFilter { connection: Option<String>, status: StatusFilter, search: TextInput }`.
  - `EditorState::history_rows(&self) -> Vec<HistoryLine>`, deduped by (connection, sql),
    holding the last run and `runs: usize`.
  - `pub fn sql_lines(sql: &str, width: usize, dialect: Dialect) -> Vec<Line<'static>>`,
    which wraps by width and keeps the colours.

- [ ] **Step 1: Failing tests**
  - **No connection open:** `HistoryLoaded` with rows from pg-dev, my-dev and shop shows
    all three.
  - **Connection filter:** `c` cycles to my-dev and only my-dev's rows show.
  - **Status filter:** `f` cycles Status to failed and only ✗ rows show.
  - **Search:** typing `count` keeps the count statement. Typing `x` while searching
    deletes nothing.
  - **Day headings:** a row from yesterday shows under `Yesterday`.
  - **Detail:** the run's fields are visible: Connection, When, Took, Rows, Result, Runs.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**
  - Entering History loads with `connection_id: None`. The search box is the toolbar's.
  - The list is rows with group headings as non-pickable rows, using `list_pane`'s
    `(Option<usize>, String)` rows the way Connections does.
  - The detail draws `sql_lines` and then `fields`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): History shows every connection's statements, filtered by connection, status and text, by day`

### Task 8: History's actions and the Saved view's toolbar

**Files:** `update.rs`, `screen/history.rs`, `runtime/storage_worker.rs` (`DeleteHistory { ids }`, `ClearHistoryMatching { ids }`), `action.rs`
**Test:** `tests/history_screen.rs`

- [ ] **Step 1: Failing tests**
  - **`⏎`:** opens a document on the row's connection with its SQL.
  - **`r`:** also returns a run effect (`StartScript`), connecting first when needed.
  - **`y`:** copies the SQL.
  - **`s`:** opens the save-query prompt with the SQL.
  - **`x`:** emits `DeleteHistory` with the row's run ids and drops them from the list.
  - **`C`:** asks "Clear 3 statements?" and, on Confirm, emits the ids shown.
  - **Saved view:** has search and a Connection chip, and `⏎/r/y/F2/x` work.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**, reusing `open_saved_query`'s document creation for Open and Run.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): History opens, runs again, copies, saves, deletes and clears what it shows`

### Task 9: Agents' Setup in fields, with detection

**Files:** `crates/dexo-app/src/mcp/clients.rs` (`installed`), `runtime/mod.rs` (`mcp_clients` fills `found`), `screens/mcp_setup.rs` (`ClientRow.found`, `Outcome` as checklist), `screen/agents.rs` (`setup_form`)
**Test:** `crates/dexo-app` unit test, `tests/agents_setup.rs`

**Interfaces:**
- Produces:
  - `McpClient::installed(self, places: &Places) -> bool`: the command on PATH via
    `resolve_command`, or for Claude Desktop the config folder exists.
  - `ClientRow { found: bool, config_exists: bool }`.

- [ ] **Step 1: Failing tests**
  - **Rendered Setup detail:** no line is longer than the pane, there is no `By hand:`
    line, and `Writes`, `Skill`, `Status` and `Profile` are present. `y` copies the
    command.
  - **List:** `not found` on a client whose command is missing (the fake `ClientRow`
    with `found: false`).
  - **dexo-app:** `installed` is true for a client whose command is in a temp `PATH`
    folder.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** per the spec's Setup layout; the outcome lines become
  `✓ …` / `→ restart …`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): Agents' Setup shows a client as fields, says which are installed, and copies the command`

### Task 10: Profiles are edited in the TUI

**Files:** `screens/mcp_profiles.rs`, `screen/agents.rs`, `update.rs`, `runtime/mod.rs` (`Effect::SaveMcpProfileAccess { name, connections, reads }` via `off_the_loop`)
**Test:** `tests/agents_profiles.rs`

- [ ] **Step 1: Failing tests**
  - **`c`:** opens the connections checklist in the detail. Space toggles, and Enter
    emits `SaveMcpProfileAccess` with the checked names.
  - **`q`:** emits it with `reads` flipped.
  - **`n`:** opens Setup with a new profile.
  - **Buttons:** the action bar holds Enable/Disable, Connections, Read SQL, Grant,
    Revoke and Delete.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): a profile's connections and read SQL are changed on Agents`

### Task 11: Approvals and Activity get buttons and filters

**Files:** `screen/agents.rs`, `screens/mcp_audit.rs`, `update.rs`
**Test:** `tests/agents_activity.rs`

- [ ] **Step 1: Failing tests**
  - **Approvals:** the action bar holds Approve and Deny, and a click on Approve opens
    the confirmation.
  - **Activity:** the `p` chip cycles Profile and `o` cycles Outcome (ok / failed /
    denied); the list narrows and the title says `N shown`.
- [ ] **Step 2-4:** run, implement, run.
- [ ] **Step 5: Commit** — `feat(tui): Approvals answers by button and Activity filters by profile and outcome`

### Task 12: Server's toolbar, sessions filters, sort, cancel and "you"

**Files:** `crates/dexo-driver-api/src/admin.rs` (`SessionInfo { application: Option<String>, client: Option<String> }` with `#[serde(default)]`), postgres and mysql `admin.rs`, `screens/admin.rs`, `screen/server.rs`, `update.rs`, `runtime/admin_manager.rs` (cancel through `execute_action`)
**Test:** `tests/server_screen.rs`, driver unit tests where they exist

- [ ] **Step 1: Failing tests**
  - **Idle hidden:** the sessions list hides idle by default; `a` shows them.
  - **Search:** narrows by user, database and query.
  - **Sort:** `s` cycles the column, and the header shows `▼`.
  - **"you":** a session with `application == Some("dexo")` shows `you` and its Cancel
    and Terminate buttons are dimmed with a reason.
  - **Cancel:** `k` opens a Cancel/Confirm question, and Confirm emits
    `Effect::AdminCancel { session, target }`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**
  - Postgres adds `application_name` and `client_addr::text`; MySQL adds `HOST` as the
    client.
  - The toolbar holds the server chip `Server: pg-dev` (`c`), the live state, and the
    Pause and Refresh buttons.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(tui): Server hides idle sessions, searches, sorts, cancels a query and marks Dexo's own`

### Task 13: Server's Locks, Sizes, Stats and Settings

**Files:** `action.rs` (`Effect::LoadAdminView { session, view }`, `Action::AdminViewLoaded`), `runtime/mod.rs` and `admin_manager.rs` (side connection as for sessions), `screens/admin.rs` (`ServerView`, rows per view), `screen/server.rs`
**Test:** `tests/server_screen.rs`

- [ ] **Step 1: Failing tests**
  - **Views:** `2` shows Locks; `AdminViewLoaded` with locks draws its table.
  - **Settings:** `5` with variables is searchable.
  - **Refused:** a restriction shows its message in the view.
- [ ] **Step 2-4:** run, implement, run.
- [ ] **Step 5: Commit** — `feat(tui): Server shows its locks, sizes, statistics and settings`

### Task 14: Compare's sources toolbar, swap, chips and grouped list

**Files:** `screens/schema_diff.rs`, `screen/compare.rs`, `update.rs`
**Test:** `tests/compare_screen.rs`

- [ ] **Step 1: Failing tests**
  - **After comparing:** the toolbar still shows `From: pg-dev` and `To:`; `s` swaps
    them.
  - **Chips:** `+N added` toggles with `a` and with a click.
  - **Grouping:** the list is grouped under `Tables`, `Indexes` and so on.
  - **Match:** an empty diff says "✓ The schemas match."
- [ ] **Step 2-4:** run, implement, run.
- [ ] **Step 5: Commit** — `feat(tui): Compare keeps its sources in view, swaps them, and lists differences by kind with counts`

### Task 15: The floors

**Files:** `screen/mod.rs` (`list_and_detail` narrow mode), every `screen/*.rs` render
**Test:** `tests/screen_floors.rs`

- [ ] **Step 1: Failing tests**
  - **80x24:** on every screen with data, the action bar's first button and the list's
    first row are on screen.
  - **60x20:** one column. The list is shown; Enter (or Alt+2) shows the detail with its
    buttons; Esc returns to the list.
- [ ] **Step 2-4:** run, implement, run.
- [ ] **Step 5: Commit** — `fix(tui): every screen keeps its list, detail and buttons at 80x24 and folds to one column at 60`

### Task 16: Docs and QA

- [ ] Update `docs/src/workbench.md` (Connections, History, Server, Compare, Agents sections) and `docs/src/mcp.md` (Setup).
- [ ] QA harness: every screen at 140x40, 80x24 and 60x20 against qa-pg/qa-mysql.
- [ ] Full gate: fmt, clippy, workspace tests, `cargo deny check`, `cargo check --locked`.
- [ ] Commit docs: `docs: the screens' buttons, filters and views`.
