# Dexo Sprint 08: Schema DDL and Security Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Criar e alterar objetos, usuários, roles e grants por mudanças tipadas com preview DDL e proteção proporcional ao risco.

**Architecture:** Forms produce `SchemaChange` domain values; driver DDL renderers produce statements and risk metadata. Preview and policy evaluation are mandatory before execution; TUI never edits catalog state optimistically.

**Tech Stack:** driver API, PostgreSQL/MySQL protocol clients, SQL dialect services, Ratatui forms, proptest.

---

## File map

- Create: `dexo-driver-api/src/schema_change.rs`
- Create: driver `ddl/{render.rs,execute.rs}`
- Create: `dexo-app/src/schema/{change.rs,preview.rs,apply.rs,security.rs}`
- Create: `dexo-tui/src/screens/schema_editor.rs`, `widgets/form.rs`, `modals/ddl_preview.rs`
- Test: driver DDL golden/round-trip contracts.

### Task 1: Define typed schema changes and risk

**Files:** driver API/app schema.

- [x] **Step 1:** Add failing test asserting `DropObject` is destructive/irreversible and `AddIndex` is lock-sensitive/reversible.
- [x] **Step 2:** Run target; expect types absent.
- [x] **Step 3:** Implement `SchemaChange::{CreateTable,AlterTable,CreateView,AlterRoutine,CreateIndex,DropObject,RenameObject,Grant,Revoke}` and `ChangeRisk { destructive, data_loss, lock_level, reversible }`; validate non-empty qualified targets.
- [x] **Step 4:** Run exhaustive match test proving every variant has risk classification.
- [x] **Step 5:** Commit with `git commit -m "feat(schema): model typed DDL changes and risk"`.

### Task 2: Render PostgreSQL DDL

**Files:** PostgreSQL DDL modules/golden tests.

- [x] **Step 1:** Add goldens for identity columns, enum/domain, partition, materialized view, index options, policy, function/procedure/trigger, role and grant.
- [x] **Step 2:** Run goldens; expect renderer absent.
- [x] **Step 3:** Implement quoting via one `PgDialect` service; render a `DdlPlan { statements, rollback, warnings }`; never accept raw identifier strings outside `QualifiedName`.
- [x] **Step 4:** Execute plans against a container, re-introspect and assert requested shape.
- [x] **Step 5:** Commit with `git commit -m "feat(postgres): render and verify DDL plans"`.

### Task 3: Render MySQL DDL

**Files:** MySQL DDL modules/golden tests.

- [x] **Step 1:** Add goldens for engine/charset/collation, auto increment, generated column, partition, event/routine/trigger, user/role/grant.
- [x] **Step 2:** Run goldens; expect renderer absent.
- [x] **Step 3:** Implement `MysqlDialect` plans, marking implicit-commit statements and operations with table rebuild/lock risk.
- [x] **Step 4:** Execute against container and re-introspect exact shape.
- [x] **Step 5:** Commit with `git commit -m "feat(mysql): render and verify DDL plans"`.

### Task 4: Preview dependencies and enforce destructive policies

**Files:** app preview/apply/security.

- [x] **Step 1: Add policy test**

```rust
#[test]
fn production_drop_requires_typed_target() {
    let decision = evaluate(&drop_table("prod.public.orders"), &production_policy());
    assert_eq!(decision.confirmation, Confirmation::TypeTarget("prod.public.orders".into()));
}
```

- [x] **Step 2:** Run target; expect FAIL.
- [x] **Step 3:** Combine DDL plan, known dependents, grants and risk into preview. Apply only after exact confirmation; execute transactionally when driver says possible; otherwise report committed statement boundary.
- [x] **Step 4:** Run production/read-only/unknown-SQL/cancel tests.
- [x] **Step 5:** Commit with `git commit -m "feat(schema): preview impact and protect DDL"`.

### Task 5: Build common-object TUI forms and raw DDL escape hatch

**Files:** schema editor/form/preview snapshots.

- [x] **Step 1:** Add reducer/snapshots for table columns/defaults/identity, indexes, constraints, FKs, view/routine/trigger and validation errors.
- [x] **Step 2:** Run snapshots; expect screens absent.
- [x] **Step 3:** Implement focus-safe forms producing typed changes. Raw DDL uses editor/workbench execution and the same risk classifier; show generated diff before replacing form state.
- [x] **Step 4:** Run snapshots at full/compact sizes.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): edit schema objects with DDL preview"`.

### Task 6: Manage users, roles and grants

**Files:** app security service, driver DDL/admin methods, TUI security screen, CLI inspect extensions.

- [x] **Step 1:** Add shared least-privilege tests creating role, granting SELECT on one table, verifying second table denied, then revoking.
- [x] **Step 2:** Run both container tests; expect FAIL.
- [x] **Step 3:** Implement list/effective grants when available, create/alter/drop principal, grant/revoke object/role. Password inputs go straight from `SecretString` to protocol and never SQLite/history/preview.
- [x] **Step 4:** Run grant contracts and sentinel scan.
- [x] **Step 5:** Commit with `git commit -m "feat(security): manage database roles and grants"`.

### Task 7: Invalidate catalog only after confirmed DDL outcome

**Files:** schema apply/catalog service tests.

- [x] **Step 1:** Add failure test proving catalog remains valid when first statement fails, and uncertain boundary marks subtree stale.
- [x] **Step 2:** Run target; expect FAIL.
- [x] **Step 3:** Map outcome `RolledBack|Committed|PartiallyCommitted|Unknown` to precise cache invalidation and mandatory refresh indicator.
- [x] **Step 4:** Run full sprint gate and both DDL suites.
- [x] **Step 5:** Commit with `git commit -m "feat(catalog): invalidate after DDL outcomes"`.

## Sprint exit

- [x] Common/native DDL fixtures round-trip through introspection.
- [x] Every change has risk and preview.
- [x] Production destructive confirmation is target-specific.
- [x] Principal secrets never persist or log.
- [x] Partial/unknown DDL outcome is explicit.
