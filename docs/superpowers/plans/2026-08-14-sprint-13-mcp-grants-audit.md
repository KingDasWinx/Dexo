# Dexo Sprint 13: MCP Grants and Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adicionar elevação MCP temporária e separada para data_write/ddl/admin com idempotência, revogação e auditoria sanitizada.

**Architecture:** Local TUI/CLI creates grants; MCP can only consume them. Policy evaluation, grant consumption and operation records are transactional in SQLite; mutation execution reuses existing application use cases and reports exact side-effect state.

**Tech Stack:** RMCP tools/list_changed, SQLite migration 6, UUID operation IDs, test clock, existing mutation/DDL/admin services.

---

## File map

- Create: `dexo-app/src/mcp/{grant.rs,operation.rs,audit.rs}`
- Create: `dexo-storage/src/mcp/{grant_repo.rs,operation_repo.rs,audit_repo.rs}` and migration 6
- Create: `dexo-mcp/src/tools_write.rs`
- Extend: MCP TUI/CLI administration
- Test: race/idempotency/audit sentinel suites.

### Task 1: Persist bounded grants transactionally

- [ ] **Step 1:** Add tests for default 15m, hard max 24h, one-use consumption and independent `DataWrite|Ddl|Admin` capabilities.
- [ ] **Step 2:** Run; expect types absent.
- [ ] **Step 3:** Add grant/audit/operation tables; implement `Grant { id, profile, connection, selectors, tools, capability, expires_at, remaining_uses }`; reject `all`, empty tools and scope broader than profile.
- [ ] **Step 4:** Run v5->v6, concurrent consume and fake-clock expiration tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): persist bounded temporary grants"`.

### Task 2: Create/revoke grants only from local UX

- [ ] **Step 1:** Add CLI tests for `grant create|list|revoke`, typed target and `--expires 15m`; assert MCP tool catalog has no grant-management tool.
- [ ] **Step 2:** Run; expect commands absent.
- [ ] **Step 3:** Implement local commands and TUI countdown/diff/revoke-all. Validate requested tool/capability/target against effective profile and connection policy before writing.
- [ ] **Step 4:** Run CLI/TUI/protocol catalog tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): manage grants only through local interfaces"`.

### Task 3: Advertise mutating tools dynamically

- [ ] **Step 1:** Add initialized-client test: create grant externally, receive `notifications/tools/list_changed`, list contains only granted tool; revoke and verify removal.
- [ ] **Step 2:** Run; expect FAIL.
- [ ] **Step 3:** Observe grant revision changes; emit notification only when negotiated; recompute authorization on every list/call regardless of notification. Add concrete tools `data_insert/update/delete/execute_sql`, `schema_apply_ddl` and specific admin actions.
- [ ] **Step 4:** Run list-change, expiration and client-without-notification tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): publish tools from active grants"`.

### Task 4: Execute mutations through existing protected services

- [ ] **Step 1:** Add E2E tests proving `data_write` cannot DDL, `ddl` cannot terminate session, admin cannot read hidden table and every target stays inside grant selectors.
- [ ] **Step 2:** Run; expect write tools absent.
- [ ] **Step 3:** Map structured inputs to Sprint 07 change sets, DDL to Sprint 08 plans and admin to Sprint 11 actions. Require `operation_id`, active grant, connection policy and actual DB privilege. `data_execute_sql` requires explicit tool rule and understood effect.
- [ ] **Step 4:** Run PostgreSQL/MySQL capability-isolation tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): execute scoped mutation tools"`.

### Task 5: Guarantee operation idempotency

- [ ] **Step 1: Add exact replay tests**

```rust
#[tokio::test]
async fn same_operation_and_payload_executes_once() {
    let first = call("op-1", json!({"id": 7})).await;
    let replay = call("op-1", json!({"id": 7})).await;
    assert_eq!(first, replay);
    assert_eq!(database_execution_count(), 1);
}
```

Also assert same ID/different payload fails and unknown outcome never retries.

- [ ] **Step 2:** Run; expect FAIL.
- [ ] **Step 3:** Transactionally reserve `(profile,session,operation_id,tool,payload_hash)` before grant use; states `Running|Succeeded|Failed|Unknown`; replay same payload returns recorded result, otherwise conflict. Expire records after configured TTL.
- [ ] **Step 4:** Run concurrent duplicate and process-interruption tests.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): make mutations idempotent per session"`.

### Task 6: Revoke safely during active calls

- [ ] **Step 1:** Add race tests revoking before dispatch, during read, during transactional DML, after irreversible MySQL DDL commit.
- [ ] **Step 2:** Run; expect incorrect/absent handling.
- [ ] **Step 3:** Recheck grant before dispatch; bind cancellation token to grant revision; rollback when uncommitted; report `Committed|RolledBack|PartiallyCommitted|Unknown` exactly and never claim reversal after commit.
- [ ] **Step 4:** Run race suite repeatedly with `cargo nextest run -E 'test(revoke_)' --retries 0`.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): revoke grants with explicit side-effect state"`.

### Task 7: Record and export sanitized audit

- [ ] **Step 1:** Add sentinel test covering allowed/denied/resource/tool/grant/replay calls; result data and `SUPER_SECRET_SENTINEL` must be absent.
- [ ] **Step 2:** Run; expect audit absent.
- [ ] **Step 3:** Record timestamp, request/operation, profile, declared client info, target, decision, grant, duration, rows, bytes, status and known effects. SQL mode `None|Hash|Sanitized`; export local-only with preview and retention prune.
- [ ] **Step 4:** Run sentinel, retention and CLI/TUI audit tests plus MCP conformance.
- [ ] **Step 5:** Commit with `git commit -m "feat(mcp): audit calls without storing results or secrets"`.

## Sprint exit

- [ ] MCP cannot create/renew grants.
- [ ] Capability groups never imply each other.
- [ ] One-use and expiry are race-safe.
- [ ] Duplicate operation executes once.
- [ ] Revocation reports exact outcome.
- [ ] Audit contains metadata, never results/secrets.
