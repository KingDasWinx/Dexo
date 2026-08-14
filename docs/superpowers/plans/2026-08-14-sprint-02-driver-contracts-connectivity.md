# Dexo Sprint 02: Driver Contracts and Connectivity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Definir contratos modulares de drivers e estabelecer transportes TCP, TLS, proxy e SSH verificáveis.

**Architecture:** `dexo-driver-api` contains small capability-oriented traits and lossless values. `dexo-transport` returns generic async streams; concrete database drivers consume streams without knowing how they were created.

**Tech Stack:** async-trait 0.1.92, futures-core/bytes 1.x, Tokio 1.53 net/io, rustls 0.23.43, tokio-postgres-rustls 0.14, rustls-platform-verifier 0.7, russh 0.62.6, secrecy 0.10.3.

---

## File map

- Create: `crates/dexo-driver-api/src/{capability.rs,identifier.rs,value.rs,query.rs,connection.rs,error.rs,lib.rs}`
- Create: `crates/dexo-transport/src/{config.rs,tcp.rs,proxy.rs,tls.rs,ssh.rs,host_key.rs,lib.rs}`
- Create: `crates/dexo-app/src/connection_policy.rs`
- Test: `crates/dexo-driver-api/tests/contracts.rs`
- Test: `crates/dexo-transport/tests/{proxy.rs,tls.rs,host_key.rs}`

### Task 1: Define canonical identifiers, values and capabilities

**Files:** `dexo-driver-api` files above.

- [x] **Step 1: Write failing round-trip tests**

```rust
use dexo_driver_api::{Capability, CapabilityState, DbValue, QualifiedName};

#[test]
fn value_and_identifier_preserve_native_information() {
    let name = QualifiedName::new(Some("db"), Some("public"), "orders");
    assert_eq!(name.display_unquoted(), "db.public.orders");
    let value = DbValue::Native { type_name: "ltree".into(), bytes: b"a.b".to_vec(), text: "a.b".into() };
    assert_eq!(value.type_name(), Some("ltree"));
}

#[test]
fn unavailable_capability_keeps_reason() {
    let state = CapabilityState::unavailable(Capability::ExplainAnalyze, "server version");
    assert_eq!(state.reason(), Some("server version"));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-driver-api --test contracts`

Expected: FAIL with unresolved exported types.

- [x] **Step 3: Implement the exact domain types**

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Capability { Catalog, Query, Cancel, Transactions, DataWrite, Ddl, Explain, ExplainAnalyze, Admin, Import, Export }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityState { pub capability: Capability, pub available: bool, reason: Option<String> }
impl CapabilityState {
    pub fn available(capability: Capability) -> Self { Self { capability, available: true, reason: None } }
    pub fn unavailable(capability: Capability, reason: impl Into<String>) -> Self {
        Self { capability, available: false, reason: Some(reason.into()) }
    }
    pub fn reason(&self) -> Option<&str> { self.reason.as_deref() }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DbValue { Null, Bool(bool), I64(i64), U64(u64), Decimal(String), Text(String), Bytes(Vec<u8>), Json(String), Native { type_name: String, bytes: Vec<u8>, text: String } }
```

Implement `QualifiedName` with optional catalog/schema and non-empty object name. Export all types.

- [x] **Step 4: Run tests**

Run: `cargo test -p dexo-driver-api --test contracts`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-driver-api
git commit -m "feat(driver-api): define canonical values and capabilities"
```

### Task 2: Define small async driver contracts

**Files:** `connection.rs`, `query.rs`, `error.rs`, `lib.rs`.

- [x] **Step 1: Write a compile-time fake driver test**

```rust
use dexo_driver_api::*;
struct FakeFactory;
#[async_trait::async_trait]
impl ConnectionFactory for FakeFactory {
    fn driver_name(&self) -> &'static str { "fake" }
    async fn connect(&self, _: ConnectRequest) -> Result<Box<dyn Session>, DriverError> { Err(DriverError::unsupported("fake")) }
}
#[test]
fn factory_is_object_safe() { let _: Box<dyn ConnectionFactory> = Box::new(FakeFactory); }
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-driver-api factory_is_object_safe`

Expected: FAIL because contracts are absent.

- [x] **Step 3: Implement contracts**

```rust
#[derive(Clone, Debug)]
pub struct ConnectRequest { pub endpoint: String, pub database: Option<String>, pub username: String, pub secret: secrecy::SecretString, pub read_only: bool }

#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync {
    fn driver_name(&self) -> &'static str;
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError>;
}

#[async_trait::async_trait]
pub trait Session: Send + Sync {
    fn capabilities(&self) -> &[CapabilityState];
    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError>;
    async fn cancel(&self, query: QueryId) -> Result<(), DriverError>;
    async fn close(self: Box<Self>) -> Result<(), DriverError>;
}

#[derive(Clone, Debug)]
pub struct QueryId(pub uuid::Uuid);
#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub id: QueryId, pub sql: String, pub parameters: Vec<DbValue>,
    pub row_limit: u64, pub timeout: std::time::Duration, pub mutating: bool,
}
impl QueryRequest {
    pub fn read(sql: impl Into<String>, row_limit: u64) -> Self {
        Self { id: QueryId(uuid::Uuid::new_v4()), sql: sql.into(), parameters: vec![], row_limit,
            timeout: std::time::Duration::from_secs(30), mutating: false }
    }
    pub fn write(sql: impl Into<String>) -> Self {
        Self { mutating: true, ..Self::read(sql, 0) }
    }
}
```

Define `ColumnMeta`, `RowBatch`, `QueryEvent`, and `QueryStream = Pin<Box<dyn Stream<Item=Result<QueryEvent, DriverError>> + Send>>`. `DriverError` contains stable category, safe message, native code/position, and retryable flag.

- [x] **Step 4: Compile all dependents**

Run: `cargo test -p dexo-driver-api && cargo check --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-driver-api
git commit -m "feat(driver-api): add object-safe connection and query contracts"
```

### Task 3: Build TCP and proxy connectors

**Files:** `crates/dexo-transport/src/{config.rs,tcp.rs,proxy.rs,lib.rs}`, test `proxy.rs`.

- [x] **Step 1: Write failing config validation test**

```rust
#[test]
fn rejects_proxy_without_port() {
    let config = ProxyConfig::http_connect("proxy.internal", 0);
    assert_eq!(config.validate().unwrap_err().to_string(), "proxy port must be between 1 and 65535");
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-transport --test proxy`

Expected: FAIL because `ProxyConfig` is undefined.

- [x] **Step 3: Implement transport contracts**

```rust
pub trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T> AsyncStream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
pub type BoxStream = Box<dyn AsyncStream>;

#[derive(Clone, Debug)]
pub enum ProxyConfig { Socks5 { host: String, port: u16 }, HttpConnect { host: String, port: u16 } }
```

Implement direct `TcpStream::connect`, SOCKS5 via `tokio-socks`, and HTTP CONNECT by writing a bounded request and accepting only 2xx. Credentials use `SecretString` and never implement `Debug` as plaintext.

- [x] **Step 4: Run proxy tests with a local fake server**

Run: `cargo test -p dexo-transport --test proxy`

Expected: PASS for direct rejection, valid CONNECT and non-2xx error.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-transport
git commit -m "feat(transport): add TCP and proxy connectors"
```

### Task 4: Enforce verified TLS by default

**Files:** `tls.rs`, `config.rs`, test `tls.rs`.

- [x] **Step 1: Write failing policy test**

```rust
#[test]
fn insecure_tls_requires_explicit_flag() {
    let config = TlsConfig { mode: TlsMode::DisableVerification, explicit_insecure: false, server_name: "db.local".into(), ca_file: None };
    assert!(matches!(config.validate(), Err(TransportError::UnsafeConfiguration(_))));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-transport --test tls`

Expected: FAIL because TLS config is absent.

- [x] **Step 3: Implement verified modes**

Define `TlsMode::{Preferred, Required, VerifyCa, VerifyFull, DisableVerification}`. Build rustls using platform verifier by default; load a PEM CA only from the explicit path; load client certificate/key through typed inputs. `DisableVerification` compiles behind feature `dangerous-tls` and still requires `explicit_insecure=true`.

- [x] **Step 4: Run TLS tests**

Run: `cargo test -p dexo-transport --all-features --test tls`

Expected: PASS for trusted test CA, hostname mismatch, expired cert and explicit insecure mode.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-transport
git commit -m "feat(transport): verify TLS by default"
```

### Task 5: Verify SSH host keys and open tunnels

**Files:** `ssh.rs`, `host_key.rs`, test `host_key.rs`.

- [x] **Step 1: Write failing changed-key test**

```rust
#[test]
fn changed_host_key_is_never_accepted() {
    let known = KnownHost { host: "bastion", port: 22, fingerprint: "SHA256:old".into() };
    let decision = verify_host_key(Some(&known), "SHA256:new");
    assert_eq!(decision, HostKeyDecision::Changed);
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-transport --test host_key`

Expected: FAIL because host-key model is missing.

- [x] **Step 3: Implement verification and tunnel API**

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostKeyDecision { Trusted, New { fingerprint: String }, Changed }

pub struct SshTunnelRequest {
    pub bastion_host: String, pub bastion_port: u16, pub username: String,
    pub auth: SshAuth, pub target_host: String, pub target_port: u16,
}
```

Use `russh` client authentication for agent, private key or password. The host-key callback returns an error for `Changed`, returns a typed confirmation requirement for `New`, and accepts only `Trusted` without interaction.

- [x] **Step 4: Run tests**

Run: `cargo test -p dexo-transport --test host_key`

Expected: PASS, including new/trusted/changed cases.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-transport
git commit -m "feat(transport): add verified SSH tunneling"
```

### Task 6: Apply connection safety policy before transport

**Files:** `crates/dexo-app/src/connection_policy.rs` and tests in the same module.

- [x] **Step 1: Write failing production policy test**

```rust
#[test]
fn production_defaults_to_strict_controls() {
    let policy = ConnectionPolicy::for_environment(Environment::Production);
    assert!(policy.confirm_destructive);
    assert!(policy.require_verified_tls);
    assert_eq!(policy.max_rows, 10_000);
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-app production_defaults_to_strict_controls`

Expected: FAIL because the policy is missing.

- [x] **Step 3: Implement policy values**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment { Local, Development, Staging, Production }
pub struct ConnectionPolicy { pub read_only: bool, pub confirm_destructive: bool, pub require_verified_tls: bool, pub max_rows: u64, pub timeout_secs: u64 }
```

Production defaults: confirmations on, verified TLS required, 10k rows, 30 seconds. Local defaults: confirmations on for DROP/TRUNCATE, 100k rows, 120 seconds. Explicit user configuration may tighten or loosen except the UI must retain an insecure indicator.

- [x] **Step 4: Run full sprint gate**

Run: `cargo test -p dexo-driver-api -p dexo-transport -p dexo-app && cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-app
git commit -m "feat(app): enforce connection safety policies"
```

## Sprint exit

- [x] Driver traits are object-safe and contain no TUI/MCP types.
- [x] Values preserve native bytes/type names without lossy conversion.
- [x] TLS verification and SSH host-key changes fail closed.
- [x] Direct, SOCKS5, HTTP CONNECT and SSH connectors have local integration tests.
- [x] Production policy is strict and tested.
