# Dexo Sprint 01: Local State and Secrets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Persistir projetos e configurações não sensíveis localmente, proteger credenciais no keychain e recuperar estado sem cloud.

**Architecture:** `dexo-storage` owns SQLite/repositories and `dexo-secrets` owns opaque secret references. `dexo-app` coordinates both through traits so tests never require the real OS keychain.

**Tech Stack:** rusqlite 0.40.2 bundled, keyring 4.1.6, directories 6.0, Serde 1.0.229, TOML 1.1, UUID 1.24, secrecy 0.10.3, zeroize 1.9, tempfile 3.27.

---

## File map

- Create: `crates/dexo-storage/src/{database.rs,migrations.rs,project.rs,connection.rs,recovery.rs,lib.rs}`
- Create: `crates/dexo-secrets/src/{store.rs,keyring_store.rs,memory_store.rs,lib.rs}`
- Create: `crates/dexo-app/src/{project.rs,connection_profile.rs}`
- Test: `crates/dexo-storage/tests/{migration.rs,project_repository.rs,recovery.rs}`
- Test: `crates/dexo-secrets/tests/secret_store.rs`
- Modify: `crates/dexo-cli/src/{args.rs,run.rs}`; test CLI in `crates/dexo/tests/config_roundtrip.rs`

### Task 1: Resolve native application paths and TOML config

**Files:** `crates/dexo-storage/src/database.rs`, `crates/dexo-storage/src/lib.rs`.

- [x] **Step 1: Write a failing path test**

```rust
#[test]
fn explicit_data_home_wins() {
    let paths = dexo_storage::AppPaths::from_data_home("C:/tmp/dexo-test".into());
    assert_eq!(paths.database.file_name().unwrap(), "dexo.db");
    assert_eq!(paths.config.file_name().unwrap(), "config.toml");
}
```

- [x] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-storage explicit_data_home_wins`

Expected: FAIL because `AppPaths` is undefined.

- [x] **Step 3: Implement paths without global environment mutation**

```rust
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths { pub data_dir: PathBuf, pub database: PathBuf, pub config: PathBuf }

impl AppPaths {
    pub fn from_data_home(data_dir: PathBuf) -> Self {
        Self { database: data_dir.join("dexo.db"), config: data_dir.join("config.toml"), data_dir }
    }
    pub fn discover() -> anyhow::Result<Self> {
        let dirs = directories::ProjectDirs::from("dev", "dexo", "Dexo")
            .ok_or_else(|| anyhow::anyhow!("platform data directory is unavailable"))?;
        Ok(Self::from_data_home(dirs.data_local_dir().to_path_buf()))
    }
}
```

- [x] **Step 4: Run test**

Run: `cargo test -p dexo-storage explicit_data_home_wins`

Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-storage
git commit -m "feat(storage): resolve native application paths"
```

### Task 2: Create versioned SQLite migrations

**Files:** `crates/dexo-storage/src/{database.rs,migrations.rs}`, `crates/dexo-storage/tests/migration.rs`.

- [x] **Step 1: Write failing migration test**

```rust
use dexo_storage::Database;
#[test]
fn fresh_database_reaches_schema_one() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 1);
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-storage --test migration`

Expected: FAIL because `Database` is missing.

- [x] **Step 3: Implement migration one**

Define `MIGRATION_1` in `migrations.rs` with concrete tables `projects`, `connections`, `recovery_documents`, and `schema_migrations`. `connections` must contain `secret_ref TEXT NOT NULL` and must not contain password/secret columns.

```rust
pub const MIGRATION_1: &str = r#"
BEGIN;
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE projects(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE connections(id TEXT PRIMARY KEY, project_id TEXT, name TEXT NOT NULL,
  driver TEXT NOT NULL, environment TEXT NOT NULL, config_json TEXT NOT NULL,
  secret_ref TEXT NOT NULL, FOREIGN KEY(project_id) REFERENCES projects(id));
CREATE TABLE recovery_documents(id TEXT PRIMARY KEY, project_id TEXT NOT NULL,
  title TEXT NOT NULL, content TEXT NOT NULL, updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id));
INSERT INTO schema_migrations(version, applied_at) VALUES(1, datetime('now'));
COMMIT;
"#;
```

`Database::open` creates a recoverable `.bak` copy before any future destructive migration; `open_in_memory` skips backup and runs the same SQL.

- [x] **Step 4: Run migration tests**

Run: `cargo test -p dexo-storage --test migration`

Expected: PASS and `PRAGMA foreign_keys` equals `1`.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-storage
git commit -m "feat(storage): add versioned local schema"
```

### Task 3: Implement project and connection repositories

**Files:** `crates/dexo-storage/src/{project.rs,connection.rs}`, `crates/dexo-app/src/{project.rs,connection_profile.rs}`.

- [x] **Step 1: Write a failing repository test**

```rust
#[test]
fn deleting_connection_does_not_delete_secret() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let repo = dexo_storage::ConnectionRepository::new(db.connection());
    let profile = dexo_app::ConnectionProfile {
        id: dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        project_id: None,
        name: "local-pg".into(), driver: "postgres".into(), environment: "local".into(),
        config: serde_json::json!({"host":"localhost","port":5432}),
        secret_ref: dexo_app::SecretRef::new("secret-123".into()),
    };
    repo.save(&profile).unwrap();
    repo.delete(profile.id).unwrap();
    assert!(repo.get(profile.id).unwrap().is_none());
    assert_eq!(profile.secret_ref.as_str(), "secret-123");
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-storage --test project_repository`

Expected: FAIL with missing repositories/types.

- [x] **Step 3: Implement stable records**

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionId(pub uuid::Uuid);

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionProfile {
    pub id: ConnectionId,
    pub project_id: Option<uuid::Uuid>,
    pub name: String,
    pub driver: String,
    pub environment: String,
    pub config: serde_json::Value,
    pub secret_ref: SecretRef,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SecretRef(String);
impl SecretRef { pub fn new(value: String) -> Self { Self(value) } pub fn as_str(&self) -> &str { &self.0 } }
```

Repository methods use bound rusqlite parameters, return `Result`, and never call the secret store.

- [x] **Step 4: Run repository tests**

Run: `cargo test -p dexo-storage --test project_repository`

Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-app crates/dexo-storage
git commit -m "feat(storage): persist projects and connection profiles"
```

### Task 4: Isolate the OS keychain behind a secret store

**Files:** `crates/dexo-secrets/src/{store.rs,memory_store.rs,keyring_store.rs,lib.rs}`, `crates/dexo-secrets/tests/secret_store.rs`.

- [x] **Step 1: Write contract tests against memory store**

```rust
use dexo_secrets::{MemorySecretStore, SecretStore};
use secrecy::ExposeSecret;

#[test]
fn secret_round_trip_and_delete() {
    let store = MemorySecretStore::default();
    store.put("conn-1", "hunter2").unwrap();
    assert_eq!(store.get("conn-1").unwrap().unwrap().expose_secret(), "hunter2");
    store.delete("conn-1").unwrap();
    assert!(store.get("conn-1").unwrap().is_none());
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-secrets --test secret_store`

Expected: FAIL because the trait/store is undefined.

- [x] **Step 3: Implement trait and stores**

```rust
use secrecy::SecretString;
pub trait SecretStore: Send + Sync {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}
```

`MemorySecretStore` uses `Mutex<HashMap<String, SecretString>>`. `KeyringSecretStore` creates `keyring::Entry` with service `dev.dexo.connection` and the opaque reference as user. Map unavailable/locked keychain to `SecretError::Unavailable`; never fall back to a file.

- [x] **Step 4: Run contract and lint**

Run: `cargo test -p dexo-secrets --test secret_store && cargo clippy -p dexo-secrets --all-targets -- -D warnings`

Expected: PASS and no debug output containing `hunter2`.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-secrets
git commit -m "feat(secrets): integrate opaque keychain storage"
```

### Task 5: Add crash-recovery documents

**Files:** `crates/dexo-storage/src/recovery.rs`, `crates/dexo-storage/tests/recovery.rs`.

- [x] **Step 1: Write failing checkpoint test**

```rust
#[test]
fn latest_checkpoint_replaces_older_content() {
    let db = dexo_storage::Database::open_in_memory().unwrap();
    let repo = dexo_storage::RecoveryRepository::new(db.connection());
    repo.checkpoint("doc-1", "project-1", "scratch", "select 1").unwrap();
    repo.checkpoint("doc-1", "project-1", "scratch", "select 2").unwrap();
    assert_eq!(repo.load("doc-1").unwrap().unwrap().content, "select 2");
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo-storage --test recovery`

Expected: FAIL because `RecoveryRepository` is missing.

- [x] **Step 3: Implement atomic upsert and clear**

Use one bound `INSERT ... ON CONFLICT(id) DO UPDATE` statement and provide `load`, `list_for_project`, and `clear`. Checkpoint content is document SQL only; it must not serialize connection secrets or parameter values.

- [x] **Step 4: Run tests**

Run: `cargo test -p dexo-storage --test recovery`

Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-storage
git commit -m "feat(storage): checkpoint recoverable documents"
```

### Task 6: Export/import secret-free configuration

**Files:** `crates/dexo-cli/src/{args.rs,run.rs}`, `crates/dexo-storage/src/connection.rs`, test `crates/dexo/tests/config_roundtrip.rs`.

- [x] **Step 1: Write failing sentinel test**

```rust
#[test]
fn exported_config_contains_reference_not_secret() {
    let output = export_fixture_with_secret("SUPER_SECRET_SENTINEL");
    assert!(output.contains("secret_ref"));
    assert!(!output.contains("SUPER_SECRET_SENTINEL"));
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test -p dexo --test config_roundtrip`

Expected: FAIL because export is absent.

- [x] **Step 3: Implement commands and DTO**

Add `dexo config export --output <path>` and `dexo config import --input <path>`. Serialize a versioned TOML DTO containing projects, non-secret connection configuration and an empty `secret_ref` import marker. Import generates fresh secret references and reports which connections need a secret.

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct PortableConfig { version: u32, projects: Vec<PortableProject>, connections: Vec<PortableConnection> }
```

- [x] **Step 4: Run full sprint gate**

Run: `cargo test -p dexo-storage -p dexo-secrets -p dexo-cli && cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS; sentinel absent from stdout, stderr and generated file.

- [x] **Step 5: Commit**

```bash
git add crates/dexo-cli crates/dexo-storage
git commit -m "feat(config): export and import secret-free settings"
```

## Sprint exit

- [x] SQLite schema v1 migrates from a fresh database.
- [x] No local table or TOML field can store a plaintext password.
- [x] Real keychain failures request per-session input rather than file fallback.
- [x] Projects, connections and recovery documents round-trip.
- [x] Portable export passes the secret sentinel test.
