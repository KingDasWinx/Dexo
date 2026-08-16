# Command Palette Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Corrigir as 129 entradas do Command Palette para que cada comando execute uma ação válida, abra a tela/modal correto ou fique indisponível com uma explicação verdadeira.

**Architecture:** Um registro contextual central, avaliado contra o `Model` real, produz `Dispatch(Action)` ou `OpenFlow(FlowIntent)`. A TUI apenas prepara e valida estado renderizável; filesystem, banco e subprocessos são executados pelo `WorkbenchRuntime` e seus managers, com resultados correlacionados de volta ao reducer.

**Tech Stack:** Rust 2024, Ratatui/Crossterm, Tokio, drivers `dexo-driver-api`, serviços `dexo-app`, storage SQLite, testes unitários e de integração Cargo, snapshots Insta.

---

## Referências e estado inicial

- Especificação aprovada: `docs/superpowers/specs/2026-08-16-command-palette-remediation-design.md`.
- Auditoria comando a comando: `docs/audits/2026-08-16-command-palette-audit.md`.
- Registro atual: `crates/dexo-tui/src/palette.rs:6-1126`.
- Seleção/foco atual: `crates/dexo-tui/src/update.rs:1531-1810` e `3335-3347`.
- Runtime atual: `crates/dexo-tui/src/runtime/mod.rs:160-447`.
- Baseline conhecido: os 6 testes unitários do palette passam; `cargo test -p dexo-tui` possui 3 snapshots divergentes. Não aceite snapshots em lote: cada alteração visual precisa de assertion comportamental correspondente.

## Mapa de arquivos

### Criar

- `crates/dexo-tui/src/palette/registry.rs` — metadados, disponibilidade e invocação contextual dos 129 comandos.
- `crates/dexo-tui/src/screens/transaction_prompt.rs` — entrada visível para create/rollback/release de savepoint.
- `crates/dexo-tui/src/screens/diagnostics.rs` — preview, destino, progresso e erro da exportação de diagnostics.
- `crates/dexo-tui/tests/command_palette_contract.rs` — contrato tabelado de todos os IDs.
- `crates/dexo-tui/tests/command_palette_flow.rs` — interação real `OpenPalette -> digitar -> Enter`.

### Modificar no núcleo

- `crates/dexo-tui/src/palette.rs` — tipos públicos, busca/filtro e reexport do registro.
- `crates/dexo-tui/src/model.rs` — foco de origem, prompts e estados correlacionados.
- `crates/dexo-tui/src/action.rs` — intents, requests e resultados ausentes.
- `crates/dexo-tui/src/update.rs` — resolução contextual e preparação das telas.
- `crates/dexo-tui/src/render.rs` — atalhos, prompts, erros e confirmações visíveis.
- `crates/dexo-tui/src/keymap.rs` — resolver comandos com o `Model` real.
- `crates/dexo-tui/src/screens/mod.rs` — exportar as duas novas screens.

### Modificar por domínio

- Projetos/conexões: `screens/projects.rs`, `screens/connections.rs`, `runtime/session_registry.rs`, `tests/projects_flow.rs`, `tests/connections_flow.rs`.
- Dados/explorer: `screens/data.rs`, `screens/explorer.rs`, `tests/data_flow.rs`, `tests/catalog_flow.rs`.
- Transfer: `screens/transfer.rs`, `screens/file_picker.rs`, `runtime/transfer_manager.rs`, `runtime/mod.rs`, `crates/dexo-app/src/transfer/{export.rs,import.rs,native_tool.rs}`, `tests/schema_transfer_explain_flow.rs`.
- Schema/security: `screens/schema_diff.rs`, `screens/security.rs`, `runtime/schema_manager.rs`, `runtime/mod.rs`, `tests/schema_transfer_explain_flow.rs`.
- Editor/explain: `screens/editor.rs`, `runtime/explain_manager.rs`, `runtime/storage_worker.rs`, `tests/editor_flow.rs`, `tests/schema_transfer_explain_flow.rs`.
- Settings/recovery/MCP/diagnostics: `screens/settings.rs`, `screens/recovery.rs`, `screens/mcp_profiles.rs`, `screens/mcp_audit.rs`, `runtime/diagnostic_manager.rs`, `runtime/mod.rs`, `crates/dexo-storage/src/mcp/grant_repo.rs`, `tests/admin_settings_mcp_flow.rs`.

## Regras de implementação

- Preserve todos os 129 IDs.
- Não use `Model::default()` para resolver comandos durante a execução.
- Não use nome vazio, `"sp1"`, primeiro item implícito ou alvo invisível como argumento.
- Um item indisponível permanece no palette, Enter não fecha o overlay e o motivo fica visível.
- Nenhum I/O novo entra em `update.rs`.
- Cada task começa com teste falhando, termina com teste verde e commit próprio.

### Task 1: Introduzir o contrato contextual e restaurar foco/atalhos

**Files:**
- Create: `crates/dexo-tui/src/palette/registry.rs`
- Create: `crates/dexo-tui/tests/command_palette_flow.rs`
- Modify: `crates/dexo-tui/src/palette.rs:1-15,1107-1126`
- Modify: `crates/dexo-tui/src/model.rs:37-60,905-1010`
- Modify: `crates/dexo-tui/src/update.rs:1467-1472,1774-1810,3335-3347`
- Modify: `crates/dexo-tui/src/render.rs:598-635`

- [ ] **Step 1: Escrever os testes falhos do caminho real**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::{Action, Effect, Focus, Model, update};
use dexo_tui::screens::projects::ProjectsMode;

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn choose_effects(model: &mut Model, query: &str) -> Vec<Effect> {
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(press(model, KeyCode::Char(ch)));
    }
    effects.extend(press(model, KeyCode::Enter));
    effects
}

fn choose(model: &mut Model, query: &str) {
    let _ = choose_effects(model, query);
}

#[test]
fn escape_restores_palette_origin_focus() {
    let mut model = Model { focus: Focus::Results, ..Model::default() };
    update(&mut model, Action::OpenPalette);
    assert_eq!(model.focus, Focus::Palette);
    press(&mut model, KeyCode::Esc);
    assert_eq!(model.focus, Focus::Results);
}

#[test]
fn project_create_opens_the_existing_name_form() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    assert!(!model.palette.open);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert!(model.projects.name_input.is_empty());
}

#[test]
fn palette_renders_registered_shortcut() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    let view = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(view.contains("Ctrl+P"));
}
```

- [ ] **Step 2: Executar e confirmar as três falhas**

Run: `cargo test -p dexo-tui --test command_palette_flow -- --nocapture`

Expected: FAIL porque o foco volta ao Editor, `project.create` envia nome vazio e atalhos não são renderizados.

- [ ] **Step 3: Definir os tipos mínimos do contrato**

Em `palette.rs`, substitua a função-ponte sem contexto pelos tipos abaixo e declare `mod registry; pub use registry::palette_entries;`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowIntent {
    ProjectCreate,
    ProjectSwitch,
    ProjectRename,
    ProjectDelete,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteInvocation {
    Dispatch(Action),
    OpenFlow(FlowIntent),
}

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub requirements: &'static [Requirement],
    pub disabled_reason: Option<String>,
    pub invocation: PaletteInvocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement {
    ActiveSession,
    Results,
    RowSelection,
    ExplorerNode,
    LoadedDdl,
    PendingChanges,
    Breadcrumb,
    ActiveQuery,
    Completion,
    Parameters,
    History,
    Recovery,
}

impl Requirement {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ActiveSession => "connect a session first",
            Self::Results => "no results available",
            Self::RowSelection => "select a result row or cell first",
            Self::ExplorerNode => "select an explorer object first",
            Self::LoadedDdl => "load DDL first",
            Self::PendingChanges => "no pending changes",
            Self::Breadcrumb => "no previous data location",
            Self::ActiveQuery => "no query is running",
            Self::Completion => "no completion available",
            Self::Parameters => "no query parameters",
            Self::History => "history is empty",
            Self::Recovery => "no recovery checkpoint",
        }
    }
}

pub fn invocation_by_id(model: &Model, id: &str) -> Option<PaletteInvocation> {
    palette_entries(model)
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.invocation)
}
```

Mova a lista atual para `palette/registry.rs`. Nesta task, converta todas as closures diretas de `action: || Action::X` para `invocation: PaletteInvocation::Dispatch(Action::X)`. Converta somente os quatro comandos de projeto acima para `OpenFlow`; as demais exceções entram nas tasks dos respectivos domínios.

- [ ] **Step 4: Guardar e restaurar o foco de origem**

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub offset: usize,
    pub origin_focus: Option<Focus>,
}

fn open_palette(model: &mut Model) {
    if !model.palette.open {
        model.palette.origin_focus = Some(model.focus);
    }
    model.palette.open = true;
    model.palette.query.clear();
    model.palette.selected = 0;
    model.palette.offset = 0;
    model.focus = Focus::Palette;
}

fn close_palette(model: &mut Model) {
    if model.palette.open {
        model.palette.open = false;
        if model.focus == Focus::Palette {
            model.focus = model.palette.origin_focus.take().unwrap_or(Focus::Editor);
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub requirements: &'static [Requirement],
    pub invocation: PaletteInvocation,
}
```

- [ ] **Step 5: Executar `Dispatch` e `OpenFlow` sem fechar itens inválidos**

```rust
fn invoke_palette(model: &mut Model, invocation: crate::palette::PaletteInvocation) -> Vec<Effect> {
    use crate::palette::{FlowIntent, PaletteInvocation};
    match invocation {
        PaletteInvocation::Dispatch(action) => update(model, action),
        PaletteInvocation::OpenFlow(FlowIntent::ProjectCreate) => {
            model.projects.open = true;
            model.projects.mode = crate::screens::projects::ProjectsMode::Create;
            model.projects.name_input.clear();
            Vec::new()
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectSwitch) => {
            model.projects.open = true;
            vec![Effect::ListProjects]
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectRename) => {
            model.projects.open = true;
            vec![Effect::ListProjects]
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectDelete) => {
            model.projects.open = true;
            vec![Effect::ListProjects]
        }
    }
}

fn palette_select(model: &mut Model) -> Vec<Effect> {
    let entries = crate::palette::palette_entries(model);
    let visible = crate::palette::filter_entries(&entries, &model.palette.query);
    let Some(entry) = visible.get(model.palette.selected) else { return Vec::new() };
    if let Some(reason) = &entry.disabled_reason {
        model.messages.push(reason.clone());
        return Vec::new();
    }
    let invocation = entry.invocation.clone();
    close_palette(model);
    invoke_palette(model, invocation)
}
```

Altere keymaps e menu de resultados para chamar `invocation_by_id(model, id)` e `invoke_palette`, nunca uma ação resolvida com modelo default.

- [ ] **Step 6: Renderizar atalhos sem criar outro layout**

```rust
let shortcut = entry
    .shortcut
    .map(|value| format!(" [{value}]"))
    .unwrap_or_default();
lines.push(format!("{marker} {}{shortcut}{disabled}", entry.title));
```

- [ ] **Step 7: Formatar e executar os testes focados**

Run: `cargo fmt --all && cargo test -p dexo-tui --lib palette::tests && cargo test -p dexo-tui --test command_palette_flow`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/dexo-tui/src/palette.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-tui/tests/command_palette_flow.rs
git commit -m "refactor(tui): add contextual palette invocation"
```

### Task 2: Criar o contrato tabelado dos 129 comandos e guards contextuais

**Files:**
- Create: `crates/dexo-tui/tests/command_palette_contract.rs`
- Modify: `crates/dexo-tui/src/palette/registry.rs`
- Modify: `crates/dexo-tui/src/palette.rs`
- Modify: `crates/dexo-tui/src/keymap.rs:628-646`
- Modify: `crates/dexo-tui/tests/sprint14_screens.rs:44-64`

- [ ] **Step 1: Adicionar a lista canônica que falha se qualquer ID sumir ou duplicar**

Use exatamente este array em `command_palette_contract.rs`:

```rust
const COMMAND_IDS: [&str; 129] = [
    "workbench.quit", "palette.open", "query.execute", "query.execute_statement",
    "query.execute_selection", "query.execute_document", "query.cancel",
    "transaction.begin", "transaction.savepoint", "transaction.rollback_savepoint",
    "transaction.release_savepoint", "transaction.commit", "transaction.rollback",
    "help.open", "focus.explorer", "focus.editor", "focus.results", "focus.inspector",
    "layout.cycle", "layout.results_focus", "layout.hide_inspector", "layout.reset",
    "layout.results_grow", "layout.results_shrink", "layout.explorer_grow",
    "layout.explorer_shrink", "layout.inspector_grow", "layout.inspector_shrink",
    "data.copy.csv", "data.copy.text", "data.copy.json", "data.copy.markdown",
    "data.copy.sql", "data.apply", "data.revert", "data.nav_back", "data.page_next",
    "data.page_prev", "data.sort", "data.filter", "data.review", "data.related",
    "data.inspect", "schema.preview", "schema.raw", "schema.diff", "transfer.export",
    "transfer.import", "backup.dump", "backup.restore", "schema.security",
    "explain.open", "admin.sessions", "mcp.profiles", "explorer.expand",
    "explorer.refresh", "explorer.refresh_all", "explorer.inspect", "explorer.ddl",
    "explorer.refresh_subtree", "explorer.up", "explorer.down",
    "explorer.dependencies", "explorer.dependents", "tab.sql", "tab.data", "tab.ddl",
    "tab.properties", "tab.explain", "tab.next", "document.next", "document.new",
    "document.save", "document.open", "results.select_row", "results.select_column",
    "results.next_tab", "results.prev_tab", "inspector.next_tab", "settings.theme",
    "settings.keymap", "settings.mouse", "explorer.data", "editor.goto",
    "explorer.copy_name", "explorer.copy_simple", "explorer.copy_ddl",
    "explorer.favorite", "explorer.favorites_only", "explorer.system_objects",
    "results.up", "results.down", "results.left", "results.right", "results.pageup",
    "results.pagedown", "results.top", "results.extend_up", "results.extend_down",
    "results.actions", "results.toggle_pick", "connection.add", "connection.browse",
    "connection.connect", "connection.duplicate", "connection.test", "connection.delete",
    "connection.close_session", "project.browse", "project.switch", "project.create",
    "project.rename", "project.delete", "config.transfer", "settings.open",
    "settings.reset", "recovery.open", "recovery.restore", "recovery.discard",
    "mcp.audit", "mcp.revoke_all", "editor.complete", "editor.format",
    "editor.accept_completion", "editor.snippet", "editor.parameters", "editor.history",
    "editor.history.clear", "diagnostics.export",
];

#[test]
fn registry_contains_each_command_exactly_once() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    let actual: std::collections::BTreeSet<_> = entries.iter().map(|e| e.id).collect();
    let expected: std::collections::BTreeSet<_> = COMMAND_IDS.into_iter().collect();
    assert_eq!(entries.len(), 129);
    assert_eq!(actual.len(), 129, "duplicate command id");
    assert_eq!(actual, expected);
}
```

- [ ] **Step 2: Adicionar o contrato de tipo de invocação**

```rust
const FLOW_IDS: &[&str] = &[
    "transaction.savepoint", "transaction.rollback_savepoint",
    "transaction.release_savepoint", "data.sort", "data.filter", "data.review",
    "schema.preview", "schema.raw", "schema.diff", "schema.security",
    "transfer.export", "transfer.import", "backup.dump", "backup.restore",
    "connection.connect", "connection.duplicate", "connection.test",
    "connection.delete", "connection.close_session", "project.switch",
    "project.create", "project.rename", "project.delete", "settings.reset",
    "recovery.restore", "recovery.discard", "mcp.revoke_all", "editor.snippet",
    "editor.parameters", "editor.history.clear", "diagnostics.export",
];

const FLOW_INTENTS: &[(&str, FlowIntent)] = &[
    ("transaction.savepoint", FlowIntent::SavepointCreate),
    ("transaction.rollback_savepoint", FlowIntent::SavepointRollback),
    ("transaction.release_savepoint", FlowIntent::SavepointRelease),
    ("data.sort", FlowIntent::DataSort),
    ("data.filter", FlowIntent::DataFilter),
    ("data.review", FlowIntent::DataReview),
    ("schema.preview", FlowIntent::SchemaPreview),
    ("schema.raw", FlowIntent::SchemaRaw),
    ("schema.diff", FlowIntent::SchemaDiff),
    ("schema.security", FlowIntent::Security),
    ("transfer.export", FlowIntent::TransferExport),
    ("transfer.import", FlowIntent::TransferImport),
    ("backup.dump", FlowIntent::Backup),
    ("backup.restore", FlowIntent::Restore),
    ("connection.connect", FlowIntent::ConnectionConnect),
    ("connection.duplicate", FlowIntent::ConnectionDuplicate),
    ("connection.test", FlowIntent::ConnectionTest),
    ("connection.delete", FlowIntent::ConnectionDelete),
    ("connection.close_session", FlowIntent::ConnectionCloseSession),
    ("project.switch", FlowIntent::ProjectSwitch),
    ("project.create", FlowIntent::ProjectCreate),
    ("project.rename", FlowIntent::ProjectRename),
    ("project.delete", FlowIntent::ProjectDelete),
    ("settings.reset", FlowIntent::SettingsReset),
    ("recovery.restore", FlowIntent::RecoveryRestore),
    ("recovery.discard", FlowIntent::RecoveryDiscard),
    ("mcp.revoke_all", FlowIntent::McpRevokeAll),
    ("editor.snippet", FlowIntent::InsertSnippet),
    ("editor.parameters", FlowIntent::SubmitParameters),
    ("editor.history.clear", FlowIntent::ClearHistory),
    ("diagnostics.export", FlowIntent::DiagnosticsExport),
];

#[test]
fn every_command_declares_direct_or_flow_invocation() {
    use dexo_tui::palette::PaletteInvocation;
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    for entry in entries {
        match entry.invocation {
            PaletteInvocation::OpenFlow(_) => assert!(FLOW_IDS.contains(&entry.id), "unexpected flow {}", entry.id),
            PaletteInvocation::Dispatch(_) => assert!(!FLOW_IDS.contains(&entry.id), "{} must open a flow", entry.id),
        }
    }
}
```

- [ ] **Step 3: Executar e observar falhas de cobertura e tipo**

Run: `cargo test -p dexo-tui --test command_palette_contract -- --nocapture`

Expected: FAIL até todas as closures serem convertidas e os 31 flows declarados. Expanda `FlowIntent` com exatamente os variants usados em `FLOW_INTENTS` e faça o teste também comparar cada ID ao intent esperado.

- [ ] **Step 4: Implementar requisitos de disponibilidade sem duplicar condições**

```rust
fn unmet_requirement(model: &Model, requirement: Requirement) -> Option<String> {
    match requirement {
        Requirement::ActiveSession => model.active_session.is_none().then(|| "connect a session first".into()),
        Requirement::Results => model.results.rows().is_empty().then(|| "no results available".into()),
        Requirement::RowSelection => matches!(model.results.kind, GridSelection::Column { .. })
            .then(|| "select a result row or cell first".into()),
        Requirement::ExplorerNode => model.explorer.selected.is_none().then(|| "select an explorer object first".into()),
        Requirement::LoadedDdl => model.inspector.ddl.is_none().then(|| "load DDL first".into()),
        Requirement::PendingChanges => model.data.changes.pending().is_empty().then(|| "no pending changes".into()),
        Requirement::Breadcrumb => model.data.crumbs.is_empty().then(|| "no previous data location".into()),
        Requirement::ActiveQuery => model.active_operation.is_none().then(|| "no query is running".into()),
        Requirement::Completion => model.editor.completions.is_empty().then(|| "no completion available".into()),
        Requirement::Parameters => model.editor.parameters.is_empty().then(|| "no query parameters".into()),
        Requirement::History => model.editor.history.is_empty().then(|| "history is empty".into()),
        Requirement::Recovery => model.recovery.checkpoints.is_empty().then(|| "no recovery checkpoint".into()),
    }
}

fn first_unmet(model: &Model, requirements: &[Requirement]) -> Option<String> {
    requirements.iter().find_map(|value| unmet_requirement(model, *value))
}
```

Faça `unmet_requirement` retornar `requirement.reason().to_string()` quando a condição daquele variant não for satisfeita. Mude a lista em `registry.rs` para `command_specs() -> Vec<CommandSpec>` e derive as entradas contextuais sem duplicar metadados:

```rust
pub fn palette_entries(model: &Model) -> Vec<PaletteEntry> {
    command_specs()
        .into_iter()
        .map(|spec| PaletteEntry {
            id: spec.id,
            title: spec.title,
            keywords: spec.keywords,
            shortcut: spec.shortcut,
            requirements: spec.requirements,
            disabled_reason: first_unmet(model, spec.requirements),
            invocation: spec.invocation,
        })
        .collect()
}

pub fn command_spec(id: &str) -> Option<CommandSpec> {
    command_specs().into_iter().find(|spec| spec.id == id)
}
```

A execução usa `palette_entries(model)`; `command_spec(id)` existe para inspeção e testes do contrato estático.

- [ ] **Step 5: Aplicar guards às famílias exatas**

Use estas regras no registro:

| Regra | IDs |
|---|---|
| sessão ativa | `query.execute`, `query.execute_statement`, `query.execute_selection`, `query.execute_document`, `transaction.begin`, `transaction.savepoint`, `transaction.rollback_savepoint`, `transaction.release_savepoint`, `transaction.commit`, `transaction.rollback`, `schema.preview`, `schema.raw`, `schema.diff`, `schema.security`, `explain.open`, `admin.sessions`, `explorer.inspect`, `explorer.ddl`, `explorer.dependencies`, `explorer.dependents`, `explorer.data`, `data.page_next`, `data.page_prev`, `data.sort`, `data.filter`, `data.apply` |
| resultado existente | `data.copy.csv`, `data.copy.text`, `data.copy.json`, `data.copy.markdown`, `data.copy.sql`, `data.inspect`, `data.related`, `data.sort`, `data.filter`, `transfer.export`, `results.select_row`, `results.select_column`, `results.next_tab`, `results.prev_tab`, `results.up`, `results.down`, `results.left`, `results.right`, `results.pageup`, `results.pagedown`, `results.top`, `results.extend_up`, `results.extend_down`, `results.actions`, `results.toggle_pick` |
| seleção de célula/linha | `data.inspect`, `data.related`, `results.actions`, `results.toggle_pick` |
| nó do explorer | `explorer.expand`, `explorer.inspect`, `explorer.ddl`, `explorer.refresh_subtree`, `explorer.dependencies`, `explorer.dependents`, `explorer.copy_name`, `explorer.copy_simple`, `explorer.favorite`, `explorer.data` |
| DDL carregado | `explorer.copy_ddl` |
| mudanças pendentes | `data.apply`, `data.revert`, `data.review` |
| breadcrumb existente | `data.nav_back` |
| query ativa | `query.cancel` |
| completion existente | `editor.accept_completion` |
| parâmetros existentes | `editor.parameters` |
| histórico existente | `editor.history.clear` |
| recuperação existente | `recovery.restore`, `recovery.discard` |

Para transações, preserve as regras existentes e acrescente read-only. `rollback_savepoint` deve aceitar `Active` e `Failed`; `commit` somente `Active`; rollback principal aceita `Active` e `Failed`.

Implemente a atribuição sem curingas, com estes braços (os IDs ausentes retornam `&[]`):

```rust
fn requirements_for(id: &str) -> &'static [Requirement] {
    use Requirement::*;
    match id {
        "query.execute" | "query.execute_statement" | "query.execute_selection"
        | "query.execute_document" | "transaction.begin" | "transaction.savepoint"
        | "transaction.rollback_savepoint" | "transaction.release_savepoint"
        | "transaction.commit" | "transaction.rollback" | "schema.preview"
        | "schema.raw" | "schema.diff" | "schema.security" | "explain.open"
        | "admin.sessions" | "data.page_next" | "data.page_prev" => &[ActiveSession],
        "explorer.inspect" | "explorer.ddl" | "explorer.dependencies"
        | "explorer.dependents" | "explorer.data" => &[ActiveSession, ExplorerNode],
        "data.sort" | "data.filter" => &[ActiveSession, Results],
        "data.apply" => &[ActiveSession, PendingChanges],
        "data.copy.csv" | "data.copy.text" | "data.copy.json"
        | "data.copy.markdown" | "data.copy.sql" | "transfer.export"
        | "results.select_row" | "results.select_column" | "results.next_tab"
        | "results.prev_tab" | "results.up" | "results.down" | "results.left"
        | "results.right" | "results.pageup" | "results.pagedown" | "results.top"
        | "results.extend_up" | "results.extend_down" => &[Results],
        "data.inspect" | "data.related" | "results.actions"
        | "results.toggle_pick" => &[Results, RowSelection],
        "explorer.expand" | "explorer.refresh_subtree" | "explorer.copy_name"
        | "explorer.copy_simple" | "explorer.favorite" => &[ExplorerNode],
        "explorer.copy_ddl" => &[LoadedDdl],
        "data.revert" | "data.review" => &[PendingChanges],
        "data.nav_back" => &[Breadcrumb],
        "query.cancel" => &[ActiveQuery],
        "editor.accept_completion" => &[Completion],
        "editor.parameters" => &[Parameters],
        "editor.history.clear" => &[History],
        "recovery.restore" | "recovery.discard" => &[Recovery],
        _ => &[],
    }
}
```

- [ ] **Step 6: Testar motivos representativos de todas as famílias**

```rust
#[test]
fn default_model_explains_missing_context() {
    let entries = dexo_tui::palette::palette_entries(&dexo_tui::Model::default());
    for (id, reason) in [
        ("query.execute_statement", "connect a session first"),
        ("data.copy.csv", "no results available"),
        ("explorer.inspect", "select an explorer object first"),
        ("editor.accept_completion", "no completion available"),
    ] {
        let entry = entries.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(entry.disabled_reason.as_deref(), Some(reason));
    }
}
```

- [ ] **Step 7: Substituir os testes de “reachability” pela fonte canônica**

Faça `keymap.rs` e `sprint14_screens.rs` comparar seus IDs diretamente com `palette_entries(&Model::default())`. Remova as listas de 11/12 IDs que davam cobertura falsa; `COMMAND_IDS` permanece a lista canônica no teste de integração.

```rust
fn assert_registered(ids: impl IntoIterator<Item = &'static str>) {
    let registered: std::collections::BTreeSet<_> =
        crate::palette::palette_entries(&crate::Model::default())
            .into_iter().map(|entry| entry.id).collect();
    for id in ids {
        assert!(registered.contains(id), "unregistered command: {id}");
    }
}
```

- [ ] **Step 8: Rodar e commit**

Run: `cargo fmt --all && cargo test -p dexo-tui --test command_palette_contract && cargo test -p dexo-tui --lib keymap::tests`

Expected: PASS.

```bash
git add crates/dexo-tui/src/palette.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/keymap.rs crates/dexo-tui/tests/command_palette_contract.rs crates/dexo-tui/tests/sprint14_screens.rs
git commit -m "test(tui): contract all command palette entries"
```

### Task 3: Corrigir projetos e conexões com seleção visível

**Files:**
- Modify: `crates/dexo-tui/src/action.rs`
- Modify: `crates/dexo-tui/src/palette.rs`
- Modify: `crates/dexo-tui/src/palette/registry.rs`
- Modify: `crates/dexo-tui/src/screens/projects.rs:6-115`
- Modify: `crates/dexo-tui/src/screens/connections.rs:21-135`
- Modify: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/update.rs:1143-1179,1284-1294,3114-3300`
- Modify: `crates/dexo-tui/tests/command_palette_flow.rs`
- Modify: `crates/dexo-tui/tests/projects_flow.rs`
- Modify: `crates/dexo-tui/tests/connections_flow.rs`

- [ ] **Step 1: Escrever testes falhos para create, rename, switch, delete e conexão**

```rust
#[test]
fn project_create_preserves_invalid_input() {
    let mut model = Model::default();
    choose(&mut model, "project.create");
    press(&mut model, KeyCode::Enter);
    assert!(model.projects.open);
    assert_eq!(model.projects.mode, ProjectsMode::Create);
    assert_eq!(model.projects.error.as_deref(), Some("project name is required"));
}

#[test]
fn project_rename_loads_a_visible_chooser_before_input() {
    let mut model = Model::default();
    let effects = choose_effects(&mut model, "project.rename");
    assert!(model.projects.open);
    assert_eq!(model.projects.intent, Some(ProjectIntent::Rename));
    assert!(matches!(effects.as_slice(), [Effect::ListProjects]));
}

#[test]
fn connection_delete_opens_browser_and_never_hides_confirmation() {
    let mut model = Model::default();
    choose(&mut model, "connection.delete");
    assert!(model.connections.open);
    assert_eq!(model.connections.intent, Some(ConnectionIntent::Delete));
    assert!(model.connections.delete_target.is_none());
}
```

- [ ] **Step 2: Executar os testes focados**

Run: `cargo test -p dexo-tui --test command_palette_flow project -- --nocapture && cargo test -p dexo-tui --test command_palette_flow connection -- --nocapture`

Expected: FAIL porque não existem intents visíveis nem erro de formulário.

- [ ] **Step 3: Acrescentar intents e erro às screens existentes**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectIntent { Switch, Rename, Delete }

// Adições a ProjectsScreen:
pub intent: Option<ProjectIntent>,
pub error: Option<String>,

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionIntent { Connect, Duplicate, Test, Delete, CloseSession }

// Adições a ConnectionsScreen:
pub intent: Option<ConnectionIntent>,
pub error: Option<String>,
```

`lines()` deve renderizar `choose project to ...`, `choose connection to ...` e o erro persistente quando houver.

- [ ] **Step 4: Preparar cada flow no reducer**

```rust
fn open_project_intent(model: &mut Model, intent: ProjectIntent) -> Vec<Effect> {
    model.projects.open = true;
    model.projects.intent = Some(intent);
    model.projects.mode = ProjectsMode::Browse;
    model.projects.error = None;
    vec![Effect::ListProjects]
}

fn open_connection_intent(model: &mut Model, intent: ConnectionIntent) -> Vec<Effect> {
    model.connections.open = true;
    model.connections.intent = Some(intent);
    model.connections.error = None;
    if model.connections.profiles.is_empty() { vec![Effect::LoadConnectionProfiles] } else { Vec::new() }
}
```

Adicione `Effect::LoadConnectionProfiles` em `action.rs` e o braço correspondente em `WorkbenchRuntime::dispatch`; ele executa `ConnectionRepository::list()` e emite o `Action::ProfilesLoaded(Vec<ConnectionProfile>)` já existente. Não reutilize `LoadMcpProfiles`, que consulta outro repositório.

- [ ] **Step 5: Validar nomes antes de limpar ou sair do modo**

```rust
fn submit_project_name(model: &mut Model) -> Vec<Effect> {
    let name = model.projects.name_input.trim();
    if name.is_empty() {
        model.projects.error = Some("project name is required".into());
        return Vec::new();
    }
    let name = name.to_string();
    model.projects.error = None;
    match model.projects.mode {
        ProjectsMode::Create => update(model, Action::CreateProject { name }),
        ProjectsMode::Rename => update(model, Action::RenameProject { name }),
        _ => Vec::new(),
    }
}
```

- [ ] **Step 6: Fazer Enter consumir o intent sobre a seleção renderizada**

No browser de projetos, Enter com `ProjectIntent::Rename` muda para `ProjectsMode::Rename` e preenche `name_input`; Delete emite preview e mantém `projects.open = true`; Switch inicia o fluxo existente. No browser de conexões, Enter chama o handler correspondente e mantém o browser aberto quando surgir `delete_target`.

```rust
fn choose_project_intent(model: &mut Model) -> Vec<Effect> {
    let Some(project) = model.projects.selected().cloned() else {
        model.projects.error = Some("select a project first".into());
        return Vec::new();
    };
    match model.projects.intent {
        Some(ProjectIntent::Switch) => update(model, Action::SwitchProject { name: project.name }),
        Some(ProjectIntent::Rename) => {
            model.projects.mode = ProjectsMode::Rename;
            model.projects.name_input = project.name;
            Vec::new()
        }
        Some(ProjectIntent::Delete) => update(model, Action::PreviewProjectDelete { id: project.id }),
        None => Vec::new(),
    }
}

fn choose_connection_intent(model: &mut Model) -> Vec<Effect> {
    if model.connections.selected().is_none() {
        model.connections.error = Some("select a connection first".into());
        return Vec::new();
    }
    match model.connections.intent {
        Some(ConnectionIntent::Connect) => update(model, Action::ConnectSelected),
        Some(ConnectionIntent::Duplicate) => update(model, Action::DuplicateConnection),
        Some(ConnectionIntent::Test) => update(model, Action::TestConnection),
        Some(ConnectionIntent::Delete) => update(model, Action::DeleteConnection),
        Some(ConnectionIntent::CloseSession) => update(model, Action::CloseSelectedSession),
        None => Vec::new(),
    }
}
```

- [ ] **Step 7: Rodar regressões de storage e UI**

Run: `cargo test -p dexo-tui --test command_palette_flow project && cargo test -p dexo-tui --test projects_flow && cargo test -p dexo-tui --test connections_flow`

Expected: PASS, incluindo confirmação de delete renderizada.

- [ ] **Step 8: Commit**

```bash
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/palette.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/screens/projects.rs crates/dexo-tui/src/screens/connections.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/command_palette_flow.rs crates/dexo-tui/tests/projects_flow.rs crates/dexo-tui/tests/connections_flow.rs
git commit -m "fix(tui): open visible project and connection flows"
```

### Task 4: Corrigir savepoints, sort/filter, paginação e refresh do explorer

**Files:**
- Create: `crates/dexo-tui/src/screens/transaction_prompt.rs`
- Modify: `crates/dexo-tui/src/screens/mod.rs`
- Modify: `crates/dexo-tui/src/screens/data.rs:25-129`
- Modify: `crates/dexo-tui/src/model.rs`
- Modify: `crates/dexo-tui/src/action.rs:111-171`
- Modify: `crates/dexo-tui/src/update.rs:393,1268-1455,2354-2418`
- Modify: `crates/dexo-tui/src/render.rs`
- Modify: `crates/dexo-tui/tests/command_palette_flow.rs`
- Modify: `crates/dexo-tui/tests/data_flow.rs`
- Modify: `crates/dexo-tui/tests/catalog_flow.rs`

- [ ] **Step 1: Escrever testes falhos dos quatro defeitos de estado**

```rust
#[test]
fn savepoint_asks_for_a_name_instead_of_using_sp1() {
    let mut model = active_transaction_model();
    choose(&mut model, "transaction.savepoint");
    assert!(model.transaction_prompt.open);
    assert!(model.transaction_prompt.name.is_empty());
}

#[test]
fn page_without_session_does_not_change_offset_or_loading() {
    let mut model = Model::default();
    update(&mut model, Action::NextDataPage);
    assert_eq!(model.data.page_offset, 0);
    assert!(!model.data.loading);
}

#[test]
fn offline_refresh_preserves_visible_tree() {
    let mut model = explorer_fixture_without_session();
    let before = model.explorer.roots.clone();
    update(&mut model, Action::RefreshCatalogAll);
    assert_eq!(model.explorer.roots, before);
}

#[test]
fn refresh_subtree_does_not_replace_roots() {
    let mut model = connected_explorer_fixture();
    let effects = update(&mut model, Action::RefreshCatalogSubtree);
    assert!(matches!(effects.as_slice(), [Effect::LoadCatalogChildren { replace_roots: false, parent: Some(_), .. }]));
}
```

- [ ] **Step 2: Executar e confirmar as falhas**

Run: `cargo test -p dexo-tui --test command_palette_flow savepoint && cargo test -p dexo-tui --test data_flow page_without && cargo test -p dexo-tui --test catalog_flow refresh`

Expected: FAIL.

- [ ] **Step 3: Implementar o prompt de savepoint**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SavepointIntent { Create, Rollback, Release }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionPrompt {
    pub open: bool,
    pub intent: Option<SavepointIntent>,
    pub name: String,
    pub error: Option<String>,
}
```

Enter valida `trim().is_empty()`, preserva o modal em erro e só então emite `Effect::Savepoint`, `RollbackToSavepoint` ou `ReleaseSavepoint` com o nome digitado. Remova todos os literais `"sp1"` do caminho de produção.

- [ ] **Step 4: Adicionar prompt próprio ao DataScreen**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataQueryIntent { Sort, Filter }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataQueryPrompt {
    pub open: bool,
    pub intent: Option<DataQueryIntent>,
    pub column: String,
    pub value: String,
    pub descending: bool,
    pub error: Option<String>,
}
```

Sort produz `Sort { column: ColumnId(column), descending }`. Filter produz `Filter::Eq(ColumnId(column), DbValue::Text(value))`. Ambos validam coluna existente em `model.data.table`, mantêm o prompt aberto em erro e chamam `apply_remote_query` somente após atualizar o estado.

- [ ] **Step 5: Validar antes de mutar paginação/loading**

```rust
fn change_data_page(model: &mut Model, offset: u64) -> Vec<Effect> {
    if model.active_session.is_none() {
        model.data.last_error = Some("connect a session first".into());
        return Vec::new();
    }
    if model.data.target.object().is_empty() {
        model.data.last_error = Some("open a table first".into());
        return Vec::new();
    }
    model.data.page_offset = offset;
    model.data.loading = true;
    let effects = reload_object_data(model);
    if effects.is_empty() { model.data.loading = false; }
    effects
}
```

- [ ] **Step 6: Separar refresh all de subtree e validar sessão antes de clear**

```rust
Action::RefreshCatalogSubtree => refresh_catalog(model, false),
Action::RefreshCatalogAll => refresh_catalog(model, true),

fn refresh_catalog(model: &mut Model, all: bool) -> Vec<Effect> {
    if model.active_session.is_none() {
        model.messages.push("connect a session to refresh the catalog".into());
        return Vec::new();
    }
    let operation = OperationId::new();
    if all {
        model.explorer.clear();
        return catalog_load_effect(model, None, operation, true);
    }
    let Some(id) = model.explorer.selected.clone() else { return Vec::new() };
    model.explorer.expand_with(&id, operation);
    catalog_load_effect(model, Some(id), operation, false)
}
```

- [ ] **Step 7: Rodar testes e commit**

Run: `cargo fmt --all && cargo test -p dexo-tui --test command_palette_flow savepoint && cargo test -p dexo-tui --test data_flow && cargo test -p dexo-tui --test catalog_flow`

Expected: PASS.

```bash
git add crates/dexo-tui/src/screens/transaction_prompt.rs crates/dexo-tui/src/screens/mod.rs crates/dexo-tui/src/screens/data.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-tui/tests/command_palette_flow.rs crates/dexo-tui/tests/data_flow.rs crates/dexo-tui/tests/catalog_flow.rs
git commit -m "fix(tui): validate transaction data and explorer commands"
```

### Task 5: Separar Export, Import, Backup e Restore e remover I/O do reducer

**Files:**
- Modify: `crates/dexo-tui/src/model.rs:123-170`
- Modify: `crates/dexo-tui/src/action.rs:520-546`
- Modify: `crates/dexo-tui/src/screens/transfer.rs`
- Modify: `crates/dexo-tui/src/screens/file_picker.rs:3-21`
- Modify: `crates/dexo-tui/src/update.rs:754-767,1369-1393,2988-3025,3079-3111`
- Modify: `crates/dexo-tui/src/runtime/transfer_manager.rs`
- Modify: `crates/dexo-tui/src/runtime/session_registry.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs:125-149,160-330`
- Modify: `crates/dexo-app/src/transfer/native_tool.rs`
- Modify: `crates/dexo-tui/tests/schema_transfer_explain_flow.rs`

- [ ] **Step 1: Escrever testes de segurança que falham no runner atual**

```rust
#[test]
fn every_transfer_palette_command_opens_its_own_mode() {
    for (id, expected) in [
        ("transfer.export", TransferMode::Export),
        ("transfer.import", TransferMode::Import),
        ("backup.dump", TransferMode::Backup),
        ("backup.restore", TransferMode::Restore),
    ] {
        let mut model = transfer_ready_model();
        choose(&mut model, id);
        assert_eq!(model.transfer.mode, expected);
    }
}

#[tokio::test]
async fn import_and_restore_never_write_to_the_source_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.dump");
    std::fs::write(&source, b"ORIGINAL").unwrap();
    let runtime = recording_transfer_runtime();
    runtime.run(TransferRequest::restore(source.clone(), session_id())).await.unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b"ORIGINAL");
    assert_eq!(runtime.recorded_modes(), vec![TransferMode::Restore]);
}
```

- [ ] **Step 2: Executar o teste e confirmar que Import/Restore chegam a Export**

Run: `cargo test -p dexo-tui --test schema_transfer_explain_flow transfer -- --nocapture`

Expected: FAIL; o código atual chama `export_rows` para todos os modos.

- [ ] **Step 3: Tipar modo, request e resultados**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferMode { Export, Import, Backup, Restore }

// Substitua TransferScreen.mode: &'static str e acrescente estes campos:
pub mode: TransferMode,
pub operation: Option<OperationId>,
pub error: Option<String>,
pub message: Option<String>,
pub confirm_restore: bool,

#[derive(Clone, Debug)]
pub enum TransferRequest {
    Export {
        operation: OperationId,
        path: PathBuf,
        format: dexo_app::transfer::TransferFormat,
        columns: Vec<String>,
        rows: std::sync::Arc<Vec<Vec<DbValue>>>,
    },
    Import {
        operation: OperationId,
        path: PathBuf,
        format: dexo_app::transfer::TransferFormat,
        target: dexo_driver_api::QualifiedName,
        strategy: dexo_app::transfer::ErrorStrategy,
        session: SessionId,
    },
    Backup { operation: OperationId, path: PathBuf, session: SessionId },
    Restore { operation: OperationId, path: PathBuf, session: SessionId },
}

// Variants a adicionar a Action:
TransferProgress { operation: OperationId, rows: u64, bytes: u64 },
TransferFinished { operation: OperationId, message: String },
TransferFailed { operation: OperationId, message: String },

// Variant que substitui o RunTransfer { path, mode } atual em Effect:
RunTransfer(TransferRequest),
```

- [ ] **Step 4: Tornar o snapshot de rows barato**

Mude `ResultBuffer.rows` para `Arc<Vec<Vec<DbValue>>>`, inicialize-o com `Arc::default()`, use `Arc::make_mut` em `append_rows`/`clear` e exponha:

```rust
pub fn rows_snapshot(&self) -> Arc<Vec<Vec<DbValue>>> {
    Arc::clone(&self.rows)
}
```

Isso evita `rows().to_vec()` no reducer sem reestruturar a grade inteira. Como `Arc::new` não é const, mude `EMPTY_GRID` para `static EMPTY_GRID: std::sync::LazyLock<GridModel> = std::sync::LazyLock::new(GridModel::default);` e, em `ResultsState::grid`, use `.unwrap_or_else(|| &*EMPTY_GRID)`.

- [ ] **Step 5: Fazer `run_transfer` apenas validar e emitir Effect**

```rust
fn run_transfer(model: &mut Model) -> Vec<Effect> {
    let path = PathBuf::from(model.transfer.path.trim());
    if path.as_os_str().is_empty() {
        model.file_picker.open = true;
        model.file_picker_mode = FilePickerMode::Transfer;
        model.file_picker.refresh();
        return Vec::new();
    }
    match build_transfer_request(model, path) {
        Ok(request) => {
            model.transfer.running = true;
            model.transfer.error = None;
            vec![Effect::RunTransfer(request)]
        }
        Err(message) => {
            model.transfer.error = Some(message);
            Vec::new()
        }
    }
}
```

`build_transfer_request(&Model, PathBuf)` é uma função livre e pura em `update.rs`; ela exige resultados para Export, sessão+target para Import, sessão para Backup/Restore e confirmação visível para Restore. Mantê-la fora de `TransferScreen` evita emprestar `model.transfer` e `model` ao mesmo tempo.

- [ ] **Step 6: Implementar dispatch real por variant no manager**

```rust
pub async fn run(&mut self, request: TransferRequest, runtime: &RuntimeAccess) {
    match request {
        TransferRequest::Export { operation, path, format, columns, rows } => {
            run_export(operation, path, format, columns, rows, runtime).await;
        }
        TransferRequest::Import { operation, path, format, target, strategy, session } => {
            run_import(operation, path, format, target, strategy, session, runtime).await;
        }
        TransferRequest::Backup { operation, path, session } => {
            run_native(operation, path, session, TransferMode::Backup, runtime).await;
        }
        TransferRequest::Restore { operation, path, session } => {
            run_native(operation, path, session, TransferMode::Restore, runtime).await;
        }
    }
}
```

Export usa `spawn_blocking(export_rows)`. Import lê/decodifica o arquivo em worker bloqueante e chama `session.bulk().insert_batch` via `import_rows`. Backup/Restore obtêm profile e segredo da sessão ativa e usam `NativeToolRunner<TokioProcessRunner>`.

- [ ] **Step 7: Corrigir argumentos do native tool**

Substitua host, porta, usuário e `backup.dump` hard-coded por uma request explícita:

```rust
pub struct NativeToolRequest {
    pub kind: NativeToolKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub path: PathBuf,
    pub secret: secrecy::SecretString,
    pub expected_major: u32,
}
```

`run_native` resolve o profile da sessão e seleciona exatamente `PgDump`/`MysqlDump` para `Backup` e `PgRestore`/`MysqlRestore` para `Restore`; outro driver retorna erro visível. Estenda `ProcessSpec` com `stdin: Option<PathBuf>` e `stdout: Option<PathBuf>`. PgDump recebe `--host`, `--port`, `--username`, `--file` seguido de `request.path` e database; PgRestore recebe os mesmos dados e `request.path` como origem. MySQLDump define `stdout` para arquivo temporário irmão e persiste atomicamente após sucesso; MySQLRestore define `stdin` como o arquivo de origem. Password nunca entra em argv/log.

- [ ] **Step 8: Correlacionar progresso, erro e cancelamento**

`TransferManager` mantém `HashMap<OperationId, Arc<AtomicBool>>` para export/import e handles nativos para backup/restore. `CancelOperation` tenta cancelar transfer antes do query runner. Toda saída remove o handle e emite exatamente um de `TransferFinished`, `TransferFailed` ou `OperationCancelled`.

```rust
pub enum RunningTransfer {
    Cooperative(std::sync::Arc<std::sync::atomic::AtomicBool>),
    Native(dexo_app::transfer::NativeHandle),
}

#[derive(Default)]
pub struct TransferManager {
    running: std::collections::HashMap<OperationId, RunningTransfer>,
}

impl TransferManager {
    pub async fn cancel(&mut self, operation: OperationId) -> bool {
        match self.running.remove(&operation) {
            Some(RunningTransfer::Cooperative(token)) => {
                token.store(true, std::sync::atomic::Ordering::Release);
                true
            }
            Some(RunningTransfer::Native(handle)) => handle.cancel().await.is_ok(),
            None => false,
        }
    }
}
```

No reducer, aceite `TransferProgress`, `TransferFinished` e `TransferFailed` somente quando `model.transfer.operation == Some(operation)`; eventos stale retornam `Vec::new()` sem alterar a screen. Finish/Failed definem `running = false`; Failed preserva o modal e grava `error`.

- [ ] **Step 9: Rodar testes de perda de dados, cancelamento e streaming**

Run: `cargo test -p dexo-app transfer && cargo test -p dexo-tui --test schema_transfer_explain_flow transfer`

Expected: PASS; source de Import/Restore permanece byte a byte intacta.

- [ ] **Step 10: Commit**

```bash
git add crates/dexo-tui/src/model.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/screens/transfer.rs crates/dexo-tui/src/screens/file_picker.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/runtime/transfer_manager.rs crates/dexo-tui/src/runtime/session_registry.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-app/src/transfer/native_tool.rs crates/dexo-tui/tests/schema_transfer_explain_flow.rs
git commit -m "fix(transfer): dispatch import export backup and restore safely"
```

### Task 6: Ligar Schema Diff e Security aos managers reais

**Files:**
- Modify: `crates/dexo-tui/src/action.rs:238-249,509-519`
- Modify: `crates/dexo-tui/src/screens/schema_diff.rs`
- Modify: `crates/dexo-tui/src/screens/security.rs`
- Modify: `crates/dexo-tui/src/runtime/schema_manager.rs:168-294,329-380`
- Modify: `crates/dexo-tui/src/runtime/mod.rs:203-234`
- Modify: `crates/dexo-tui/src/update.rs:709-753,1268-1367`
- Modify: `crates/dexo-tui/src/render.rs:55-91`
- Modify: `crates/dexo-tui/tests/schema_transfer_explain_flow.rs`

- [ ] **Step 1: Escrever testes falhos para tela carregada e teclado de Security**

```rust
#[test]
fn schema_diff_command_starts_loading_instead_of_opening_empty_default() {
    let mut model = connected_model();
    choose(&mut model, "schema.diff");
    assert!(model.schema_diff.open);
    assert!(model.schema_diff.source_prompt);
    assert!(model.schema_diff.entries.is_empty());
}

#[test]
fn security_loads_and_closes_with_escape() {
    let mut model = connected_model();
    let effects = choose_effects(&mut model, "schema.security");
    assert!(model.security.open);
    assert!(matches!(effects.as_slice(), [Effect::LoadSecurity { .. }]));
    press(&mut model, KeyCode::Esc);
    assert!(!model.security.open);
}
```

- [ ] **Step 2: Executar e confirmar as falhas**

Run: `cargo test -p dexo-tui --test schema_transfer_explain_flow schema_diff_command security_loads -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Adicionar protocolo de schema diff**

```rust
// Variant a adicionar a Effect:
LoadSchemaDiff {
    session: SessionId,
    left: dexo_app::schema_diff::DiffSource,
    right: dexo_app::schema_diff::DiffSource,
    generation: u64,
},

// Variants a adicionar a Action:
SchemaDiffLoaded {
    from_label: String,
    to_label: String,
    ordered: Vec<dexo_app::schema_diff::OrderedChange>,
},
SchemaDiffFailed { message: String },
```

`OpenFlow(SchemaDiff)` abre seleção de duas fontes. Só após ambas existirem emite `LoadSchemaDiff`. O runtime chama `SchemaManager::diff`, envia `SchemaDiffLoaded`, e o reducer usa `SchemaDiffScreen::from_ordered`.

- [ ] **Step 4: Persistir loading/error/source no screen**

Acrescente `source_prompt`, `left`, `right`, `loading` e `error`. Enter sem duas fontes preserva a tela e mostra `select both schema sources`; Esc sempre fecha. Apply continua bloqueado sem confirmação e sem ordered changes carregadas.

```rust
pub source_prompt: bool,
pub left: Option<dexo_app::schema_diff::DiffSource>,
pub right: Option<dexo_app::schema_diff::DiffSource>,
pub loading: bool,
pub error: Option<String>,

fn request_schema_diff(model: &mut Model) -> Vec<Effect> {
    let (Some(left), Some(right), Some(session)) = (
        model.schema_diff.left.clone(),
        model.schema_diff.right.clone(),
        model.active_session,
    ) else {
        model.schema_diff.error = Some("select both schema sources".into());
        return Vec::new();
    };
    model.schema_diff.loading = true;
    model.schema_diff.error = None;
    vec![Effect::LoadSchemaDiff {
        session,
        left,
        right,
        generation: model.session_generation,
    }]
}
```

- [ ] **Step 5: Adicionar protocolo de Security**

```rust
// Variant a adicionar a Effect:
LoadSecurity { session: SessionId, generation: u64 },

// Variants a adicionar a Action:
SecurityLoaded { principals: Vec<String>, grants: Vec<GrantRecord> },
SecurityFailed { message: String },
```

O runtime chama `session.security().list_grants(None)`, deriva `principals` únicos dos grants e emite indisponibilidade se o driver não oferecer `SecurityAdmin`. Create role/grant select produzem `SchemaChange` e entram no fluxo DDL protegido já existente.

```rust
impl SecurityScreen {
    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.principals.len() {
            self.selected += 1;
        }
    }
}
```

- [ ] **Step 6: Adicionar branch de teclado antes do keymap global**

```rust
if model.security.open {
    return match key.code {
        KeyCode::Esc => { model.security.open = false; Vec::new() }
        KeyCode::Up => { model.security.select_previous(); Vec::new() }
        KeyCode::Down => { model.security.select_next(); Vec::new() }
        KeyCode::Enter => open_security_change_preview(model),
        _ => Vec::new(),
    };
}
```

- [ ] **Step 7: Rodar e commit**

Run: `cargo fmt --all && cargo test -p dexo-tui --test schema_transfer_explain_flow schema && cargo test -p dexo-tui --lib screens::security::tests`

Expected: PASS.

```bash
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/screens/schema_diff.rs crates/dexo-tui/src/screens/security.rs crates/dexo-tui/src/runtime/schema_manager.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-tui/tests/schema_transfer_explain_flow.rs
git commit -m "fix(tui): load schema diff and security flows"
```

### Task 7: Corrigir cursor do Explain, dialeto, snippets, parâmetros e histórico

**Files:**
- Modify: `crates/dexo-tui/src/action.rs:313-323,520-525`
- Modify: `crates/dexo-tui/src/screens/editor.rs:17-38,121-220,255-276`
- Modify: `crates/dexo-tui/src/runtime/explain_manager.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs:235-250,279-285`
- Modify: `crates/dexo-tui/src/update.rs:768-787,906-955,1268-1311`
- Modify: `crates/dexo-tui/tests/editor_flow.rs`
- Modify: `crates/dexo-tui/tests/schema_transfer_explain_flow.rs`

- [ ] **Step 1: Escrever regressões pelo caminho integrado**

```rust
#[test]
fn explain_effect_carries_second_statement_cursor() {
    let mut model = connected_model_with_sql("select 1;\nselect 2;");
    model.active_document_mut().sql.set_cursor("select 1;\nselect ".chars().count());
    let effects = update(&mut model, Action::OpenExplain);
    assert!(matches!(effects.as_slice(), [Effect::RunExplain { cursor, .. }] if *cursor > 0));
}

#[test]
fn insert_snippet_loads_storage_before_opening_picker() {
    let mut model = Model::default();
    let effects = choose_effects(&mut model, "editor.snippet");
    assert!(model.editor.snippet_pending);
    assert!(matches!(effects.as_slice(), [Effect::LoadSnippets]));
}

#[test]
fn submit_parameters_outside_prompt_never_executes_query() {
    let mut model = connected_model_with_sql("select 1");
    let effects = update(&mut model, Action::SubmitParameters);
    assert!(effects.is_empty());
    assert!(model.active_operation.is_none());
}
```

- [ ] **Step 2: Executar e confirmar falhas**

Run: `cargo test -p dexo-tui --test schema_transfer_explain_flow explain_effect && cargo test -p dexo-tui --test editor_flow snippet parameters`

Expected: FAIL.

- [ ] **Step 3: Propagar cursor em bytes até `run_live`**

```rust
// Substituição do variant RunExplain atual em Effect:
RunExplain {
    sql: String,
    cursor: usize,
    analyze: bool,
    session: SessionId,
    generation: u64,
},

fn explain_effect(model: &Model, analyze: bool) -> Vec<Effect> {
    let Some(session) = model.active_session else { return Vec::new() };
    let document = model.active_document();
    let sql = document.text();
    let cursor = sql.chars().take(document.cursor()).map(char::len_utf8).sum();
    vec![Effect::RunExplain { sql, cursor, analyze, session, generation: model.session_generation }]
}
```

No runtime, passe `cursor` para `explain_manager::run_live`; remova o literal `0`.

- [ ] **Step 4: Selecionar dialeto pela conexão ativa**

Acrescente `driver: String` a `ConnectionStatus` e à `Action::ConnectionChanged`. Preencha com `profile.driver` no runtime. Em `editor.rs`:

```rust
fn editor_dialect(model: &Model) -> Dialect {
    if model.connection.driver == "mysql" { Dialect::MySql } else { Dialect::Postgres }
}
```

Use essa função em format e completion; não deixe `Dialect::Postgres` hard-coded nesses caminhos.

- [ ] **Step 5: Carregar snippets sob demanda e continuar a intenção**

Adicione `snippet_pending: bool`. `OpenFlow(InsertSnippet)` emite `LoadSnippets` quando a lista está vazia; `SnippetsLoaded` limpa pending e abre o picker para seleção. Se storage devolver zero itens, mostre `no snippets available` e não abra overlay vazio.

```rust
fn open_snippets(model: &mut Model) -> Vec<Effect> {
    if model.editor.snippets.is_empty() {
        model.editor.snippet_pending = true;
        return vec![Effect::LoadSnippets];
    }
    model.editor.snippet_open = true;
    Vec::new()
}

Action::SnippetsLoaded(snippets) => {
    model.editor.snippet_pending = false;
    model.editor.snippets = snippets;
    model.editor.snippet_open = !model.editor.snippets.is_empty();
    if model.editor.snippets.is_empty() {
        model.messages.push("no snippets available".into());
    }
    Vec::new()
}
```

- [ ] **Step 6: Separar abrir parâmetros de submeter parâmetros**

`OpenFlow(SubmitParameters)` executa `refresh_intelligence`, verifica parâmetros e abre `parameter_prompt` no índice do primeiro valor nulo. `Action::SubmitParameters` retorna vazio quando `parameter_prompt == false`; somente o handler do prompt chama `start_query` após o último parâmetro válido.

```rust
fn open_parameters(model: &mut Model) -> Vec<Effect> {
    crate::screens::editor::refresh_intelligence(model, false);
    if model.editor.parameters.is_empty() {
        model.messages.push("no query parameters".into());
        return Vec::new();
    }
    model.editor.parameter_index = 0;
    model.editor.parameter_draft.clear();
    model.editor.parameter_prompt = true;
    Vec::new()
}

fn submit_parameter_prompt(model: &mut Model) -> Vec<Effect> {
    if !model.editor.parameter_prompt {
        return Vec::new();
    }
    crate::screens::editor::submit_parameters(model);
    if model.editor.parameter_prompt { Vec::new() } else { start_query(model) }
}

Action::SubmitParameters => submit_parameter_prompt(model),
```

- [ ] **Step 7: Tornar clear history uma confirmação visível e coerente**

Adicione `history_confirm_clear: bool` e renderize `clear history for <connection|all>?`. O palette abre History com essa confirmação; Enter emite `ClearHistory` usando o mesmo `connection_id` da busca. Esc apenas cancela.

```rust
fn open_clear_history(model: &mut Model) -> Vec<Effect> {
    model.editor.history_open = true;
    model.editor.history_confirm_clear = true;
    Vec::new()
}

fn confirm_clear_history(model: &mut Model) -> Vec<Effect> {
    let connection_id = model.connection.name.clone();
    model.editor.history_confirm_clear = false;
    vec![Effect::ClearHistory { connection_id }]
}
```

- [ ] **Step 8: Rodar e commit**

Run: `cargo fmt --all && cargo test -p dexo-tui --test editor_flow && cargo test -p dexo-tui --test schema_transfer_explain_flow explain`

Expected: PASS.

```bash
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/screens/editor.rs crates/dexo-tui/src/runtime/explain_manager.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/editor_flow.rs crates/dexo-tui/tests/schema_transfer_explain_flow.rs
git commit -m "fix(tui): connect editor and explain palette flows"
```

### Task 8: Corrigir confirmações locais, revoke all e export de diagnostics

**Files:**
- Create: `crates/dexo-tui/src/screens/diagnostics.rs`
- Modify: `crates/dexo-tui/src/screens/mod.rs`
- Modify: `crates/dexo-tui/src/model.rs:937-964`
- Modify: `crates/dexo-tui/src/action.rs:262-271,321-329,534-546`
- Modify: `crates/dexo-tui/src/screens/settings.rs`
- Modify: `crates/dexo-tui/src/screens/recovery.rs`
- Modify: `crates/dexo-tui/src/screens/mcp_profiles.rs`
- Modify: `crates/dexo-tui/src/screens/mcp_audit.rs`
- Modify: `crates/dexo-tui/src/screens/file_picker.rs`
- Modify: `crates/dexo-tui/src/runtime/diagnostic_manager.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs:268-285,1155-1233`
- Modify: `crates/dexo-tui/src/update.rs:827-905,1409-1455,3052-3111`
- Modify: `crates/dexo-tui/src/render.rs:104-222`
- Modify: `crates/dexo-storage/src/mcp/grant_repo.rs:72-86`
- Modify: `crates/dexo-tui/tests/admin_settings_mcp_flow.rs`

- [ ] **Step 1: Escrever testes falhos das confirmações visíveis**

```rust
#[test]
fn destructive_local_commands_open_their_owner_before_confirmation() {
    for (id, visible) in [
        ("settings.reset", "confirm_reset=true"),
        ("recovery.discard", "confirm_discard=true"),
        ("mcp.revoke_all", "confirm revoke all grants"),
    ] {
        let mut model = model_with_local_state();
        choose(&mut model, id);
        let view = dexo_tui::render::render_to_string(&model, 100, 30);
        assert!(view.contains(visible), "{id} confirmation is hidden");
    }
}

#[test]
fn diagnostics_command_opens_preview_and_destination_flow() {
    let mut model = Model::default();
    choose(&mut model, "diagnostics.export");
    assert!(model.diagnostics.open);
    assert!(model.diagnostics.preview.contains("Dexo never uploads"));
    assert!(!model.diagnostics.writing);
}
```

- [ ] **Step 2: Executar e confirmar falhas**

Run: `cargo test -p dexo-tui --test admin_settings_mcp_flow destructive_local diagnostics`

Expected: FAIL.

- [ ] **Step 3: Abrir Settings e Recovery já no estado de confirmação**

`OpenFlow(SettingsReset)` abre Settings e define `confirm_reset = true`; Enter reseta/persiste, Esc cancela. `OpenFlow(RecoveryRestore)` abre Recovery com checkpoints visíveis; `OpenFlow(RecoveryDiscard)` abre Recovery e define `confirm_discard = true`. Remova o requisito de selecionar o mesmo comando duas vezes fora do overlay.

```rust
FlowIntent::SettingsReset => {
    model.settings.open = true;
    model.settings.confirm_reset = true;
    Vec::new()
}
FlowIntent::RecoveryRestore => {
    model.recovery.open = true;
    model.recovery.confirm_discard = false;
    Vec::new()
}
FlowIntent::RecoveryDiscard => {
    model.recovery.open = true;
    model.recovery.confirm_discard = true;
    Vec::new()
}
```

- [ ] **Step 4: Carregar todos os perfis MCP**

Substitua o payload de primeiro perfil por:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct McpProfileSummary {
    pub name: String,
    pub enabled: bool,
    pub scopes: Vec<String>,
    pub tools: Vec<String>,
}

Action::McpProfilesLoaded { profiles: Vec<McpProfileSummary> }
```

`McpProfilesScreen` mantém `profiles` e `selected`; Up/Down navegam. Perfil vazio mostra `no MCP profiles`, nunca nome vazio.

- [ ] **Step 5: Implementar revoke realmente global**

```rust
pub fn revoke_all(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute(
        "UPDATE mcp_grants SET remaining_uses = 0, revoked = 1, revision = revision + 1 WHERE revoked = 0",
        [],
    ).map_err(Into::into)
}
```

Adicione `Effect::RevokeAllMcpGrants` e `Action::McpGrantsRevoked { count }`. O runtime chama `revoke_all`, recarrega audit e mostra a contagem. A confirmação permanece aberta em erro.

- [ ] **Step 6: Criar estado e protocolo de diagnostics**

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagnosticsScreen {
    pub open: bool,
    pub preview: String,
    pub path: Option<PathBuf>,
    pub writing: bool,
    pub error: Option<String>,
}

// Variant a adicionar a Effect:
WriteDiagnostics { path: PathBuf, bundle: dexo_app::diagnostic_service::DiagnosticBundle },

// Variants a adicionar a Action:
DiagnosticsWritten { path: PathBuf },
DiagnosticsFailed { message: String },
```

O preview deve incluir a frase de privacidade antes do file picker. Adicione `FilePickerMode::Diagnostics`. Após escolher destino, emita `WriteDiagnostics`; o runtime executa `bundle.write_zip(&path)` em `spawn_blocking`.

- [ ] **Step 7: Fechar diagnostics e Security antes do keymap global**

Esc fecha e limpa apenas o estado transitório; Enter em diagnostics abre o picker ou tenta novamente. Falha preserva preview/path e mostra `error`. Sucesso mantém o caminho final visível até Esc.

```rust
fn open_diagnostics_picker(model: &mut Model) -> Vec<Effect> {
    model.file_picker.open = true;
    model.file_picker_mode = FilePickerMode::Diagnostics;
    model.file_picker.refresh();
    Vec::new()
}

if model.diagnostics.open {
    return match key.code {
        KeyCode::Esc => {
            model.diagnostics.open = false;
            model.diagnostics.writing = false;
            Vec::new()
        }
        KeyCode::Enter if !model.diagnostics.writing => open_diagnostics_picker(model),
        _ => Vec::new(),
    };
}
if model.security.open {
    return match key.code {
        KeyCode::Esc => { model.security.open = false; Vec::new() }
        KeyCode::Up => { model.security.select_previous(); Vec::new() }
        KeyCode::Down => { model.security.select_next(); Vec::new() }
        KeyCode::Enter => open_security_change_preview(model),
        _ => Vec::new(),
    };
}
```

- [ ] **Step 8: Rodar testes de storage/sentinela/UI**

Run: `cargo test -p dexo-storage mcp && cargo test -p dexo-app diagnostic && cargo test -p dexo-tui --test admin_settings_mcp_flow`

Expected: PASS; o sentinel de segredo não aparece no ZIP.

- [ ] **Step 9: Commit**

```bash
git add crates/dexo-tui/src/screens/diagnostics.rs crates/dexo-tui/src/screens/mod.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/screens/settings.rs crates/dexo-tui/src/screens/recovery.rs crates/dexo-tui/src/screens/mcp_profiles.rs crates/dexo-tui/src/screens/mcp_audit.rs crates/dexo-tui/src/screens/file_picker.rs crates/dexo-tui/src/runtime/diagnostic_manager.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/render.rs crates/dexo-storage/src/mcp/grant_repo.rs crates/dexo-tui/tests/admin_settings_mcp_flow.rs
git commit -m "fix(tui): make destructive and diagnostic flows visible"
```

### Task 9: Provar os 129 contratos, limpar falsos positivos e executar o gate

**Files:**
- Modify: `crates/dexo-tui/tests/command_palette_contract.rs`
- Modify: `crates/dexo-tui/tests/command_palette_flow.rs`
- Modify: `crates/dexo-tui/tests/snapshots.rs`
- Modify: `docs/testing/release-checklist.md`
- Modify: `docs/audits/2026-08-16-command-palette-audit.md`

- [ ] **Step 1: Provar cada requisito declarado no registro**

```rust
#[test]
fn every_context_command_has_a_reason_then_becomes_actionable() {
    for id in COMMAND_IDS {
        let requirements = command_spec(id).unwrap().requirements;
        let ready_model = model_satisfying(requirements);
        let ready = palette_entries(&ready_model)
            .into_iter().find(|entry| entry.id == id).unwrap();
        assert!(ready.disabled_reason.is_none(), "{id}");

        for requirement in requirements {
            let blocked_model = model_missing(requirements, *requirement);
            let blocked = palette_entries(&blocked_model)
                .into_iter().find(|entry| entry.id == id).unwrap();
            assert_eq!(
                blocked.disabled_reason.as_deref(),
                Some(requirement.reason()),
                "{id} did not explain {requirement:?}",
            );
        }
    }
}

fn model_satisfying(requirements: &[Requirement]) -> Model {
    let mut model = Model::default();
    for requirement in requirements {
        satisfy(&mut model, *requirement);
    }
    model
}

fn satisfy(model: &mut Model, requirement: Requirement) {
    use Requirement::*;
    match requirement {
        ActiveSession => {
            model.active_session = Some(SessionId(uuid::Uuid::from_u128(1)));
            model.session_generation = 1;
        }
        Results => model.results.append_rows(vec![vec![DbValue::I64(1)]]),
        RowSelection => {
            if model.results.rows().is_empty() {
                model.results.append_rows(vec![vec![DbValue::I64(1)]]);
            }
            model.results.select_cell(0, 0);
        }
        ExplorerNode => model.explorer.selected = Some(ObjectId::new("table:items")),
        LoadedDdl => model.inspector.ddl = Some("create table items(id bigint)".into()),
        PendingChanges => {
            model.data.table = TableMeta {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    primary_key: true,
                    unique: true,
                    nullable: false,
                }],
            };
            model.data.changes = ChangeSet::for_table(&model.data.table);
            model.data.changes.insert(vec![("id".into(), DbValue::I64(1))]);
        }
        Breadcrumb => model.data.crumbs.push((model.data.target.clone(), None, 0)),
        ActiveQuery => model.active_operation = Some(OperationId::new()),
        Completion => {
            model.set_sql("sel");
            dexo_tui::screens::editor::refresh_intelligence(model, true);
        }
        Parameters => {
            model.set_sql("select :id");
            dexo_tui::screens::editor::refresh_intelligence(model, false);
        }
        History => model.editor.history.push("select 1".into()),
        Recovery => model.recovery.checkpoints.push(("doc".into(), "now".into(), "select 1".into())),
    }
}

fn model_missing(requirements: &[Requirement], missing: Requirement) -> Model {
    let mut model = model_satisfying(requirements);
    match missing {
        Requirement::ActiveSession => model.active_session = None,
        Requirement::Results => model.results.clear(),
        Requirement::RowSelection => model.results.select_column(0),
        Requirement::ExplorerNode => model.explorer.selected = None,
        Requirement::LoadedDdl => model.inspector.ddl = None,
        Requirement::PendingChanges => model.data.changes = ChangeSet::for_table(&model.data.table),
        Requirement::Breadcrumb => model.data.crumbs.clear(),
        Requirement::ActiveQuery => model.active_operation = None,
        Requirement::Completion => model.editor.completions.clear(),
        Requirement::Parameters => model.editor.parameters.clear(),
        Requirement::History => model.editor.history.clear(),
        Requirement::Recovery => model.recovery.checkpoints.clear(),
    }
    model
}
```

Importe no teste `ChangeSet`, `ColumnDef`, `DbValue`, `ObjectId`, `OperationId`, `Requirement`, `SessionId` e `TableMeta`. O teste percorre os 129 `COMMAND_IDS`, portanto um comando contextual sem `requirements` falha no contrato em vez de escapar de uma lista manual de exemplos.

- [ ] **Step 2: Adicionar um smoke real para cada ID**

Para cada entrada de `COMMAND_IDS`, abra o palette, digite o ID e pressione Enter. O teste aceita exatamente um destes resultados:

```rust
enum ObservedOutcome {
    Effects,
    VisibleFlow,
    VisibleDisabledReason,
    DirectStateChange,
}

fn observe_command(id: &str) -> ObservedOutcome {
    let spec = command_spec(id).unwrap();
    let mut model = model_satisfying(spec.requirements);
    let before = model.clone();
    let before_view = dexo_tui::render::render_to_string(&before, 100, 30);
    let effects = choose_effects(&mut model, id);
    let after_view = dexo_tui::render::render_to_string(&model, 100, 30);

    if !effects.is_empty() {
        return ObservedOutcome::Effects;
    }
    if model.palette.open && !model.messages.is_empty() {
        return ObservedOutcome::VisibleDisabledReason;
    }
    if matches!(spec.invocation, PaletteInvocation::OpenFlow(_)) && before_view != after_view {
        return ObservedOutcome::VisibleFlow;
    }

    let mut normalized = model;
    normalized.palette = before.palette.clone();
    normalized.focus = before.focus;
    assert_ne!(normalized, before, "{id} closed the palette as a silent no-op");
    ObservedOutcome::DirectStateChange
}

#[test]
fn every_palette_id_has_an_observable_outcome() {
    for id in COMMAND_IDS {
        let spec = command_spec(id).unwrap();
        let observed = observe_command(id);
        match spec.invocation {
            PaletteInvocation::OpenFlow(_) => assert!(
                matches!(observed, ObservedOutcome::Effects | ObservedOutcome::VisibleFlow),
                "{id} did not open or start its declared flow",
            ),
            PaletteInvocation::Dispatch(_) => assert!(!matches!(
                observed,
                ObservedOutcome::VisibleDisabledReason
            ), "ready command {id} remained disabled"),
        }
    }
}
```

Falhe se o palette fechar sem effect, flow visível ou mudança direta prevista. Compare o resultado com o tipo `Dispatch/OpenFlow` da tabela, não apenas com “não panicou”.

- [ ] **Step 3: Executar buscas sentinela contra as causas raiz**

Run:

```bash
rg -n 'name:\s*String::new\(\)|"sp1"' crates/dexo-tui/src/palette.rs crates/dexo-tui/src/palette
rg -n 'export_rows|rows\(\)\.to_vec\(\)' crates/dexo-tui/src/update.rs
rg -n 'run_live\([\s\S]*,\s*0,\s*' crates/dexo-tui/src/runtime/mod.rs
```

Expected: nenhum match.

- [ ] **Step 4: Rodar toda a suíte TUI e revisar snapshots individualmente**

Run: `cargo test -p dexo-tui --no-fail-fast`

Expected: PASS. Se `.snap.new` aparecer, compare cada arquivo com a screen alterada; só substitua o snapshot quando a diferença mostrar atalho, confirmação, erro ou estado aprovado nesta spec. Uma divergência sem explicação comportamental deve ser corrigida no código.

- [ ] **Step 5: Rodar formatação e lint do workspace**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS sem warnings.

- [ ] **Step 6: Rodar o gate completo**

Run: `cargo test --workspace --all-features --no-fail-fast`

Expected: PASS.

- [ ] **Step 7: Atualizar documentação somente após o gate verde**

No relatório de auditoria, acrescente esta seção somente depois dos gates verdes. No release checklist, marque Command Palette como pass somente se atalhos, flows contextuais, confirmação destrutiva e teste real estiverem verdes.

```markdown
## Remediação verificada

- Contratos: 129/129 IDs únicos, todos classificados como `Dispatch` ou `OpenFlow`.
- Fluxos: atalhos, foco, entradas visíveis e confirmações destrutivas verificados pelo caminho real do palette.
- Segurança: Import e Restore preservam o arquivo de origem byte a byte.
- Gates executados: TUI, clippy e workspace completo passaram sem falhas.
```

- [ ] **Step 8: Commit final**

```bash
git add crates/dexo-tui/tests/command_palette_contract.rs crates/dexo-tui/tests/command_palette_flow.rs crates/dexo-tui/tests/snapshots.rs docs/testing/release-checklist.md docs/audits/2026-08-16-command-palette-audit.md
git commit -m "test(tui): verify every command palette contract"
```

## Matriz de aceite por classificação original

- **46 adequados:** permanecem `Dispatch`, com regressão contra mudança de comportamento.
- **45 contextuais:** ficam bloqueados com motivo ou abrem chooser visível quando o próprio fluxo consegue obter o alvo.
- **11 parciais/enganosos:** título e comportamento passam a coincidir; não há placeholder ou sucesso sintético.
- **27 quebrados:** possuem teste de regressão pelo caminho real antes da correção.

## Definition of Done

- 129 IDs únicos e estáveis.
- 129 resultados classificados como `Dispatch` ou `OpenFlow`.
- Nenhum Enter habilitado termina em no-op silencioso.
- Nenhuma entrada obrigatória é fabricada.
- Nenhuma confirmação destrutiva fica escondida.
- Import/Restore preservam o arquivo de origem.
- Schema Diff, Security, Diagnostics, snippets e Explain alcançam os managers reais.
- Foco anterior e atalhos são visíveis e testados.
- TUI, clippy e workspace completos ficam verdes.
