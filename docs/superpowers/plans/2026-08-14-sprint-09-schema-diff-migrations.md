# Dexo Sprint 09: Schema Diff and Migrations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Comparar bancos/snapshots, ordenar mudanças e gerar scripts de migração revisáveis e seguros.

**Architecture:** Immutable versioned snapshots feed driver-aware normalization, structural diff and a dependency graph. Script generation reuses Sprint 08 DDL renderers; application reuses protected script execution.

**Tech Stack:** Serde/JSON, SQLite, standard-library Kahn topological sort/Tarjan SCC, Insta goldens.

---

## File map

- Create: `dexo-app/src/schema_diff/{snapshot.rs,normalize.rs,diff.rs,graph.rs,risk.rs,script.rs,lib.rs}`
- Create: `dexo-storage/src/schema_snapshot.rs` and migration 4
- Create: `dexo-tui/src/screens/schema_diff.rs`
- Extend: CLI `schema snapshot|diff`
- Test: normalization/diff/script goldens per driver.

### Task 1: Persist portable versioned snapshots

- [ ] **Step 1:** Add failing round-trip test with common and driver-specific attributes plus SHA-256 content digest.
- [ ] **Step 2:** Run target; expect snapshot type absent.
- [ ] **Step 3:** Implement `SchemaSnapshot { format_version: 1, driver, server_version, captured_at, scope, objects, digest }`; canonicalize object order before digest; migration 4 stores compressed JSON only after measuring benefit.
- [ ] **Step 4:** Run v3->v4 and tampered-digest rejection tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(diff): persist versioned schema snapshots"`.

### Task 2: Normalize only proven equivalences

- [ ] **Step 1:** Add goldens proving ordering noise is removed while PostgreSQL policy/MySQL collation differences remain.
- [ ] **Step 2:** Run tests; expect normalizers absent.
- [ ] **Step 3:** Implement common normalization plus `DriverNormalizer`; sort unordered sets, normalize server defaults only from explicit version rules, preserve unknown attributes.
- [ ] **Step 4:** Run idempotence property `normalize(normalize(x)) == normalize(x)`.
- [ ] **Step 5:** Commit with `git commit -m "feat(diff): normalize schemas conservatively"`.

### Task 3: Compute structural changes without guessing renames

- [ ] **Step 1:** Add tests for add/remove/alter and ambiguous similar objects. Assert ambiguous rename remains remove+add until user supplies mapping.
- [ ] **Step 2:** Run target; expect diff absent.
- [ ] **Step 3:** Implement `SchemaDifference::{Added,Removed,Changed}` keyed by qualified identity and `RenameMapping` explicit input; filters by scope/kind.
- [ ] **Step 4:** Run bidirectional test where swapping inputs reverses added/removed and before/after.
- [ ] **Step 5:** Commit with `git commit -m "feat(diff): compute explicit structural changes"`.

### Task 4: Order dependencies and surface cycles

- [ ] **Step 1:** Add graph test table-before-FK on create, FK-before-table on drop, and two-view cycle marked manual.
- [ ] **Step 2:** Run target; expect graph absent.
- [ ] **Step 3:** Implement deterministic Kahn topological sort and strongly connected component reporting; do not invent an order inside cycles.
- [ ] **Step 4:** Run graph property tests ensuring every non-cycle edge respects output order.
- [ ] **Step 5:** Commit with `git commit -m "feat(diff): order migrations by dependencies"`.

### Task 5: Generate risk-classified forward/reverse scripts

- [ ] **Step 1:** Add PostgreSQL/MySQL golden scripts including irreversible drop, data-loss type change, lock warning and manual cycle marker.
- [ ] **Step 2:** Run goldens; expect generator absent.
- [ ] **Step 3:** Convert differences to Sprint 08 `SchemaChange`, render through driver, prepend machine-readable comments for risk; emit reverse only where every change is reversible.
- [ ] **Step 4:** Apply forward to fixture, re-introspect and assert empty diff; apply reverse for reversible fixture.
- [ ] **Step 5:** Commit with `git commit -m "feat(diff): generate reviewed migration scripts"`.

### Task 6: Expose diff in TUI and CLI

- [ ] **Step 1:** Add CLI goldens for `dexo schema snapshot` and `dexo schema diff --from ... --to ... --format json|sql`; TUI snapshots for filters/risk/script.
- [ ] **Step 2:** Run; expect commands/screens absent.
- [ ] **Step 3:** Implement background capture, object filters, explicit rename mapping, save report/script and protected apply through workbench. CLI never applies unless `--apply --confirm-target` is supplied.
- [ ] **Step 4:** Run full sprint gate and driver round-trip E2E.
- [ ] **Step 5:** Commit with `git commit -m "feat(schema): compare schemas and review migrations"`.

## Sprint exit

- [ ] Snapshot format/digest is stable and tamper-detected.
- [ ] Normalization preserves native differences.
- [ ] Cycles and ambiguous renames require human resolution.
- [ ] Forward script reaches zero diff for fixtures.
- [ ] Apply remains explicit and protected.
