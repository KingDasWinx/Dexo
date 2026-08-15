# Dexo Sprint 22: Admin, Settings, Recovery, MCP, Accessibility, and Completion Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete every remaining production path: live administration and security, durable settings, useful mouse/accessibility behavior, real crash recovery and diagnostics, a policy-enforced multi-connection MCP server, and cross-platform release evidence.

**Architecture:** Admin operations use the active session's advertised capabilities and explicit confirmations. Settings are a versioned local file applied through a runtime configuration snapshot. Recovery and diagnostics use the storage worker and atomic filesystem service. MCP stdio starts on demand with its own connection router, profile policy, expiring grant ledger, audit sink, and cancellation registry; it never depends on the TUI process or cloud services.

**Tech Stack:** Tokio, dexo-driver-api admin/security traits, serde and TOML, Ratatui/Crossterm mouse events, SQLite repositories, rmcp stdio server, OS keychain integration, tracing, Docker-backed PostgreSQL/MySQL tests, GitHub Actions multi-platform releases.

---

## File map

Create `crates/dexo-app/src/settings.rs`, `crates/dexo-tui/src/runtime/admin_manager.rs`, `settings_manager.rs`, `recovery_manager.rs`, `diagnostic_manager.rs`, `mouse.rs`, `crates/dexo-mcp/src/router.rs`, `limits.rs`, `audit.rs`, `cancellation.rs`, `crates/dexo-tui/tests/admin_settings_mcp_flow.rs`, `crates/dexo-mcp/tests/multi_connection.rs`, and `crates/dexo/tests/completion_live.rs`.

Modify both drivers' admin/security implementations, app admin/diagnostic/recovery/MCP modules, storage MCP/recovery repositories and exports, TUI action/model/update/event/render/theme/keymap/accessibility and affected screens, CLI MCP startup, MCP server/tools/resources/prompts/stdio, CI/release workflows, README, and user/security documentation.

Requires Sprints 16–21 green. This is the final implementation sprint; its exit gate covers the complete approved specification.

### Task 1: Load live sessions, locks, statistics, sizes, and variables

**Files:** `crates/dexo-driver-api/src/admin.rs`; `crates/dexo-driver-postgres/src/admin.rs`; `crates/dexo-driver-mysql/src/admin.rs`; create `crates/dexo-tui/src/runtime/admin_manager.rs`; modify `crates/dexo-tui/src/{action.rs,model.rs,update.rs,event.rs}`, `screens/admin.rs`; test `crates/dexo-tui/tests/admin_settings_mcp_flow.rs`.

- [ ] **Step 1: Write a failing correlated-refresh test**

```rust
#[tokio::test]
async fn admin_refresh_uses_selected_session_and_ignores_stale_response() {
    let mut harness = admin_harness_with_two_sessions();
    let first = harness.refresh(session_id(1), AdminView::Sessions);
    let second = harness.refresh(session_id(2), AdminView::Sessions);
    harness.complete(first, vec![session_info("old")]);
    harness.complete(second, vec![session_info("current")]);
    assert_eq!(harness.model().admin.sessions[0].id, "current");
}
```

- [ ] **Step 2: Run and verify fixture behavior fails**

Run: `cargo test -p dexo-tui --test admin_settings_mcp_flow admin_refresh_uses_selected_session_and_ignores_stale_response`

Expected: FAIL.

- [ ] **Step 3: Add typed admin requests and responses**

```rust
pub enum AdminView { Sessions, Locks, BlockingGraph, Statistics, Sizes(Page), Variables }

pub enum AdminAction {
    Loading { operation_id: OperationId, view: AdminView },
    Loaded { operation_id: OperationId, captured_at: DateTime<Utc>, page: AdminPage },
    Failed { operation_id: OperationId, error: UiError },
}
```

Store loading/error/unsupported state per tab. Poll only while the overlay is visible and not paused; cancel the poll token when session, tab, or overlay changes.

- [ ] **Step 4: Complete driver queries and capability reasons**

PostgreSQL must read `pg_stat_activity`, `pg_locks`, database/relation sizes, statistics, and settings. MySQL must use `performance_schema`/`information_schema` where available. Permission/version failures return `CapabilityState::Unavailable` with a concrete reason rather than empty data.

- [ ] **Step 5: Add sorting, paging, filtering, refresh interval, and capture timestamp**

All lists show the session/connection source and last successful capture. Preserve the last page while refreshing, and label it stale after an error.

- [ ] **Step 6: Test and commit**

Run: `cargo test -p dexo-driver-postgres admin -p dexo-driver-mysql admin -p dexo-tui --test admin_settings_mcp_flow admin`

Expected: PASS.

Commit: `feat(admin): load live database administration views`

### Task 2: Execute cancel, terminate, analyze, vacuum, reindex, and optimize safely

**Files:** `crates/dexo-app/src/admin_service.rs`; both driver `src/admin.rs`; `crates/dexo-tui/src/runtime/admin_manager.rs`, `screens/admin.rs`, action/model/update; test `admin_settings_mcp_flow.rs` and `crates/dexo/tests/completion_live.rs`.

- [ ] **Step 1: Test preview/confirmation/execution invariants**

```rust
#[tokio::test]
async fn terminate_requires_exact_backend_id_and_never_retries() {
    let op = harness().preview(AdminCommand::Terminate { backend_id: "42".into() }).await;
    assert!(op.execute(ConfirmationAnswer::Text("41".into())).await.is_err());
    op.execute(ConfirmationAnswer::Text("42".into())).await.unwrap();
    assert_eq!(op.driver_calls(), 1);
}
```

Add cancel-current-query, cancel-other-session, vacuum/analyze/reindex/optimize, permission failure, and disconnected-session tests.

- [ ] **Step 2: Run the focused failures**

Run: `cargo test -p dexo-tui --test admin_settings_mcp_flow admin_command`

Expected: FAIL.

- [ ] **Step 3: Route query cancellation correctly**

F5 query cancellation uses the query's driver `QueryId` and Sprint 16 cancellation registry. Admin cancel/terminate uses `AdministrationProvider` and a backend/session identifier. Keep these paths distinct in types and audit output.

- [ ] **Step 4: Execute maintenance as one-shot operations**

Render the exact driver preview, lock risk, expected transaction restrictions, target, and confirmation kind. Do not retry non-idempotent or administrative commands automatically. Refresh the relevant admin/catalog views after a confirmed outcome.

- [ ] **Step 5: Verify against both databases**

Run: `cargo test -p dexo --test completion_live admin_actions -- --ignored --test-threads=1`

Expected: PASS with Docker services; the test verifies observable server state and cancellation, not only returned text.

- [ ] **Step 6: Commit**

Commit: `feat(admin): execute protected administrative actions`

### Task 3: Manage real roles, grants, and effective privileges

**Files:** `crates/dexo-driver-api/src/ddl.rs`; both driver DDL/security modules; `crates/dexo-app/src/schema/{security.rs,change.rs}`; `crates/dexo-tui/src/runtime/schema_manager.rs`, `screens/security.rs`, action/model/update; test `admin_settings_mcp_flow.rs` and `completion_live.rs`.

- [ ] **Step 1: Add effective-privilege and grant-delta tests**

```rust
#[tokio::test]
async fn security_screen_distinguishes_direct_inherited_and_public_grants() {
    let view = load_security_view(session(), object()).await.unwrap();
    assert_eq!(view.privileges["SELECT"].source, PrivilegeSource::Inherited("analyst".into()));
    assert_eq!(view.privileges["INSERT"].source, PrivilegeSource::Direct);
}
```

- [ ] **Step 2: Extend `SecurityAdmin` with read contracts**

```rust
#[async_trait]
pub trait SecurityAdmin: Send + Sync {
    async fn roles(&self) -> Result<Vec<RoleInfo>, DriverError>;
    async fn grants(&self, target: QualifiedName) -> Result<Vec<GrantRecord>, DriverError>;
    async fn effective_privileges(&self, principal: &str, target: QualifiedName)
        -> Result<Vec<EffectivePrivilege>, DriverError>;
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, DriverError>;
}
```

- [ ] **Step 3: Implement PostgreSQL and MySQL semantics**

Preserve grantor, grantee, role membership, inheritance/source, grant option, object scope, and server limitations. When the server cannot prove a privilege source, label it `Unknown`; never infer effective access from role names.

- [ ] **Step 4: Wire review/apply through Sprint 21 DDL protection**

The overlay edits a change set, displays before/after grants and exact SQL, then delegates to `SchemaManager`. Create/drop role and privilege escalation require typed confirmation; successful apply reloads roles, grants, and catalog privilege inspector.

- [ ] **Step 5: Test and commit**

Run: `cargo test -p dexo-driver-postgres security -p dexo-driver-mysql security -p dexo-tui --test admin_settings_mcp_flow security`

Expected: PASS.

Commit: `feat(security): inspect and apply database privileges`

### Task 4: Persist and immediately apply settings, themes, and keymaps

**Files:** create `crates/dexo-app/src/settings.rs`, `crates/dexo-tui/src/runtime/settings_manager.rs`; modify `crates/dexo-app/src/lib.rs`, `crates/dexo-tui/src/{theme.rs,keymap.rs,terminal.rs,action.rs,model.rs,update.rs,render.rs}`, `screens/settings.rs`; test `admin_settings_mcp_flow.rs`.

- [ ] **Step 1: Test versioned round-trip and live application**

```rust
#[tokio::test]
async fn saved_theme_keymap_and_mouse_survive_restart() {
    let paths = temp_app_paths();
    save_settings(&paths, customized_settings()).await.unwrap();
    let restarted = bootstrap_with(paths).await.unwrap();
    assert_eq!(restarted.settings.theme, ThemeId::HighContrast);
    assert_eq!(restarted.keymap.binding(Command::RunStatement), keys("Ctrl+Enter"));
    assert!(!restarted.terminal.mouse_capture_enabled());
}
```

- [ ] **Step 2: Define a strict versioned local settings file**

```rust
#[derive(Serialize, Deserialize)]
pub struct SettingsFile {
    pub version: u32,
    pub theme: ThemeId,
    pub keymap: KeymapConfig,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: UnicodeMode,
    pub recovery_interval_secs: u64,
}
```

Load from the platform config directory, reject conflicting bindings with both command names, preserve the last valid settings on parse failure, and save atomically with a `.bak` recovery copy.

- [ ] **Step 3: Replace stringly typed settings state**

The settings overlay edits a draft, shows a semantic diff, validates, then emits `Effect::SaveSettings`. On `SettingsSaved`, atomically swap the runtime configuration and re-render; on failure retain the draft and prior active settings.

- [ ] **Step 4: Apply theme and keymap everywhere**

Remove hard-coded colors/keys from screen rendering and event dispatch. Every command must resolve through `Keymap`; every style through semantic `ThemeRole`. Reinitialize mouse capture when its setting changes.

- [ ] **Step 5: Test and commit**

Run: `cargo test -p dexo-app settings -p dexo-tui --test admin_settings_mcp_flow settings && cargo test -p dexo-tui --test snapshots`

Expected: PASS; update intentional snapshots for default, high-contrast, and ASCII modes.

Commit: `feat(settings): persist and apply ui preferences`

### Task 5: Make mouse interaction useful and accessibility testable

**Files:** create `crates/dexo-tui/src/mouse.rs`; modify `crates/dexo-tui/src/{terminal.rs,event.rs,layout.rs,render.rs,accessibility.rs}`, widgets and screens; tests `crates/dexo-tui/tests/mouse_accessibility.rs`.

- [ ] **Step 1: Test hit targets and coordinate translation**

```rust
#[test]
fn click_on_second_result_tab_selects_that_tab() {
    let map = render_hit_map(terminal_size(120, 40), model_with_results(3));
    let point = map.center(HitTarget::ResultTab(result_key(2)));
    assert_eq!(mouse_action(MouseEvent::left_down(point), &map), Some(Action::SelectResult(result_key(2))));
}
```

Cover explorer selection/expand, editor focus/cursor placement, grid cell selection, scrollbar drag, wheel scrolling, tab selection, modal buttons, resize, and clicks outside active modal.

- [ ] **Step 2: Add a render-produced hit map**

Each interactive widget registers stable `HitTarget` rectangles while rendering. Event translation reads the most recent map and produces existing semantic `Action` values; business logic does not inspect screen coordinates.

- [ ] **Step 3: Add accessible rendering invariants**

Selection, focus, production, warning, error, success, loading, and disabled states must have non-color markers. ASCII mode contains no non-ASCII glyphs. High-contrast theme meets the documented terminal color-pair policy. Respect reduced animation by replacing spinners with static status text.

- [ ] **Step 4: Verify keyboard-only reachability**

Add a focus traversal test for every control in connection, schema, transfer, admin, settings, recovery, and MCP overlays. Every mouse action must have a command/keymap equivalent.

- [ ] **Step 5: Test and commit**

Run: `cargo test -p dexo-tui --test mouse_accessibility -p dexo-tui --test snapshots`

Expected: PASS.

Commit: `feat(tui): add useful mouse and accessible states`

### Task 6: Restore real crash checkpoints and export diagnostic bundles

**Files:** `crates/dexo-app/src/{recovery_service.rs,diagnostic_service.rs}`; `crates/dexo-storage/src/{recovery.rs,session_recovery.rs,document.rs,layout.rs}`; create `crates/dexo-tui/src/runtime/{recovery_manager.rs,diagnostic_manager.rs}`; modify `screens/recovery.rs`, settings screen, TUI action/model/update/event; tests `crates/dexo-storage/tests/recovery_crash.rs`, `admin_settings_mcp_flow.rs`.

- [ ] **Step 1: Test dirty shutdown detection and selective restore**

```rust
#[tokio::test]
async fn startup_offers_only_the_last_unclean_checkpoint() {
    let paths = checkpoint_after_simulated_crash();
    let candidates = bootstrap_with(paths).await.unwrap().recovery_candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].transaction_state, RecoveredTransaction::UnknownNotRestored);
}
```

Cover restoring selected documents/layout, discarding a checkpoint, clean shutdown, malformed checkpoint quarantine, and the rule that sessions/transactions are never resurrected.

- [ ] **Step 2: Wire checkpoint lifecycle to the runtime**

Debounce writes after document/layout changes; serialize sanitized drafts through the storage worker; mark startup active only after bootstrap; mark clean shutdown after documents and layout flush successfully. Recovery screen reads repository candidates rather than constructing screen data.

- [ ] **Step 3: Add diagnostic preview and explicit local save**

Gather Dexo/Rust/OS versions, driver descriptors, capability states, redacted settings, migration version, last bounded log lines, and recent sanitized error categories. Show the exact manifest before `Effect::WriteDiagnosticBundle(PathBuf)`.

- [ ] **Step 4: Strengthen secret scanning**

Test URL passwords, key/value secrets, keychain labels, passfile paths, query parameters marked sensitive, proxy credentials, SSH private-key content, and injected sentinel values. Dexo never uploads a bundle; the only output is the user-selected local path.

- [ ] **Step 5: Test and commit**

Run: `cargo test -p dexo-storage --test recovery_crash -p dexo-app diagnostic -p dexo-tui --test admin_settings_mcp_flow recovery`

Expected: PASS.

Commit: `feat(recovery): restore checkpoints and save diagnostics`

### Task 7: Create and edit durable MCP profiles from TUI and CLI

**Files:** `crates/dexo-app/src/mcp/{profile.rs,selector.rs,policy.rs}`; `crates/dexo-storage/src/mcp_profile.rs`; `crates/dexo-tui/src/screens/mcp_profiles.rs`, action/model/update/render; `crates/dexo-cli/src/{args.rs,run.rs}`; tests `admin_settings_mcp_flow.rs`, `crates/dexo-cli/tests/mcp.rs`.

- [ ] **Step 1: Test complete profile round-trip**

```rust
#[tokio::test]
async fn mcp_profile_editor_persists_connections_selectors_tools_and_limits() {
    let profile = edited_profile();
    harness().save_mcp_profile(profile.clone()).await.unwrap();
    assert_eq!(harness().reload_profile(profile.id).await.unwrap(), profile);
}
```

- [ ] **Step 2: Replace fixture screen state with repository state**

List/create/rename/duplicate/edit/delete profiles. Edit enabled flag, allowed saved connection IDs, ordered allow/deny selectors down to column scope, separately named tool capabilities, query mode, row/byte/time/concurrency limits, and audit retention.

- [ ] **Step 3: Validate policy before persistence**

Reject missing/deleted connections, wildcard tools, zero/unbounded limits, selectors outside selected connections, invalid column rules, duplicate name, and a write-capable persistent policy. Persistent access remains read-only; writes require temporary grants.

- [ ] **Step 4: Require explicit enable/delete confirmations**

Enable preview lists exact connections, selectors, tools, limits, and read-only guarantee. Delete preview lists active grants and audit retention consequences; revoke active grants transactionally before deletion.

- [ ] **Step 5: Keep CLI and TUI behavior identical**

Move profile orchestration into an app service consumed by both adapters. CLI emits stable human and JSON output; TUI displays repository errors without mutating the active draft.

- [ ] **Step 6: Test and commit**

Run: `cargo test -p dexo-storage mcp_profile -p dexo-cli --test mcp -p dexo-tui --test admin_settings_mcp_flow mcp_profile`

Expected: PASS.

Commit: `feat(mcp): manage durable local profiles`

### Task 8: Route MCP calls across all allowed connections with strict limits

**Files:** create `crates/dexo-mcp/src/{router.rs,limits.rs,cancellation.rs}`; modify `crates/dexo-mcp/src/{lib.rs,server.rs,stdio.rs,tools_read.rs,tools_write.rs,resources.rs,prompts.rs}`, `crates/dexo-app/src/mcp/{service.rs,selector.rs}`, `crates/dexo-cli/src/run.rs`; test `crates/dexo-mcp/tests/multi_connection.rs`, `crates/dexo/tests/mcp_stdio.rs`.

- [ ] **Step 1: Prove the current first-connection startup is wrong**

```rust
#[tokio::test]
async fn same_profile_routes_each_request_to_its_named_connection() {
    let server = server_with_connections([("sales", session_a()), ("audit", session_b())]);
    server.call("query_execute_read", json!({"connection":"audit","sql":"select 1"})).await.unwrap();
    assert_eq!(session_a().query_calls(), 0);
    assert_eq!(session_b().query_calls(), 1);
}
```

- [ ] **Step 2: Add a profile-scoped connection router**

```rust
pub struct McpConnectionRouter {
    allowed: BTreeMap<ConnectionId, McpSessionSlot>,
    connector: Arc<dyn SessionConnector>,
}

impl McpConnectionRouter {
    pub async fn session(&self, id: &ConnectionId) -> Result<Arc<dyn Session>, McpFault>;
}
```

Resolve only IDs listed by the profile, fetch secrets from the system keychain at stdio startup/on first use, open independent read-only sessions lazily, reconnect idempotent reads once when safe, and close all sessions when stdin closes.

- [ ] **Step 3: Require an explicit connection in database-addressed calls**

Tool schemas, resources, prompts, result URIs, and audit records include `connection_id`. Omission is allowed only when the profile contains exactly one connection; otherwise return a non-enumerating validation error.

- [ ] **Step 4: Enforce selectors before catalog disclosure and query execution**

Deny rules override allows. Filter catalog search, descriptions, resources, completion metadata, and query output columns. Unauthorized and nonexistent objects return the same external error while audit retains the internal reason.

- [ ] **Step 5: Enforce capability-specific limits**

Use a semaphore for concurrency, cancellation token plus driver cancel for timeout/client cancellation, bounded stream accounting for rows/bytes, and a per-call result store limit. Truncation is explicit in MCP content and resource metadata.

- [ ] **Step 6: Keep stdio clean**

Protocol frames are the only stdout bytes. Logs and diagnostics go to stderr/local files with secrets redacted. Starting `dexo mcp serve PROFILE` is the only server lifecycle; do not add a daemon or network listener.

- [ ] **Step 7: Test and commit**

Run: `cargo test -p dexo-mcp --test multi_connection -p dexo --test mcp_stdio`

Expected: PASS.

Commit: `feat(mcp): route policy-safe multi-connection stdio calls`

### Task 9: Persist temporary grants, countdowns, revocation, audit, progress, and cancellation

**Files:** create `crates/dexo-mcp/src/audit.rs`; modify `crates/dexo-app/src/mcp/{grant.rs,ledger.rs,audit.rs,operation.rs}`, storage `src/mcp/{grant_repo.rs,audit_repo.rs,operation_repo.rs}`, TUI `screens/{mcp_profiles.rs,mcp_audit.rs}`, CLI MCP commands, MCP server/tools; tests `admin_settings_mcp_flow.rs`, `crates/dexo/tests/{mcp_write.rs,mcp_stdio.rs}`.

- [ ] **Step 1: Test grant expiry against repository time**

```rust
#[tokio::test]
async fn expired_grant_disappears_from_tools_and_cannot_authorize_write() {
    let clock = TestClock::at(1_000);
    let grant = ledger(&clock).create(grant_request(Duration::from_secs(60))).await.unwrap();
    clock.advance(Duration::from_secs(61));
    assert!(!server().list_tools().await.contains("data_update"));
    assert!(server().write_with(grant.id).await.is_err());
}
```

- [ ] **Step 2: Create grants through one transactional service**

The request contains profile, capability, exact tools, connection/object selectors no broader than the profile, reason, creator surface, and bounded expiry. Show the policy diff and require local confirmation in CLI or TUI before insert.

- [ ] **Step 3: Drive countdown from persisted expiration**

TUI ticks recalculate `expires_at - now`; they do not decrement an in-memory counter. Expired/revoked grants disappear from advertised tools without restarting the MCP process. Revoke one/all persists first, signals running operations, then refreshes UI.

- [ ] **Step 4: Audit every decision and operation**

Record profile, MCP session, connection, tool, sanitized selector/SQL fingerprint, grant ID, decision, rows/bytes, duration, truncation, cancellation, and categorized outcome. Audit writes must not include result values, passwords, raw secrets, or full SQL with sensitive parameters.

- [ ] **Step 5: Expose real audit filters and operation progress**

TUI/CLI filter by profile, connection, tool, decision, time, and outcome. Long MCP calls register `OperationId`, emit progress notifications when supported, honor `notifications/cancelled`, invoke driver cancellation, and store a terminal audit record.

- [ ] **Step 6: Test retention and restart behavior**

Verify grant survival/revocation across processes, automatic expiry, audit retention pruning by profile, revoke-all during active write, cancellation, concurrent-limit refusal, and absence of secrets.

- [ ] **Step 7: Test and commit**

Run: `cargo test -p dexo-storage mcp -p dexo --test mcp_write -p dexo --test mcp_stdio -p dexo-tui --test admin_settings_mcp_flow mcp`

Expected: PASS.

Commit: `feat(mcp): enforce expiring grants and durable audit`

### Task 10: Remove production fixture paths and make integration CI execute real tests

**Files:** all `crates/*/src/**/*.rs`, `.github/workflows/ci.yml`, `.github/workflows/integration.yml`, `scripts/check-production-fixtures.ps1`, `scripts/check-production-fixtures.sh`, `docs/testing.md`.

- [ ] **Step 1: Add an allowlist-based production-source check**

The script scans Rust production sources for `fixture`, `fake`, `sample`, hard-coded catalog rows, synthetic progress, and reducers that report success without emitting an effect. Legitimate terms must be listed with file, line pattern, and rationale; unknown matches fail CI.

- [ ] **Step 2: Delete obsolete screen constructors and branches**

Remove `fixture()` constructors from schema diff, transfer, explain, admin, settings, recovery, MCP profiles, and audit production modules. Snapshot tests build state in `tests/support`; user-facing demo data, if retained, is an explicit disconnected sample project that cannot be mistaken for live state.

- [ ] **Step 3: Repair Docker integration execution**

Use PostgreSQL/MySQL service health checks, unique databases per test module, least-privilege and admin test users, TLS cases, and explicit ignored-test invocations:

```bash
cargo test -p dexo --test postgres_live -- --ignored --test-threads=1
cargo test -p dexo --test mysql_live -- --ignored --test-threads=1
cargo test -p dexo --test completion_live -- --ignored --test-threads=1
```

The job fails if any binary reports zero executed tests.

- [ ] **Step 4: Add capability contract tests**

For every advertised PostgreSQL/MySQL capability, execute at least one success path and one unsupported/permission path. A driver may not advertise a capability implemented as an empty vector, constant fixture, or unconditional success.

- [ ] **Step 5: Run the source and test gates, then commit**

Run: `pwsh -File scripts/check-production-fixtures.ps1 && cargo test --workspace --no-fail-fast`

Expected: PASS.

Commit: `test: prohibit simulated production functionality`

### Task 11: Finish cross-platform packaging, documentation, and full-spec acceptance

**Files:** `.github/workflows/{ci.yml,integration.yml,release.yml}`, `Cross.toml`, installer/package metadata, `README.md`, `CHANGELOG.md`, `docs/{architecture.md,configuration.md,keybindings.md,security.md,mcp.md,testing.md,troubleshooting.md}`, `crates/dexo/tests/completion_live.rs`.

- [ ] **Step 1: Build the release matrix**

Produce tested archives for Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64. Each archive contains `dexo`, license, README, shell completions where supported, and version metadata. Generate SHA-256 checksums and a real CycloneDX/SPDX SBOM from the locked dependency graph; remove placeholder SBOM output.

- [ ] **Step 2: Add platform smoke tests**

On every runner, launch `dexo --version`, `dexo doctor --json`, a temporary local-config bootstrap, settings round-trip, diagnostic export, SQL file open/save, and MCP stdio initialize/list-tools/shutdown. Verify terminal restoration after normal exit and forced cancellation.

- [ ] **Step 3: Execute the full acceptance matrix**

| Approved requirement | Evidence required before completion |
|---|---|
| Live sessions, SQL, cancel, transactions, editor, files | Sprint 16 unit/TUI/live gates |
| Saved connections, secrets, TLS/SSH/proxy, multi-session | Sprint 17 transport and Docker gates |
| Projects, associations, layout, recent/config import-export | Sprint 18 restart and repository gates |
| Lazy catalog, inspectors, DDL/dependencies, goto, snapshots | Sprint 19 driver/TUI/live gates |
| Real grid, remote paging/filter/sort, copy/view/edit/FK | Sprint 20 data/live gates |
| DDL, diff, transfer, backup/restore, explain/compare | Sprint 21 advanced/live gates |
| Admin, security, settings, recovery, MCP, mouse/a11y | Sprint 22 unit/TUI/MCP/live gates |
| Linux, macOS, Windows and permissive open source | Release artifacts, license, SBOM, platform smoke jobs |

- [ ] **Step 4: Run final local quality checks**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo deny check
cargo audit
```

Expected: PASS with no ignored warning treated as success for required live coverage.

- [ ] **Step 5: Perform manual terminal acceptance on all three OS families**

Follow a checked script covering first run, project creation, both database types, simultaneous sessions, editor/query/cancel/transactions, explorer/data editing, schema/admin/transfer, restart/recovery, settings/mouse/ASCII mode, MCP read/grant/write/revoke/audit, and clean terminal restoration. Attach terminal recordings or screenshots and command logs to the release candidate.

- [ ] **Step 6: Finish operator and contributor documentation**

Document architecture boundaries, driver extension contract, configuration paths, keychain behavior, TLS/SSH/proxy, keymaps/themes, recovery guarantees, backup dependencies, MCP threat model and grant lifecycle, diagnostic redaction, testing commands, and troubleshooting. Examples must be runnable and use non-sensitive local values.

- [ ] **Step 7: Tag only after every evidence item is linked**

Create the release candidate checklist with links to CI jobs and platform artifacts. Do not mark the milestone complete while any row above lacks executable evidence.

Commit: `release: complete dexo functional implementation`

## Sprint 22 exit criteria

- [ ] All admin, maintenance, role, grant, and effective-privilege views operate on the selected live session.
- [ ] Theme, keymap, mouse, Unicode, animation, and recovery preferences persist and apply without restart unless terminal reinitialization is required.
- [ ] Recovery restores sanitized local documents/layout but never sessions or transactions; diagnostics are local, previewed, and secret-free.
- [ ] MCP runs only as an on-demand stdio server, supports every profile-allowed connection, defaults to read-only, and enforces selectors, capabilities, limits, grants, cancellation, audit, and retention.
- [ ] Mouse actions are useful and keyboard-equivalent; semantic states remain distinguishable without color or Unicode.
- [ ] Production sources contain no fixture-backed behavior, fake success, or empty advertised capability.
- [ ] PostgreSQL/MySQL live tests actually execute in CI, and Linux/macOS/Windows release artifacts pass smoke tests.
- [ ] Every approved requirement has linked automated or manual acceptance evidence; Dexo is functionally complete for this specification.
