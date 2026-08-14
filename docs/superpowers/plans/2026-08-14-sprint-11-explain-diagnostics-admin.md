# Dexo Sprint 11: Explain Diagnostics and Administration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar explain estruturado, comparação de planos, sessões, locks, estatísticas, variáveis e manutenção protegida.

**Architecture:** Drivers parse native explain/administration output into common models with raw native payload retained. Read diagnostics and mutating admin actions are separate capabilities and policies.

**Tech Stack:** serde_json, driver system catalogs/performance_schema, Ratatui tree/table, Insta goldens.

---

## File map

- Create: `dexo-driver-api/src/{explain.rs,admin.rs}`
- Create: driver `explain.rs`, `admin.rs`
- Create: `dexo-app/src/{explain_service.rs,admin_service.rs}`
- Create: TUI explain/admin screens
- Extend: CLI `explain`, `sessions list|cancel|terminate`
- Test: plan goldens and admin contracts.

### Task 1: Parse structured PostgreSQL plans

- [x] **Step 1:** Add JSON goldens for scan/join/sort/aggregate/parallel nodes with estimated and actual metrics.
- [x] **Step 2:** Run; expect parser absent.
- [x] **Step 3:** Implement `PlanNode { kind, relation, estimates, actual, loops, children, native }` and `ExplainPlan { planning_ms, execution_ms, root, raw }`; request `FORMAT JSON`; `ANALYZE` only when explicit.
- [x] **Step 4:** Parse goldens and run container explain.
- [x] **Step 5:** Commit with `git commit -m "feat(postgres): parse structured explain plans"`.

### Task 2: Parse MySQL JSON/TREE plans

- [x] **Step 1:** Add version-specific JSON/TREE goldens and capability fallback tests.
- [x] **Step 2:** Run; expect parser absent.
- [x] **Step 3:** Prefer JSON, use TREE when actual execution requested/supported, preserve raw fields and mark unavailable metrics rather than zero.
- [x] **Step 4:** Run goldens and container explain across matrix images.
- [x] **Step 5:** Commit with `git commit -m "feat(mysql): parse explain plans by capability"`.

### Task 3: Render and compare plans

- [x] **Step 1:** Add snapshots for tree/table/summary and test comparing node path/kind/relation rather than unstable IDs.
- [x] **Step 2:** Run; expect screens absent.
- [x] **Step 3:** Render cost/cardinality/time/loops; highlight estimated-vs-actual ratios as heuristic labels; compare added/removed/changed nodes and retain raw export.
- [x] **Step 4:** Approve snapshots and comparison goldens.
- [x] **Step 5:** Commit with `git commit -m "feat(explain): render and compare query plans"`.

### Task 4: Read sessions, queries and locks

- [x] **Step 1:** Add shared admin fixture with blocker/blocked session and permission-restricted role.
- [x] **Step 2:** Run both driver tests; expect admin provider absent.
- [x] **Step 3:** Query `pg_stat_activity`/lock catalogs and MySQL performance schema/processlist; normalize `SessionInfo`, `LockInfo`, `BlockingEdge`; capability restricted includes safe reason.
- [x] **Step 4:** Run blocker graph and restricted-role tests.
- [x] **Step 5:** Commit with `git commit -m "feat(admin): inspect sessions and blocking locks"`.

### Task 5: Expose sizes, statistics and variables

- [x] **Step 1:** Add tests distinguishing captured-at timestamp, session/server variable scope and unavailable metrics.
- [x] **Step 2:** Run; expect methods absent.
- [x] **Step 3:** Implement paged size/stat queries and variable readers with native units plus normalized bytes where exact; never infer missing privileges as zero.
- [x] **Step 4:** Run both driver tests.
- [x] **Step 5:** Commit with `git commit -m "feat(admin): inspect sizes statistics and variables"`.

### Task 6: Protect cancellation, termination and maintenance

- [x] **Step 1:** Add production policy tests: cancel own query confirm once; terminate other session type target; VACUUM/ANALYZE/REINDEX/OPTIMIZE preview exact command and lock risk.
- [x] **Step 2:** Run; expect actions absent.
- [x] **Step 3:** Implement separate `AdminAction` variants and driver-specific execution. Require capability/permission and never retry. Report already-finished target as idempotent no-op only when server confirms absence.
- [x] **Step 4:** Run action contracts and permission denial tests.
- [x] **Step 5:** Commit with `git commit -m "feat(admin): protect operational actions"`.

### Task 7: Add TUI and CLI administration workflows

- [x] **Step 1:** Add snapshots and CLI goldens for explain and sessions list/cancel/terminate.
- [x] **Step 2:** Run; expect absent commands/screens.
- [x] **Step 3:** Implement background refresh with captured timestamp, pause/resume, structured output and fully qualified confirmations. `EXPLAIN ANALYZE` goes through normal statement risk policy.
- [x] **Step 4:** Run full sprint gate and Docker suites.
- [x] **Step 5:** Commit with `git commit -m "feat: expose explain diagnostics and administration"`.

## Sprint exit

- [x] Raw and normalized plans are preserved.
- [x] Explain analyze always requires execution-aware confirmation.
- [x] Blocking graph works and permission restrictions degrade gracefully.
- [x] Admin mutations are separate capabilities and never retry.
- [x] TUI/CLI present timestamps, targets and native errors accurately.
