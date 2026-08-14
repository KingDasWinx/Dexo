# Dexo Sprint 12: MCP Read-Only Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Expor catálogo e consultas read-only por MCP `stdio` com perfis, allowlists, limites e conformidade oficial.

**Architecture:** `dexo-mcp` is a protocol adapter over `dexo-app`; it never accesses drivers/storage/keychain directly. Profiles are evaluated server-side on every list/read/call and only permitted capabilities are advertised.

**Tech Stack:** official `rmcp` 3.1.2 server/transport-io/macros/schemars features, Schemars 1.2.2, Tokio 1.53 stdio, SQLite migration 5, MCP conformance suite.

---

## File map

- Create: `dexo-app/src/mcp/{profile.rs,selector.rs,policy.rs,service.rs}`
- Create: `dexo-storage/src/mcp_profile.rs`, migration 5
- Create: `dexo-mcp/src/{server.rs,stdio.rs,schema.rs,resources.rs,tools_read.rs,prompts.rs,error.rs,lib.rs}`
- Create: `dexo-tui/src/screens/mcp_profiles.rs`
- Extend: CLI `mcp profile|allow|policy|doctor|config|serve`
- Test: policy property tests, protocol fixtures, conformance.

### Task 1: Persist disabled-by-default MCP profiles

- [x] **Step 1: Write failing repository test**

```rust
#[test]
fn new_profile_is_disabled_and_read_only() {
    let profile = McpProfile::new("assistant");
    assert!(!profile.enabled);
    assert_eq!(profile.persistent_access, PersistentAccess::ReadOnly);
}
```

- [x] **Step 2:** Run target; expect types absent.
- [x] **Step 3:** Add migration 5 tables `mcp_profiles`, `mcp_selectors`, `mcp_tool_rules`; model limits rows/bytes/timeout/concurrency, query mode and audit retention. Reject wildcard capabilities and zero/overflow limits.
- [x] **Step 4:** Run v4->v5 and repository round-trip tests.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): persist safe profiles and limits"`.

### Task 2: Enforce deny-wins selectors without enumeration

- [x] **Step 1: Add policy tests**

```rust
#[test]
fn table_deny_wins_over_schema_allow() {
    let policy = policy([allow("db.public.*"), deny("db.public.secrets")]);
    assert_eq!(policy.decide(object("db.public.users")), Decision::Allow);
    assert_eq!(policy.decide(object("db.public.secrets")), Decision::DenyHidden);
}
```

- [x] **Step 2:** Run target; expect engine absent.
- [x] **Step 3:** Parse selectors into catalog/schema/object/column segments; exact and explicit `*` only; more-specific rules restrict but never broaden; denied and nonexistent targets share one safe external error.
- [x] **Step 4:** Run property tests proving adding a deny cannot increase accessible targets.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): enforce granular deny-wins policies"`.

### Task 3: Start a stdout-pure RMCP stdio server

- [x] **Step 1:** Add process test sending `initialize`, asserting every stdout line parses as JSON-RPC and stderr contains no protocol response.
- [x] **Step 2:** Run; expect `mcp serve` absent.
- [x] **Step 3:** Implement `dexo mcp serve --profile`; initialize tracing to sanitized file/stderr before binding `(tokio::io::stdin(), tokio::io::stdout())`; use official `rmcp::ServiceExt`; announce tools/resources/prompts/logging/cancellation only.
- [x] **Step 4:** Run byte-level stdout test with debug logging enabled.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): serve protocol over clean stdio"`.

### Task 4: Expose bounded catalog and result resources

- [x] **Step 1:** Add test listing a profile with one allowed schema; assert no disallowed URI/name appears and expired result resource returns generic not-found.
- [x] **Step 2:** Run; expect resources absent.
- [x] **Step 3:** Implement `dexo://profile/capabilities`, catalog/object/DDL/dependency resources and opaque `dexo://result/{random}` pages. Bind result handle to server session, owner profile and TTL; enforce bytes/rows before serialization.
- [x] **Step 4:** Run isolation, TTL, pagination and session-disconnect cleanup tests.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): expose scoped resources"`.

### Task 5: Implement read-only tools with database enforcement

- [x] **Step 1:** Add tool tests for catalog search/describe/DDL/relationships/query validate/read/explain/schema diff; attempt `WITH ... DELETE`, mutating function and disallowed table.
- [x] **Step 2:** Run; expect tools absent.
- [x] **Step 3:** Implement Schemars input/output DTOs. Structured tools use catalog IDs. `query_execute_read` requires profile `RawReadSql`, one understood read statement, matching object scopes and driver read-only transaction/session; profiles with strong object/column isolation expose structured tools only.
- [x] **Step 4:** Run both database tests with least-privilege role; mutating/unknown/disallowed calls must fail before returning data.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): add governed read tools"`.

### Task 6: Add prompts and local administration UX

- [x] **Step 1:** Add CLI goldens for profile/allow/policy/doctor/config print and TUI snapshots for permission diff/preview.
- [x] **Step 2:** Run; expect absent administration.
- [x] **Step 3:** Implement prompts `explore_schema`, `review_migration`, `analyze_plan` using only allowed URIs/tool names. `config print` emits client snippets but never edits them. Enabling a profile shows effective scopes/tools and requires explicit local confirmation.
- [x] **Step 4:** Run CLI/TUI tests and secret sentinel scan.
- [x] **Step 5:** Commit with `git commit -m "feat(mcp): manage profiles from CLI and TUI"`.

### Task 7: Pass MCP conformance and lifecycle tests

- [x] **Step 1:** Add CI job invoking the official conformance suite against `dexo mcp serve --profile conformance-fixture` with a temporary data directory.
- [x] **Step 2:** Run suite; record initial failures.
- [x] **Step 3:** Fix only negotiated stable protocol behavior; add cancellation/disconnect tests proving reads stop, resources clear and sessions close. Sampling, elicitation, roots, HTTP and client features remain absent.
- [x] **Step 4:** Run conformance plus workspace/Docker gates; expect PASS.
- [x] **Step 5:** Commit with `git commit -m "test(mcp): pass read-only server conformance"`.

## Sprint exit

- [x] New profiles are disabled/read-only.
- [x] Denied targets cannot be enumerated through list/error.
- [x] Stdout is pure JSON-RPC under debug logging.
- [x] Raw SQL is absent where database permissions cannot guarantee scope.
- [x] Conformance and disconnect cleanup pass.
