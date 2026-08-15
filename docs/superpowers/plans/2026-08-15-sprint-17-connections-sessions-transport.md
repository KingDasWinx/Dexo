# Dexo Sprint 17: Connections, Sessions, and Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver complete saved-connection management and ensure every configured policy, secret, TLS, SSH, or proxy option is honored by live PostgreSQL/MySQL sessions.

**Architecture:** Extend the modular driver descriptor and canonical connection request, persist non-secret profile data in SQLite, keep secret purposes in the OS keychain, and own proxy/SSH forwarders through a per-session `TransportLease`. The TUI browser emits effects; only runtime completion actions change connection/session status.

**Tech Stack:** Tokio, tokio-postgres-rustls, mysql_async rustls, rustls 0.23, dexo-transport, keyring 4, secrecy, rusqlite, Ratatui, testcontainers.

---

## Dependencies and file map

Requires Sprint 16 green. Read the connection and transport sections of `docs/superpowers/specs/2026-08-15-dexo-functional-completion-design.md`.

Create:

- `crates/dexo-driver-api/src/transport.rs` — canonical serializable transport request.
- `crates/dexo-transport/src/forward.rs` — local forwarding lease for proxy/SSH.
- `crates/dexo-driver-postgres/src/tls.rs` — PostgreSQL rustls adapter.
- `crates/dexo-tui/src/runtime/connection_manager.rs` — profile/session/secret orchestration.
- `crates/dexo-tui/src/screens/connections.rs` — list and CRUD state.
- `crates/dexo-tui/src/screens/secret_prompt.rs` — explicit session/keychain choice.
- `crates/dexo-tui/tests/connections_flow.rs` — reducer/runtime tests.
- `crates/dexo/tests/transport_live.rs` — ignored live driver transport tests.

Modify:

- `Cargo.toml` and driver/TUI Cargo files — shared TLS/test dependencies.
- `crates/dexo-driver-api/src/connection.rs`, `lib.rs` — descriptor and request.
- `crates/dexo-app/src/connection_profile.rs`, `connection_service.rs`, `connection_policy.rs` — profile schema and validation.
- `crates/dexo-storage/src/migrations.rs`, `connection.rs` — migration 8 and CRUD.
- `crates/dexo-driver-postgres/src/factory.rs`, `session.rs` — transport/TLS/cancel.
- `crates/dexo-driver-mysql/src/factory.rs`, `session.rs` — TLS/forward/cancel generation.
- `crates/dexo-tui/src/{action.rs,model.rs,update.rs,palette.rs,render.rs}` — connection UX.

### Task 1: Make driver connection metadata registry-driven

**Files:** `crates/dexo-driver-api/src/connection.rs`, both driver factories, `crates/dexo-app/src/driver_registry.rs`, tests in `crates/dexo-driver-api/tests/contracts.rs`.

- [x] **Step 1: Write the failing descriptor contract**

```rust
#[test]
fn factories_describe_connection_defaults_without_tui_hardcoding() {
    let descriptor = PostgresFactory.descriptor();
    assert_eq!(descriptor.id, "postgres");
    assert_eq!(descriptor.default_port, 5432);
    assert!(descriptor.options.tls && descriptor.options.ssh && descriptor.options.proxy);
}
```

- [x] **Step 2: Run it**

Run: `cargo test -p dexo-driver-api --test contracts factories_describe_connection_defaults_without_tui_hardcoding`

Expected: FAIL because `descriptor` does not exist.

- [x] **Step 3: Add and implement the descriptor**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionOptions { pub tls: bool, pub client_certificate: bool, pub ssh: bool, pub proxy: bool }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_port: u16,
    pub options: ConnectionOptions,
}

pub trait ConnectionFactory: Send + Sync {
    fn descriptor(&self) -> DriverDescriptor;
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError>;
}
```

Derive the connection form driver selector and defaults from `DriverRegistry::descriptors()`; remove TUI `match` statements selecting 5432/3306.

- [x] **Step 4: Test and commit**

Run: `cargo test -p dexo-driver-api -p dexo-app -p dexo-driver-postgres -p dexo-driver-mysql`

Expected: PASS.

```powershell
git add crates/dexo-driver-api crates/dexo-app/src/driver_registry.rs crates/dexo-driver-postgres/src/factory.rs crates/dexo-driver-mysql/src/factory.rs
git commit -m "feat(drivers): expose modular connection descriptors"
```

### Task 2: Persist groups, explicit policies, and secret purposes

**Files:** `crates/dexo-storage/src/migrations.rs`, `connection.rs`, `crates/dexo-app/src/connection_profile.rs`, `connection_policy.rs`, tests in `crates/dexo-storage/tests/migration.rs` and `project_repository.rs`.

- [x] **Step 1: Add a migration-8 test**

```rust
#[test]
fn migration_8_moves_the_legacy_password_ref_and_preserves_profiles() {
    let db = database_at_version(7);
    apply_pending(db.connection()).unwrap();
    let purposes: i64 = db.connection().query_row(
        "select count(*) from connection_secret_refs where purpose='database_password'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(purposes, 1);
    assert!(ConnectionRepository::new(db.connection()).get_by_name("legacy").unwrap().is_some());
}
```

- [x] **Step 2: Run and confirm failure**

Run: `cargo test -p dexo-storage --test migration migration_8_moves_the_legacy_password_ref_and_preserves_profiles`

Expected: FAIL at schema version 7.

- [x] **Step 3: Add migration 8 and canonical policy types**

```sql
BEGIN;
ALTER TABLE connections ADD COLUMN group_path TEXT;
ALTER TABLE connections ADD COLUMN policy_json TEXT NOT NULL DEFAULT '{}';
CREATE TABLE connection_secret_refs(
  connection_id TEXT NOT NULL,
  purpose TEXT NOT NULL,
  secret_ref TEXT NOT NULL,
  PRIMARY KEY(connection_id, purpose),
  FOREIGN KEY(connection_id) REFERENCES connections(id) ON DELETE CASCADE
);
INSERT INTO connection_secret_refs(connection_id,purpose,secret_ref)
  SELECT id,'database_password',secret_ref FROM connections;
INSERT INTO schema_migrations(version,applied_at) VALUES(8,datetime('now'));
COMMIT;
```

Add `ConnectionPolicyOverrides` with explicit read-only, destructive confirmation, verified TLS, row limit, and timeout fields. Custom environment labels must resolve defaults only through persisted policy, not `Environment::parse` fallback.

- [x] **Step 4: Add repository CRUD and commit**

Implement `update`, `duplicate` (new UUID and new secret refs), `test_input` without save, `delete`, `list_for_project`, and group moves. Export config without any secret refs; import generates fresh refs.

Run: `cargo test -p dexo-storage -p dexo-app`

Expected: migrations and profile round trips PASS.

```powershell
git add crates/dexo-storage/src/migrations.rs crates/dexo-storage/src/connection.rs crates/dexo-storage/tests crates/dexo-app/src/connection_profile.rs crates/dexo-app/src/connection_policy.rs
git commit -m "feat(storage): persist connection groups policies and secret purposes"
```

### Task 3: Define and validate the canonical transport request

**Files:** `crates/dexo-driver-api/src/transport.rs`, `connection.rs`, `lib.rs`, `crates/dexo-app/src/connection_profile.rs`, tests in `crates/dexo-app/src/connection_profile.rs`.

- [ ] **Step 1: Write invalid/insecure transport tests**

```rust
#[test]
fn production_profile_rejects_unverified_tls_and_invalid_proxy() {
    let profile = production_profile_with_transport(TlsMode::Disable, ProxyMode::Http { host: "".into(), port: 0 });
    let error = profile.connect_request(secret_map()).unwrap_err();
    assert_eq!(error.category(), ErrorCategory::Configuration);
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-app production_profile_rejects_unverified_tls_and_invalid_proxy`

Expected: FAIL because transport config is ignored.

- [ ] **Step 3: Add serializable non-secret request types**

```rust
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TransportRequest {
    pub target_host: String,
    pub target_port: u16,
    pub tls: Option<TlsRequest>,
    pub route: RouteRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RouteRequest { Direct, Socks5 { host: String, port: u16 }, HttpConnect { host: String, port: u16 }, Ssh(SshRequest) }
```

`ConnectRequest` carries `transport` plus a redacted `ConnectionSecrets` map keyed by purpose. Validate paths, ports, TLS modes, hostnames, read-only policy, and required secret purposes before opening a socket.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-driver-api -p dexo-app transport`

Expected: safe configurations PASS and insecure/invalid configurations fail closed.

```powershell
git add crates/dexo-driver-api/src/transport.rs crates/dexo-driver-api/src/connection.rs crates/dexo-driver-api/src/lib.rs crates/dexo-app/src/connection_profile.rs
git commit -m "feat(connect): define validated transport requests"
```

### Task 4: Build a real proxy/SSH `TransportLease`

**Files:** `crates/dexo-transport/src/forward.rs`, `lib.rs`, tests `crates/dexo-transport/tests/forward.rs`.

- [ ] **Step 1: Write forwarding and shutdown tests**

```rust
#[tokio::test]
async fn lease_forwards_multiple_connections_and_stops_on_drop() {
    let target = EchoServer::start().await;
    let lease = TransportLease::direct(target.address()).await.unwrap();
    assert_eq!(roundtrip(lease.endpoint(), b"one").await, b"one");
    assert_eq!(roundtrip(lease.endpoint(), b"two").await, b"two");
    let endpoint = lease.endpoint();
    lease.close().await;
    assert!(tokio::net::TcpStream::connect(endpoint).await.is_err());
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-transport --test forward`

Expected: FAIL because the lease is missing.

- [ ] **Step 3: Implement bounded local forwarding**

```rust
pub struct TransportLease {
    endpoint: std::net::SocketAddr,
    cancel: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

async fn forward_one(mut local: tokio::net::TcpStream, mut remote: BoxStream) -> Result<(), TransportError> {
    tokio::io::copy_bidirectional(&mut local, &mut remote).await
        .map(|_| ()).map_err(|e| TransportError::Io(e.to_string()))
}
```

Bind `127.0.0.1:0`, cap concurrent forwarded sockets, and open each remote stream with `connect_proxy` or `open_ssh_tunnel`. Host-key-new returns a typed confirmation request; host-key-changed always fails.

- [ ] **Step 4: Run all transport tests and commit**

Run: `cargo test -p dexo-transport --all-features`

Expected: direct, proxy, SSH, host key, TLS, multiple connection, and cleanup tests PASS.

```powershell
git add crates/dexo-transport/src/forward.rs crates/dexo-transport/src/lib.rs crates/dexo-transport/tests/forward.rs
git commit -m "feat(transport): add owned proxy and ssh forwarding leases"
```

### Task 5: Apply TLS and transport in PostgreSQL and MySQL

**Files:** workspace/driver Cargo files, `crates/dexo-driver-postgres/src/tls.rs`, `factory.rs`, `session.rs`, `crates/dexo-driver-mysql/src/factory.rs`, `session.rs`, `crates/dexo/tests/transport_live.rs`.

- [ ] **Step 1: Add ignored verified-TLS and routed-cancel tests**

Create cases for trusted CA, hostname mismatch, client certificate, proxy/SSH route, cancellation through a lease, and MySQL cancel after session generation changes.

Run: `cargo test -p dexo --test transport_live -- --ignored --nocapture`

Expected: FAIL because factories force direct/no-TLS connections.

- [ ] **Step 2: Implement PostgreSQL rustls connection and cancel connector**

Build a `rustls::ClientConfig` from system roots plus optional CA/client identity. For routed connections, connect PostgreSQL to the lease endpoint while setting the TLS server name to the original host. Store the connector/route factory in `PostgresSession`; cancellation must use it rather than `NoTls`.

```rust
pub struct PostgresCancelContext {
    pub config: tokio_postgres::Config,
    pub transport: dexo_driver_api::TransportRequest,
}
```

- [ ] **Step 3: Implement MySQL `SslOpts` and routed killer connection**

Map CA, client identity, verification flags, and original-host override into `mysql_async::SslOpts`. Build the main and killer `Opts` from the same effective endpoint/TLS configuration. Reject a cancel request whose generation differs from the active connection ID generation.

- [ ] **Step 4: Run Docker tests and commit**

Run: `cargo test -p dexo --test transport_live -- --ignored --nocapture`

Expected: all PostgreSQL/MySQL TLS, route, and cancel cases PASS.

```powershell
git add Cargo.toml crates/dexo-driver-postgres crates/dexo-driver-mysql crates/dexo/tests/transport_live.rs
git commit -m "feat(drivers): connect through verified transports"
```

### Task 6: Make keychain fallback explicit and secret-safe

**Files:** `crates/dexo-tui/src/screens/secret_prompt.rs`, `runtime/connection_manager.rs`, `screens/connection.rs`, `model.rs`, `update.rs`, tests `connections_flow.rs`.

- [ ] **Step 1: Write locked-keychain and removal-choice tests**

```rust
#[tokio::test]
async fn locked_keychain_prompts_instead_of_silently_using_memory() {
    let manager = manager_with_store(UnavailableSecretStore);
    let action = manager.connect(saved_profile()).await.unwrap_err();
    assert!(matches!(action, Action::SecretRequired { purpose: SecretPurpose::DatabasePassword, .. }));
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test connections_flow locked_keychain_prompts_instead_of_silently_using_memory`

Expected: FAIL because `SessionSecrets::put` silently falls back.

- [ ] **Step 3: Implement a redacted prompt and explicit persistence choice**

```rust
pub enum SecretChoice { SessionOnly(secrecy::SecretString), SaveToKeychain(secrecy::SecretString), Cancel }

pub struct SecretBuffer(secrecy::SecretString);

impl std::fmt::Debug for SecretBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("SecretBuffer([REDACTED])") }
}
```

Remove silent memory fallback. Delete shows `KeepSecrets`/`DeleteSecrets`; keychain deletion failure leaves the profile intact until the user chooses profile-only removal.

- [ ] **Step 4: Run sentinel tests and commit**

Run: `cargo test -p dexo-secrets -p dexo-tui --test connections_flow && cargo test -p dexo-storage --test sentinel`

Expected: prompt tests PASS and no sentinel leaks.

```powershell
git add crates/dexo-tui/src/screens/secret_prompt.rs crates/dexo-tui/src/runtime/connection_manager.rs crates/dexo-tui/src/screens/connection.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/connections_flow.rs
git commit -m "feat(tui): prompt explicitly for unavailable secrets"
```

### Task 7: Build the real connection browser and multi-session UX

**Files:** `crates/dexo-tui/src/screens/connections.rs`, `action.rs`, `model.rs`, `update.rs`, `palette.rs`, `render.rs`, `runtime/connection_manager.rs`, tests `connections_flow.rs`.

- [ ] **Step 1: Write reducer/runtime acceptance tests**

Test startup listing, connect/switch, edit, duplicate, test without save, delete, group move, custom environment policy, two sessions for one profile, session close, read-only enforcement, safe reconnect, and stale connect completion.

Run: `cargo test -p dexo-tui --test connections_flow`

Expected: new cases FAIL.

- [ ] **Step 2: Add browser state and effects**

```rust
pub struct ConnectionsScreen {
    pub open: bool,
    pub profiles: Vec<ConnectionRow>,
    pub sessions: Vec<SessionRow>,
    pub selected_profile: usize,
    pub selected_session: Option<SessionId>,
    pub form: ConnectionForm,
    pub pending: Option<OperationId>,
}
```

All CRUD/test/connect commands emit effects. Only `ProfilesLoaded`, `ConnectionTested`, `ProfileSaved`, `ProfileDeleted`, `SessionConnected`, `SessionClosed`, or typed failure actions alter visible outcome.

- [ ] **Step 3: Render and apply capabilities**

Show group tree, project/environment markers, route/TLS/read-only indicators, session count, transaction state, and disabled reasons. The form shows only descriptor-supported options and validates before dispatch.

- [ ] **Step 4: Run snapshots and commit**

Run: `cargo test -p dexo-tui --test connections_flow && cargo test -p dexo-tui --test snapshots`

Expected: connection flows and intentional snapshots PASS.

```powershell
git add crates/dexo-tui/src/screens/connections.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/palette.rs crates/dexo-tui/src/render.rs crates/dexo-tui/src/runtime/connection_manager.rs crates/dexo-tui/tests/connections_flow.rs
git commit -m "feat(tui): manage saved connections and live sessions"
```

### Task 8: Execute the connection/transport sprint gate

- [ ] **Step 1: Run static and non-Docker gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
```

Expected: PASS with no `TlsMode` warning.

- [ ] **Step 2: Run live matrix tests**

```powershell
cargo test -p dexo-transport --all-features
cargo test -p dexo --test transport_live -- --ignored --nocapture
cargo test -p dexo --test tui_query_live -- --ignored --nocapture
```

Expected: PostgreSQL and MySQL connect, test, query, cancel, read-only, TLS, proxy, and SSH flows PASS.

- [ ] **Step 3: Verify profile exports and logs contain no secrets**

Run: `cargo test -p dexo --test config_roundtrip && cargo test -p dexo-storage --test sentinel`

Expected: PASS.

- [ ] **Step 4: Commit the verified sprint state**

```powershell
git add .
git commit -m "test(connect): verify connection and transport verticals"
```

## Sprint 17 exit checklist

- [ ] Existing profiles load on startup without auto-connecting.
- [ ] List/edit/duplicate/test/delete/group/custom environment operations persist.
- [ ] Keychain locked/missing always prompts.
- [ ] TLS/CA/mTLS/SSH/proxy reach live drivers and cancel paths.
- [ ] Multiple sessions, read-only, close, switch, and safe reconnect work.
- [ ] TUI connection fields are descriptor-driven for future official drivers.
- [ ] Complete sprint gate is green.
