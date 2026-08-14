# Dexo Sprint 06: Catalog Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Entregar introspecção incremental completa, cache offline, busca e navegação de objetos para PostgreSQL e MySQL.

**Architecture:** Drivers emit a normalized catalog plus namespaced attributes. `dexo-storage` caches versioned snapshots; explorer loads tree nodes lazily and search uses an in-memory index built from allowed snapshots.

**Tech Stack:** driver protocols, Serde JSON, SQLite migration 3, Tokio tasks, Ratatui tree/list widgets.

---

## File map

- Create: `dexo-driver-api/src/{catalog.rs,ddl.rs}`
- Create: `dexo-driver-postgres/src/catalog/*`, `dexo-driver-mysql/src/catalog/*`
- Create: `dexo-app/src/{catalog_service.rs,search_service.rs}`
- Create: `dexo-storage/src/catalog_cache.rs`
- Create: `dexo-tui/src/screens/explorer.rs`, `widgets/object_tree.rs`
- Modify: CLI with `inspect`; add driver catalog contracts.

### Task 1: Define normalized catalog objects

**Files:** driver API catalog/ddl.

- [x] **Step 1:** Add failing serialization test for `CatalogObject { id, kind, qualified_name, parent, attributes }` preserving `driver.postgres.partition_key`.
- [x] **Step 2:** Run `cargo test -p dexo-driver-api catalog_object_round_trip`; expect FAIL.
- [x] **Step 3:** Implement `ObjectKind` for catalog/schema/table/view/materialized-view/column/index/constraint/sequence/function/procedure/trigger/user/role plus `DriverSpecific(String)`. IDs are stable within snapshot and never derived from display text.
- [x] **Step 4:** Run tests; expect lossless JSON round-trip.
- [x] **Step 5:** Commit with `git commit -m "feat(catalog): define normalized object model"`.

### Task 2: Introspect PostgreSQL objects and extensions

**Files:** PostgreSQL catalog modules/tests.

- [x] **Step 1:** Seed a fixture with enum, domain, sequence, partition, materialized view, policy, extension, FDW metadata, publication, function, procedure, trigger and grants; write expected object-kind assertions.
- [x] **Step 2:** Run ignored driver test; expect FAIL.
- [x] **Step 3:** Query `pg_catalog` with bound namespace filters; fetch children on demand; preserve OIDs only as driver attributes; provide DDL and dependency queries separately.
- [x] **Step 4:** Run PostgreSQL catalog contract; expect all seeded objects and no system schemas unless requested.
- [x] **Step 5:** Commit with `git commit -m "feat(postgres): introspect full catalog"`.

### Task 3: Introspect MySQL objects and extensions

**Files:** MySQL catalog modules/tests.

- [x] **Step 1:** Seed engine, generated column, partition, event, routine, trigger, charset/collation, user/role/grants; assert normalized and namespaced fields.
- [x] **Step 2:** Run ignored test; expect FAIL.
- [x] **Step 3:** Query `information_schema`, `mysql` metadata only when permission permits, and `SHOW CREATE` through explicit methods. Permission denial marks capability `restricted` rather than failing the whole tree.
- [x] **Step 4:** Run MySQL contract; expect all accessible objects and restriction reasons.
- [x] **Step 5:** Commit with `git commit -m "feat(mysql): introspect full catalog"`.

### Task 4: Cache and invalidate catalog snapshots

**Files:** `catalog_cache.rs`, migration 3, tests.

- [x] **Step 1: Add failing cache test**

```rust
#[test]
fn ddl_invalidates_only_affected_subtree() {
    let cache = fixture_cache();
    cache.invalidate(&QualifiedName::new(Some("db"), Some("public"), "orders")).unwrap();
    assert!(cache.is_stale("db.public.orders"));
    assert!(!cache.is_stale("db.public.users"));
}
```

- [x] **Step 2:** Run target; expect FAIL.
- [x] **Step 3:** Add `catalog_snapshots` and `catalog_objects` tables keyed by connection/database/snapshot/object; transactional replace; stale subtree markers; retention of latest complete snapshot.
- [x] **Step 4:** Run v2->v3 migration, offline load and invalidation tests.
- [x] **Step 5:** Commit with `git commit -m "feat(storage): cache catalog snapshots"`.

### Task 5: Search 100k objects deterministically

**Files:** `search_service.rs`, benchmark/test.

- [x] **Step 1:** Add a 100k-object test with exact/prefix/subsequence ranking and a benchmark assertion recorded, not timing-gated in unit tests.
- [x] **Step 2:** Run test; expect service absent.
- [x] **Step 3:** Build normalized lowercase token index by name/schema/kind; score exact > prefix > word start > subsequence > recency/favorite; deterministic qualified-name tie-break.
- [x] **Step 4:** Run test and `cargo bench -p dexo-app catalog_search`; record p95 baseline under `benchmarks/results/catalog-search.json`.
- [x] **Step 5:** Commit with `git commit -m "feat(catalog): index and search large catalogs"`.

### Task 6: Build lazy explorer and object actions

**Files:** explorer screen/tree widget/snapshots.

- [x] **Step 1:** Add snapshots proving expand loads only one subtree, offline badge appears, filters work, and actions include properties/DDL/data/dependencies/dependents/copy-name.
- [x] **Step 2:** Run snapshots; expect FAIL.
- [x] **Step 3:** Implement node states unloaded/loading/loaded/restricted/error; background refresh; preserve selection by stable object ID; favorites and global search navigation.
- [x] **Step 4:** Approve snapshots and run reducer tests.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): add lazy database explorer"`.

### Task 7: Expose catalog inspection through CLI

**Files:** CLI args/run/presenter/tests.

- [x] **Step 1:** Add failing tests for `dexo inspect --connection c --object db.public.users --format json` and offline `--snapshot latest`.
- [x] **Step 2:** Run tests; expect command absent.
- [x] **Step 3:** Implement inspect/search/refresh options using `CatalogService`; stdout data only; restricted object exits permission category without revealing hidden candidates.
- [x] **Step 4:** Run full sprint gate and both Docker catalog suites.
- [x] **Step 5:** Commit with `git commit -m "feat(cli): inspect and search catalog objects"`.

## Sprint exit

- [x] PostgreSQL/MySQL object fixtures satisfy shared and native contracts.
- [x] Tree is lazy and survives restricted metadata.
- [x] Offline snapshot powers explorer and autocomplete.
- [x] DDL invalidates only affected subtree.
- [x] 100k search baseline is recorded and under the spec budget on reference hardware.
