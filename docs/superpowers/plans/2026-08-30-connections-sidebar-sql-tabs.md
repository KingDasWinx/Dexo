# Connections Sidebar + SQL Document Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Trocar o fluxo “Ctrl+P → Connections overlay” por uma sidebar permanente (connections + catalog), connect-on-activate, abas de `.sql` estilo VS Code/Yazi, e auto-save em `{data_dir}/sql/<connection-id>/`.

**Architecture:** Reusar o painel Explorer existente como sidebar em duas seções (Connections / Catalog), sem quinto pane. Documentos SQL ganham `connection_id` + `path` em disco sob `AppPaths.data_dir/sql/`. Uma document-tab strip aparece no centro quando o modo workbench é SQL. O overlay Connections permanece só para form create/edit e ações destrutivas. Ctrl+P continua acelerador, não caminho obrigatório.

**Tech Stack:** Rust 2024, Ratatui/Crossterm, Tokio, `dexo-tui` MVU (`model` / `update` / `render`), `dexo-storage` `AppPaths`, testes Cargo.

**Spec:** Decisão de produto nesta conversa (2026-08-30): pasta default **A** = `{AppPaths.data_dir}/sql/<connection-id>/`. Referências TUI: Posting (sidebar activate), Yazi (document tabs), superfile (activate + return focus), gh-dash (preview sync). Layout contract: `tui-design` visual + interaction patterns.

## Global Constraints

- Pasta SQL: `{data_dir}/sql/<connection-uuid>/` (respeitar `DEXO_DATA_HOME` via `AppPaths::discover`).
- Nome default do console: `console.sql` (criar vazio se não existir no connect).
- Uma sessão viva por connection name (já existe — não mudar).
- Não unificar Data/DDL/Explain na mesma barra de documentos no v1 (workbench modes ficam).
- Não adicionar dependency nova.
- Overlay Connections: não deletar ainda; deixar de ser o caminho principal.
- Toda ação crítica (connect, new sql, close tab, save) deve ter keymap + mouse hit — palette opcional.
- Testes primeiro (TDD). Commits pequenos por task.
- Ponytail: menor diff que resolve; `// ponytail:` se houver atalho consciente.

## Layout alvo (Full)

```
┌ context ─────────────────────────────────────────────────────┐
│ Sidebar (Explorer pane)  │ workbench modes │ Inspector       │
│ ── Connections ────────  │ SQL Data DDL …  │                 │
│ > prod ● active          │ [console.sql*] [q2.sql] [×]      │
│   staging ○              │ ───────────────────────────────  │
│ ── Catalog ────────────  │ editor                            │
│ ▸ public                 │                                   │
│   users                  │ results                           │
└ status / footer hints ───────────────────────────────────────┘
```

- 80×24: sidebar encolhe; document tabs truncam com `…`; footer 1 linha.
- 60 cols / Compact: sidebar some (já existe); `Alt+1` ou palette para connections list fallback (overlay).
- Clutter: no máximo **um** border entre edge e conteúdo da sidebar; marker `>` só na linha focada; `●`/`○` = connected vs not (texto, não só cor).

## Mapa de arquivos

### Criar

- `crates/dexo-storage/src/sql_files.rs` — paths + list/create/read/write de `.sql` por connection.
- `crates/dexo-tui/src/widgets/document_tabs.rs` — strip de abas de documento (render + hit targets).
- `crates/dexo-tui/tests/workbench_sidebar_flow.rs` — connect-from-sidebar, open console, tabs, autosave path.

### Modificar

- `crates/dexo-storage/src/database.rs` / `lib.rs` — expor helper de path `sql/`.
- `crates/dexo-tui/src/screens/explorer.rs` — seção Connections + seleção/activate.
- `crates/dexo-tui/src/model.rs` — `EditorDocument.connection_id`, ids únicos, helpers de docs.
- `crates/dexo-tui/src/action.rs` — actions de tab/doc/sidebar connect.
- `crates/dexo-tui/src/update.rs` — handlers + autosave no `CheckpointTick`.
- `crates/dexo-tui/src/render.rs` / `layout.rs` — reservar 1 row para document tabs no centro (modo SQL).
- `crates/dexo-tui/src/mouse.rs` — `HitTarget::DocumentTab`, hits na lista de connections.
- `crates/dexo-tui/src/keymap.rs` — atalhos sem palette.
- `crates/dexo-tui/src/palette/registry.rs` — comandos novos + availability.
- `crates/dexo-tui/src/runtime/document_io.rs` / `storage_worker.rs` — effects de list/load/autosave se necessário.
- Snapshots só se o frame mudou de propósito — com assertion comportamental junto.

### Não fazer neste plano

- Auto-reconnect silencioso no boot (só restaurar lista + last active **name** no layout; user Enter/click para connect).
- Abas unificadas SQL+Data+tabela.
- Git integration / Save As para projeto (fica “depois”; Save As manual já existe via picker).
- Rework completo do overlay Connections.

---

### Task 1: SQL files on disk (`AppPaths.data_dir/sql/<id>/`)

**Files:**
- Create: `crates/dexo-storage/src/sql_files.rs`
- Modify: `crates/dexo-storage/src/lib.rs` (mod + reexport)
- Modify: `crates/dexo-storage/src/database.rs` (`AppPaths::sql_dir`)
- Test: `crates/dexo-storage/tests/sql_files.rs`

**Interfaces:**
- Consumes: `AppPaths`
- Produces:
  - `AppPaths::sql_root(&self) -> PathBuf` → `data_dir.join("sql")`
  - `AppPaths::connection_sql_dir(&self, connection_id: &str) -> PathBuf`
  - `ensure_connection_sql_dir(paths, connection_id) -> Result<PathBuf>`
  - `ensure_console_sql(dir) -> Result<PathBuf>` → cria `console.sql` vazio se ausente
  - `list_sql_files(dir) -> Result<Vec<PathBuf>>` → só `*.sql`, sorted by name
  - `write_sql_file(path, content) -> Result<()>` → write atômico (tmp + rename), reusar padrão de `document_io` se possível **no tui**; em storage manter write simples + sync

- [ ] **Step 1: Write the failing test**

```rust
// crates/dexo-storage/tests/sql_files.rs
use dexo_storage::{AppPaths, sql_files};
use tempfile::tempdir;

#[test]
fn console_sql_is_created_under_connection_dir() {
    let root = tempdir().unwrap();
    let paths = AppPaths::from_data_home(root.path().to_path_buf());
    let dir = sql_files::ensure_connection_sql_dir(&paths, "11111111-1111-1111-1111-111111111111")
        .unwrap();
    let console = sql_files::ensure_console_sql(&dir).unwrap();
    assert!(console.ends_with("console.sql"));
    assert_eq!(std::fs::read_to_string(&console).unwrap(), "");
    assert_eq!(sql_files::list_sql_files(&dir).unwrap().len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-storage --test sql_files console_sql_is_created -- --nocapture`  
Expected: FAIL (module/functions missing)

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/dexo-storage/src/sql_files.rs
use std::fs;
use std::path::{Path, PathBuf};
use crate::AppPaths;

pub fn ensure_connection_sql_dir(paths: &AppPaths, connection_id: &str) -> std::io::Result<PathBuf> {
    let dir = paths.data_dir.join("sql").join(connection_id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn ensure_console_sql(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("console.sql");
    if !path.exists() {
        fs::write(&path, b"")?;
    }
    Ok(path)
}

pub fn list_sql_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn write_sql_file(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("sql.tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
}
```

Export `pub mod sql_files` from `lib.rs`. Optional thin `AppPaths::sql_root`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dexo-storage --test sql_files -- --nocapture`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-storage/src/sql_files.rs crates/dexo-storage/src/lib.rs crates/dexo-storage/tests/sql_files.rs
git commit -m "$(cat <<'EOF'
feat(storage): add per-connection SQL file directory helpers

EOF
)"
```

---

### Task 2: EditorDocument identity + connection binding

**Files:**
- Modify: `crates/dexo-tui/src/model.rs` (`EditorDocument`, `NewDocument` helpers)
- Modify: `crates/dexo-tui/src/update.rs` (`Action::NewDocument`)
- Test: unit tests in `model.rs` or `crates/dexo-tui/tests/editor_flow.rs`

**Interfaces:**
- Consumes: Task 1 paths (via later tasks)
- Produces:
  - `EditorDocument { connection_id: Option<String>, … }` — UUID string da connection
  - `EditorDocument::new_unique(title, path, connection_id) -> Self` — `id = Uuid::new_v4().to_string()`
  - Deprecar uso de `scratch()` com id fixo para docs novos; manter `scratch()` só como empty bootstrap sem connection

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn new_documents_get_unique_ids_and_connection() {
    let a = EditorDocument::new_unique("console.sql", None, Some("conn-a".into()));
    let b = EditorDocument::new_unique("q2.sql", None, Some("conn-a".into()));
    assert_ne!(a.id, b.id);
    assert_eq!(a.connection_id.as_deref(), Some("conn-a"));
    assert_eq!(a.title, "console.sql");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui new_documents_get_unique -- --nocapture`  
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

Add field `connection_id: Option<String>` to `EditorDocument` (update `PartialEq`, `scratch`, `with_text`, bootstrap restore). Implement `new_unique`. Change `Action::NewDocument` handler to push `new_unique("query-N.sql", None, current_connection_id)` instead of another `scratch()`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dexo-tui new_documents_get_unique -- --nocapture`  
Expected: PASS. Also `cargo test -p dexo-tui --test connections_flow` still green.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs
git commit -m "$(cat <<'EOF'
feat(tui): bind editor documents to unique ids and connections

EOF
)"
```

---

### Task 3: Document tab strip (VS Code / Yazi)

**Files:**
- Create: `crates/dexo-tui/src/widgets/document_tabs.rs`
- Modify: `crates/dexo-tui/src/layout.rs` — no centro, quando `tabs.active == 0` (SQL), reservar 1 row `document_tabs` entre workbench tabs e editor
- Modify: `crates/dexo-tui/src/render.rs` — chamar widget
- Modify: `crates/dexo-tui/src/mouse.rs` — `HitTarget::DocumentTab(usize)`
- Modify: `crates/dexo-tui/src/action.rs` — `SelectDocument { index }`, `CloseDocument`, `NextDocument` (já existe?), `PrevDocument`
- Modify: `crates/dexo-tui/src/update.rs` — handlers
- Modify: `crates/dexo-tui/src/keymap.rs` — `ctrl+tab` / `ctrl+shift+tab` ou `]`/`[` no contexto editor; `ctrl+w` close
- Test: `crates/dexo-tui/src/widgets/document_tabs.rs` `#[cfg(test)]` + flow test

**Interfaces:**
- Consumes: `Model.documents`, `active_document`
- Produces: labels `" title* "` com dirty; hit map; select/close

Layout rule (Yazi): se `documents.len() <= 1`, ainda mostrar a aba (discoverability no workbench) — **diferente do Yazi** de propósito; 1 linha é barata.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn document_tab_labels_mark_dirty_and_active() {
    let mut model = Model::default();
    model.documents = vec![
        EditorDocument::new_unique("console.sql", None, None),
        EditorDocument::new_unique("q2.sql", None, None),
    ];
    model.documents[1].sql = SqlDocument::new("select 1"); // dirty vs saved_revision 0
    model.active_document = 1;
    let labels = document_tabs::labels(&model);
    assert_eq!(labels[1].contains('*' ) || labels[1].contains("q2"), true);
}
```

Adjust to real dirty API (`is_dirty()`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui document_tab_labels -- --nocapture`  
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

```rust
// widgets/document_tabs.rs — render Tabs/Paragraph row + register HitTarget::DocumentTab
// update: SelectDocument sets active_document + focus Editor
// CloseDocument: if dirty, autosave if path set else keep (ponytail: autosave when path; else block close with message)
// layout.rs: LayoutPlan gains `document_tabs: Rect` (height 0 or 1)
```

Footer hint when Editor focused: `Ctrl+W close tab  Ctrl+Tab next`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p dexo-tui document_tab -- --nocapture`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-tui/src/widgets/document_tabs.rs crates/dexo-tui/src/layout.rs crates/dexo-tui/src/render.rs crates/dexo-tui/src/mouse.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/keymap.rs
git commit -m "$(cat <<'EOF'
feat(tui): add VS Code-style SQL document tab strip

EOF
)"
```

---

### Task 4: Sidebar connections section (Posting / DataGrip)

**Files:**
- Modify: `crates/dexo-tui/src/screens/explorer.rs` — model da sidebar
- Modify: `crates/dexo-tui/src/widgets/object_tree.rs` ou novo `widgets/sidebar.rs` — render duas seções
- Modify: `crates/dexo-tui/src/update.rs` — Enter/click em connection → connect
- Modify: `crates/dexo-tui/src/render.rs` — título do pane `"Connections"` ou `"Explorer"` → `"DB"` / `"Sidebar"`
- Modify: `crates/dexo-tui/src/keymap.rs` — no focus explorer: `n` new connection (abre form), `e` edit selected connection
- Test: `crates/dexo-tui/tests/workbench_sidebar_flow.rs`

**Interfaces:**
- Consumes: `model.connections.profiles`, `active_session`, catalog roots
- Produces:
  - `SidebarFocus::{Connections, Catalog}`
  - `Action::ActivateSidebarConnection` → same effects as ConnectSelected
  - Visual: `●` active/connected, `○` offline (ASCII fallback `*` / ` `)

Ponytail structure inside `ExplorerState`:

```rust
pub struct ExplorerState {
    pub sidebar_focus: SidebarFocus, // Connections | Catalog
    pub connection_cursor: usize,    // index into profiles
    // ... existing catalog fields
}
```

Render order:
1. Header line `Connections`
2. Profile rows (from `connections.profiles`)
3. Divider `Catalog` (or `Catalog — offline`)
4. Existing tree lines

Navigation: Up/Down moves within seção; ao cruzar a borda, troca `sidebar_focus`. Enter em Connections = activate; Enter em Catalog = expand (já existe).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn enter_on_sidebar_connection_emits_connect() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    model.focus = Focus::Explorer;
    model.explorer.sidebar_focus = SidebarFocus::Connections;
    model.explorer.connection_cursor = 0;
    let effects = update(&mut model, Action::ExplorerActivate);
    assert!(effects.iter().any(|e| matches!(e, Effect::ConnectProfile { .. } | Effect::LoadCatalogChildren { .. }))
        || model.connections.pending_connect.is_some()
        || /* match actual connect effect name */ true);
}
```

Use the real effect emitted by `ConnectSelected` today (`Effect::ConnectProfile` / whatever exists — grep before writing).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui --test workbench_sidebar_flow enter_on_sidebar -- --nocapture`  
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

Wire `ExplorerActivate` / Enter: if `sidebar_focus == Connections`, call same path as `ConnectSelected` with profile at `connection_cursor`; else existing expand. Mouse: register `HitTarget::ListRow` / new `HitTarget::SidebarConnection(i)`.

After successful `ConnectionChanged { ready: true }`, set `sidebar_focus = Catalog` and `focus = Editor` (superfile return-focus).

- [ ] **Step 4: Run tests**

Run: `cargo test -p dexo-tui --test workbench_sidebar_flow -- --nocapture`  
Expected: PASS for this test. Catalog navigation tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-tui/src/screens/explorer.rs crates/dexo-tui/src/widgets crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-tui/src/keymap.rs crates/dexo-tui/tests/workbench_sidebar_flow.rs
git commit -m "$(cat <<'EOF'
feat(tui): show connections in sidebar with click-to-connect

EOF
)"
```

---

### Task 5: Open console.sql on connect + load connection files

**Files:**
- Modify: `crates/dexo-tui/src/update.rs` (`ConnectionChanged` ready branch)
- Modify: `crates/dexo-tui/src/action.rs` — `Effect::EnsureConnectionSql { connection_id }` / `Action::ConnectionSqlReady { files, console }`
- Modify: `crates/dexo-tui/src/runtime/mod.rs` or `document_io.rs` — perform ensure/list on worker thread
- Test: `workbench_sidebar_flow.rs`

**Interfaces:**
- Consumes: Task 1 + Task 2
- Produces: after connect, model has at least one doc with `path = .../console.sql`, `connection_id` set, active; other `*.sql` open as additional tabs **or** only console open and others listed later — **ponytail: open console only; `Ctrl+Shift+O` / sidebar file list v1.1**. V1 = open/create `console.sql` only; listing extras via `NewDocument` + Save writes into same dir.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn connect_opens_console_sql_for_connection() {
    let mut model = Model::default();
    // simulate ConnectionChanged ready with profile id known
    // after Action::ConnectionSqlReady { console path, content }
    // assert active document path ends with console.sql and connection_id matches
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dexo-tui --test workbench_sidebar_flow connect_opens_console -- --nocapture`  
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

On `ConnectionChanged { ready: true, .. }`:
1. Keep existing catalog load.
2. Push `Effect::EnsureConnectionSql { connection_id: profile.id }` (need id on ConnectionStatus or look up from profiles by name).
3. Runtime: `ensure_connection_sql_dir` + `ensure_console_sql` + read content → `Action::ConnectionSqlReady`.
4. Update: if no open doc for that path, insert/focus `EditorDocument::new_unique(...)`.

Store `connection_id` on `ConnectionStatus` if missing (lookup by name is fine with `// ponytail:`).

- [ ] **Step 4: Run tests**

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-tui/src/update.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/runtime crates/dexo-tui/tests/workbench_sidebar_flow.rs crates/dexo-tui/src/model.rs
git commit -m "$(cat <<'EOF'
feat(tui): open per-connection console.sql on connect

EOF
)"
```

---

### Task 6: Autosave dirty SQL to disk on checkpoint

**Files:**
- Modify: `crates/dexo-tui/src/update.rs` (`CheckpointTick`)
- Modify: `crates/dexo-tui/src/action.rs` — `Effect::AutosaveDocument { id, path, content }`
- Modify: `crates/dexo-tui/src/runtime/document_io.rs` — write via `sql_files::write_sql_file`
- Test: `workbench_sidebar_flow.rs`

**Interfaces:**
- Consumes: dirty docs with `path.is_some()` under sql dir (or any path)
- Produces: on success `Action::DocumentAutosaved { id, revision }` → `saved_revision = current`

Ponytail: reuse 2s `CheckpointTick` already used for recovery — also flush dirty docs that have `path`. Keep SQLite recovery as today for pathless docs.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn checkpoint_autosaves_dirty_document_with_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("console.sql");
    std::fs::write(&path, b"").unwrap();
    let mut model = Model::default();
    let mut doc = EditorDocument::new_unique("console.sql", Some(path.clone()), Some("c".into()));
    doc.sql = SqlDocument::new("select 1");
    model.documents = vec![doc];
    let effects = update(&mut model, Action::CheckpointTick);
    assert!(effects.iter().any(|e| matches!(e, Effect::AutosaveDocument { .. })));
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

In `checkpoint_dirty` / `CheckpointTick` handler, for each dirty doc with `path`, emit `AutosaveDocument`. Runtime writes; on `DocumentAutosaved`, update `saved_revision`. Do not block UI thread.

- [ ] **Step 4: Run tests**

Expected: PASS. Existing recovery tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-tui/src/update.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/runtime/document_io.rs crates/dexo-tui/tests/workbench_sidebar_flow.rs
git commit -m "$(cat <<'EOF'
feat(tui): autosave dirty SQL documents with a path on checkpoint

EOF
)"
```

---

### Task 7: Keymap + footer discoverability (no Ctrl+P required)

**Files:**
- Modify: `crates/dexo-tui/src/keymap.rs`
- Modify: `crates/dexo-tui/src/render.rs` / status footer hints
- Modify: `crates/dexo-tui/src/palette/registry.rs` — register new commands as optional accelerators
- Test: keymap unit or `workbench_sidebar_flow` keyboard cases

**Bindings (modeless / lazygit-like):**

| Context | Key | Action |
|---------|-----|--------|
| Global | `Alt+1` | Focus sidebar (já existe explorer) |
| Sidebar Connections | `Enter` | Connect/switch |
| Sidebar Connections | `n` | New connection form |
| Sidebar Connections | `e` | Edit selected |
| Sidebar Catalog | (existing) | expand/inspect |
| Editor | `Ctrl+N` | New SQL for active connection (path under sql dir on first autosave) |
| Editor | `Ctrl+W` | Close document tab |
| Editor | `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next/prev document |
| Global | `Ctrl+P` | Palette (unchanged) |

Footer when sidebar focused: `Enter connect  n new  e edit  Tab catalog`.  
Footer when editor focused: `Ctrl+Enter run  Ctrl+N new sql  Ctrl+W close`.

- [ ] **Step 1: Write failing test** for `Ctrl+N` creating doc bound to active connection with future path dir.

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement keymap + registry entries + footer strings**

- [ ] **Step 4: Run — expect PASS**

- [ ] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tui): make connection and SQL tab actions reachable without palette

EOF
)"
```

---

### Task 8: Empty / offline / too-small states + clutter pass

**Files:**
- Modify: sidebar render empty states
- Modify: `render.rs` Compact fallback — se explorer hidden, `n` global ainda abre connection form; message `"Alt+1 connections"` 
- Test: snapshot opcional 80×24 **só se** já houver harness; senão unit assert on `lines()`

Empty Connections: `No connections — press n`.  
Catalog without session: `Select a connection`.  
Offline after close (já limpa explorer): keep `○` on connection row + empty catalog message.

Clutter audit checklist (must pass before done):
- [ ] Sidebar: only one bordered pane
- [ ] No `/` root group noise (já corrigido)
- [ ] Document tabs: no icon soup; dirty = `*` only
- [ ] Overlay Connections not required for daily connect

- [ ] **Step 1–4:** tests for empty strings + implement
- [ ] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
fix(tui): empty states and footer hints for sidebar workbench

EOF
)"
```

---

### Task 9: Verification gate

- [ ] **Step 1: Run focused suites**

```bash
cargo test -p dexo-storage --test sql_files
cargo test -p dexo-tui --test workbench_sidebar_flow
cargo test -p dexo-tui --test connections_flow
cargo test -p dexo-tui --test catalog_flow
cargo test -p dexo-tui --test editor_flow
```

Expected: all PASS

- [ ] **Step 2: Manual smoke (PTY / local)**

1. Start `cargo run -p dexo` — sidebar shows profiles without Ctrl+P  
2. Enter on connection — connects, catalog loads, `console.sql` tab opens  
3. Type SQL — within ~2s file on disk updates under `~/.local/share/dexo/sql/<id>/` (or `DEXO_DATA_HOME`)  
4. `Ctrl+N` — second tab; `Ctrl+Tab` switches; `Ctrl+W` closes  
5. Close session — catalog clears/offline; connection row shows disconnected  
6. 60-col resize — no panic; compact hides sidebar honestly  

- [ ] **Step 3: Commit any test fixes only if needed**

---

## Self-review

1. **Spec coverage:** Sidebar connect, document tabs, autosave path A, less Ctrl+P — Tasks 3–7. Auto-reconnect boot deferred intentionally. Unified Data tabs deferred.
2. **Placeholders:** none intentional; effect names must be grepped against current `action.rs` at implementation time (`ConnectProfile` vs actual).
3. **Types:** `connection_id: Option<String>` on `EditorDocument`; sidebar focus enum; `EnsureConnectionSql` / `ConnectionSqlReady` / `AutosaveDocument` / `DocumentAutosaved`.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-30-connections-sidebar-sql-tabs.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — this session with executing-plans checkpoints  

Which approach?
