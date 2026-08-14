# Dexo Sprint 03: PostgreSQL and MySQL Query Vertical Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Conectar, executar consulta streaming, paginar, transacionar e cancelar com segurança em PostgreSQL e MySQL pela CLI.

**Architecture:** Drivers translate native protocols into `QueryEvent`; `dexo-runtime` applies bounded channels/backpressure; `dexo-app` owns sessions and safety. CLI is a presenter over the application use case.

**Tech Stack:** tokio-postgres 0.7.18, mysql_async 0.37.0, tokio-postgres-rustls 0.14.0, futures-util 0.3, testcontainers 0.28.0, CSV/JSON presenters.

---

## File map

- Create: `crates/dexo-driver-postgres/src/{factory.rs,session.rs,decode.rs,error.rs,lib.rs}`
- Create: `crates/dexo-driver-mysql/src/{factory.rs,session.rs,decode.rs,error.rs,lib.rs}`
- Create: `crates/dexo-app/src/{driver_registry.rs,session_manager.rs,query_service.rs,transaction_service.rs}`
- Create: `crates/dexo-runtime/src/stream.rs`
- Create: `crates/dexo-test-support/src/containers.rs`
- Modify: `crates/dexo-cli/src/{args.rs,run.rs}`
- Test: driver integration tests and `crates/dexo-cli/tests/query.rs`

### Task 1: Start real PostgreSQL and MySQL fixtures

**Files:** `dexo-test-support/src/containers.rs`, driver test manifests.

- [x] **Step 1: Write failing fixture smoke test**

```rust
#[tokio::test]
async fn databases_are_reachable() {
    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
    assert!(pair.postgres_url().starts_with("postgres://"));
    assert!(pair.mysql_url().starts_with("mysql://"));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-test-support databases_are_reachable -- --ignored`

Expected: FAIL because `DatabasePair` is missing.

- [x] **Step 3: Implement container fixture**

Use `testcontainers_modules::{postgres::Postgres, mysql::Mysql}`. Pin images through explicit tags in one constants module. Create database `dexo`, user `dexo`, password `dexo_test_only`, wait for readiness, and expose URLs only to test code. Mark Docker tests `#[ignore = "requires Docker"]`.

- [x] **Step 4: Run fixture**

Run: `cargo test -p dexo-test-support databases_are_reachable -- --ignored --nocapture`

Expected: PASS with both containers stopped on drop.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-test-support
git commit -m "test: add PostgreSQL and MySQL containers"
```

### Task 2: Implement PostgreSQL query streaming

**Files:** PostgreSQL driver files and integration test `tests/query.rs`.

- [x] **Step 1: Write failing driver contract test**

```rust
#[tokio::test]
#[ignore = "requires Docker"]
async fn streams_postgres_rows_without_collecting_all() {
    let session = connect_postgres_fixture().await;
    let mut stream = session.execute(QueryRequest::read("select generate_series(1, 513)", 1000)).await.unwrap();
    let mut batches = 0;
    while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
        if matches!(event.unwrap(), QueryEvent::Rows(_)) { batches += 1; }
    }
    assert!(batches >= 3);
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-driver-postgres --test query -- --ignored`

Expected: FAIL because `PostgresFactory` is absent.

- [x] **Step 3: Implement factory/session/decoder**

Connect using `tokio_postgres::Config` fields, never a logged URL. Spawn connection driver; use `query_raw` and emit batches of at most 256 rows. Decode NULL, bool, signed ints, floats as native text when lossless model lacks float, decimal, text, bytea, JSON, UUID, dates and arrays; unknown OIDs become `DbValue::Native`.

```rust
const ROW_BATCH_SIZE: usize = 256;
pub struct PostgresSession { client: tokio_postgres::Client, cancel: tokio_postgres::CancelToken, capabilities: Vec<CapabilityState> }
```

- [x] **Step 4: Run integration test**

Run: `cargo test -p dexo-driver-postgres --test query -- --ignored`

Expected: PASS with three or more row batches.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-driver-postgres
git commit -m "feat(postgres): stream query results"
```

### Task 3: Implement MySQL query streaming

**Files:** MySQL driver files and integration test `tests/query.rs`.

- [x] **Step 1: Write equivalent failing MySQL test**

```rust
#[tokio::test]
#[ignore = "requires Docker"]
async fn streams_mysql_rows_and_unsigned_values() {
    let session = connect_mysql_fixture().await;
    let mut stream = session.execute(QueryRequest::read("SELECT CAST(18446744073709551615 AS UNSIGNED)", 10)).await.unwrap();
    assert_eq!(first_value(&mut stream).await, DbValue::U64(u64::MAX));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-driver-mysql --test query -- --ignored`

Expected: FAIL because `MysqlFactory` is absent.

- [x] **Step 3: Implement factory/session/decoder**

Build `mysql_async::OptsBuilder` from typed fields and rustls settings. Use `QueryResult::stream_and_drop`, batch at 256, preserve unsigned values, decimal text, enum/set, JSON, zero-date native representation and unknown types.

- [x] **Step 4: Run integration test**

Run: `cargo test -p dexo-driver-mysql --test query -- --ignored`

Expected: PASS with `DbValue::U64(u64::MAX)`.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-driver-mysql
git commit -m "feat(mysql): stream query results"
```

### Task 4: Add bounded application streaming and cancellation

**Files:** `dexo-runtime/src/stream.rs`, `dexo-app/src/query_service.rs`, integration test.

- [x] **Step 1: Write failing backpressure test**

```rust
#[tokio::test]
async fn producer_waits_when_two_batches_are_buffered() {
    let (producer, mut consumer) = dexo_runtime::bounded_events::<u8>(2);
    producer.send(1).await.unwrap();
    producer.send(2).await.unwrap();
    let blocked = tokio::time::timeout(Duration::from_millis(20), producer.send(3)).await;
    assert!(blocked.is_err());
    assert_eq!(consumer.recv().await, Some(1));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-runtime producer_waits_when_two_batches_are_buffered`

Expected: FAIL because `bounded_events` is missing.

- [x] **Step 3: Implement channel and query service**

```rust
pub fn bounded_events<T>(capacity: usize) -> (tokio::sync::mpsc::Sender<T>, tokio::sync::mpsc::Receiver<T>) {
    assert!(capacity > 0); tokio::sync::mpsc::channel(capacity)
}
```

`QueryService::start` registers a runtime task, forwards events through capacity 2, invokes driver cancel when token fires, and always removes the registry entry.

- [x] **Step 4: Run runtime and driver cancellation tests**

Run: `cargo test -p dexo-runtime -p dexo-app && cargo test -p dexo-driver-postgres -p dexo-driver-mysql cancel -- --ignored`

Expected: PASS; long `pg_sleep` and MySQL `SLEEP` end as `Cancelled`.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-runtime crates/dexo-app
git commit -m "feat(query): add bounded streaming and cancellation"
```

### Task 5: Implement transaction and savepoint controls

**Files:** driver API `transaction.rs`, both sessions, `dexo-app/src/transaction_service.rs`.

- [x] **Step 1: Write shared contract test**

```rust
async fn transaction_contract(session: &dyn Session) {
    let tx = session.transactions().expect("transaction capability");
    tx.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.savepoint("before_insert").await.unwrap();
    session.execute(QueryRequest::write("insert into tx_test values (1)")).await.unwrap();
    tx.rollback_to("before_insert").await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count_rows(session, "tx_test").await, 0);
}
```

- [x] **Step 2: Verify contract failure**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql transaction_contract -- --ignored`

Expected: FAIL because transaction methods do not exist.

- [x] **Step 3: Add `TransactionControl` capability**

Define a small `TransactionControl` trait with methods `begin`, `commit`, `rollback`, `savepoint`, `rollback_to`, `release_savepoint`, and state `Idle|Active|Failed|Unknown`; add `Session::transactions() -> Option<&dyn TransactionControl>`. PostgreSQL uses protocol transaction commands; MySQL uses explicit `START TRANSACTION`. Validate savepoint identifiers and never interpolate unvalidated input.

- [x] **Step 4: Run contracts**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql transaction_contract -- --ignored`

Expected: PASS for both databases.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-driver-api crates/dexo-driver-postgres crates/dexo-driver-mysql crates/dexo-app
git commit -m "feat(query): add transaction and savepoint control"
```

### Task 6: Expose query/run through CLI

**Files:** `dexo-cli/src/{args.rs,run.rs,presenter.rs}`, `dexo-cli/tests/query.rs`.

- [x] **Step 1: Write failing non-interactive CLI test**

```rust
#[test]
fn jsonl_query_keeps_diagnostics_off_stdout() {
    Command::cargo_bin("dexo").unwrap()
      .args(["query", "--connection", "fixture", "--sql", "select 1 as n", "--format", "jsonl", "--non-interactive"])
      .assert().success().stdout("{\"n\":1}\n").stderr(predicate::str::is_empty());
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-cli --test query`

Expected: FAIL because query command/presenter is missing.

- [x] **Step 3: Implement public commands**

Add `query` for one statement and `run` for a file/stdin script. Inputs are mutually exclusive; parameters use repeated `--param name=value`; formats are table/csv/tsv/json/jsonl; `--non-interactive` denies any confirmation requirement with exit category `Permission`.

```rust
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum OutputFormat { Table, Csv, Tsv, Json, Jsonl }
```

- [x] **Step 4: Run CLI and Docker E2E**

Run: `cargo test -p dexo-cli --test query && cargo test --workspace -- --ignored`

Expected: PASS; stdout contains data only and cancellation exits with stable code.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo crates/dexo-cli crates/dexo-app
git commit -m "feat(cli): query PostgreSQL and MySQL"
```

### Task 7: Handle reconnect and unknown transaction state

**Files:** `dexo-app/src/session_manager.rs`, tests.

- [x] **Step 1: Write failing safety test**

```rust
#[tokio::test]
async fn disconnect_during_transaction_never_retries_statement() {
    let driver = DisconnectAfterExecute::new();
    let result = manager_with(driver).execute_mutating("update accounts set balance=0").await;
    assert!(matches!(result, Err(AppError { .. })));
    assert_eq!(executions(), 1);
    assert_eq!(manager_state(), SessionState::Unknown);
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-app disconnect_during_transaction_never_retries_statement`

Expected: FAIL because session lifecycle is absent.

- [x] **Step 3: Implement lifecycle**

Define `SessionState::{Connecting, Ready, Transaction, FailedTransaction, Unknown, Closed}`. Retry only pre-execution connection failures for read-only requests marked idempotent. Any disconnect after dispatch of a mutating statement sets `Unknown` and requires a new session.

- [x] **Step 4: Run sprint gate**

Run: `cargo test --workspace && cargo test --workspace -- --ignored && cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS with Docker available.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-app
git commit -m "feat(session): fail safely on uncertain connection state"
```

## Sprint exit

- [x] Both databases stream 513+ rows in bounded batches.
- [x] Cancellation reaches the server and UI-facing task state.
- [x] Transactions/savepoints pass one shared contract suite.
- [x] CLI query/run supports structured formats and non-interactive semantics.
- [x] No mutating operation is retried after uncertain dispatch.
