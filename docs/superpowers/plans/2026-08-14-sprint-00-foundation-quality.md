# Dexo Sprint 00: Foundation and Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Criar o workspace Rust, os contratos básicos de aplicação/runtime, um binário mínimo e todos os gates de qualidade.

**Architecture:** O binário `dexo` compõe crates focados; interfaces externas dependem de `dexo-app`, e tarefas assíncronas vivem em `dexo-runtime`. Esta sprint não conecta a banco e não cria abstrações ainda não usadas.

**Tech Stack:** Rust 1.93.1, edition 2024, Tokio 1.53.1, Clap 4.6.6, thiserror 2.0.20, tracing 0.1.44, cargo-nextest, cargo-deny.

---

## File map

- Create: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`
- Create: `.config/nextest.toml`, `.github/workflows/ci.yml`
- Create: `crates/*/Cargo.toml` and `crates/*/src/lib.rs` for every crate listed in the roadmap
- Create: `crates/dexo/src/main.rs`
- Create: `crates/dexo-app/src/{lib.rs,error.rs,event.rs}`
- Create: `crates/dexo-runtime/src/{lib.rs,task.rs}`
- Create: `crates/dexo-cli/src/{lib.rs,args.rs,run.rs}`
- Create: `crates/dexo-test-support/src/{lib.rs,clock.rs}`
- Test: `crates/dexo/tests/cli_smoke.rs`, `crates/dexo-runtime/tests/task_registry.rs`

### Task 1: Bootstrap the Cargo workspace

**Files:** root manifests and all `crates/*` manifests/lib entrypoints.

- [ ] **Step 1: Verify the workspace is absent**

Run: `cargo metadata --no-deps`

Expected: FAIL because `Cargo.toml` does not exist.

- [ ] **Step 2: Generate crate directories**

Run exactly:

```powershell
$libs = 'dexo-app','dexo-cli','dexo-tui','dexo-mcp','dexo-driver-api','dexo-driver-postgres','dexo-driver-mysql','dexo-sql','dexo-storage','dexo-secrets','dexo-transport','dexo-runtime','dexo-test-support'
New-Item -ItemType Directory -Force crates | Out-Null
cargo new crates/dexo --bin --vcs none
foreach ($name in $libs) { cargo new "crates/$name" --lib --vcs none }
```

Expected: fourteen packages created without `.git` directories.

- [ ] **Step 3: Replace the root workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.93"
license = "MIT OR Apache-2.0"
repository = "https://github.com/kingdaswinx/Dexo"

[workspace.dependencies]
anyhow = "1.0.104"
async-trait = "0.1.92"
clap = { version = "4.6.6", features = ["derive", "env"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.20"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "signal", "sync", "time"] }
tokio-util = { version = "0.7.19", features = ["rt"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "fmt"] }
uuid = { version = "1.24.0", features = ["serde", "v4"] }
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.93.1"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

- [ ] **Step 4: Verify all packages compile**

Run: `cargo check --workspace`

Expected: PASS and a new `Cargo.lock`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml crates
git commit -m "build: bootstrap Dexo workspace"
```

### Task 2: Define application errors and events

**Files:** `crates/dexo-app/src/error.rs`, `crates/dexo-app/src/event.rs`, `crates/dexo-app/src/lib.rs`.

- [ ] **Step 1: Write the failing error contract test**

Append to `crates/dexo-app/src/error.rs` after creating the module:

```rust
#[cfg(test)]
mod tests {
    use super::{AppError, ErrorCategory};

    #[test]
    fn public_error_never_exposes_technical_source() {
        let error = AppError::new(ErrorCategory::Network, "connection failed")
            .with_technical("password=hunter2");
        assert_eq!(error.to_string(), "connection failed");
        assert_eq!(error.category(), ErrorCategory::Network);
    }
}
```

- [ ] **Step 2: Run the test and observe failure**

Run: `cargo test -p dexo-app public_error_never_exposes_technical_source`

Expected: FAIL because `AppError` and `ErrorCategory` are undefined.

- [ ] **Step 3: Implement the minimal stable error model**

Create `crates/dexo-app/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    Configuration, Authentication, Network, Transport, Permission, Syntax,
    Conflict, Timeout, Cancelled, Capability, Storage, ExternalTool, McpPolicy, Internal,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppError {
    category: ErrorCategory,
    message: String,
    technical: Option<String>,
}

impl AppError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self { category, message: message.into(), technical: None }
    }
    pub fn with_technical(mut self, technical: impl Into<String>) -> Self {
        self.technical = Some(technical.into()); self
    }
    pub fn category(&self) -> ErrorCategory { self.category }
    pub fn technical(&self) -> Option<&str> { self.technical.as_deref() }
}
```

Create `crates/dexo-app/src/event.rs`:

```rust
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaskId(pub Uuid);

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    TaskStarted(TaskId),
    TaskProgress { id: TaskId, completed: u64, total: Option<u64> },
    TaskFinished(TaskId),
    TaskFailed { id: TaskId, message: String },
}
```

Export both modules from `crates/dexo-app/src/lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p dexo-app`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-app
git commit -m "feat(app): define stable errors and events"
```

### Task 3: Build the cancellable task registry

**Files:** `crates/dexo-runtime/src/task.rs`, `crates/dexo-runtime/src/lib.rs`, `crates/dexo-runtime/tests/task_registry.rs`.

- [ ] **Step 1: Write the failing integration test**

```rust
use dexo_runtime::TaskRegistry;

#[tokio::test]
async fn cancellation_reaches_registered_task() {
    let registry = TaskRegistry::default();
    let task = registry.register();
    assert!(!task.token.is_cancelled());
    assert!(registry.cancel(task.id));
    task.token.cancelled().await;
    assert!(task.token.is_cancelled());
}
```

- [ ] **Step 2: Verify failure**

Run: `cargo test -p dexo-runtime --test task_registry`

Expected: FAIL with unresolved `TaskRegistry`.

- [ ] **Step 3: Implement registry and handle**

Create `crates/dexo-runtime/src/task.rs`:

```rust
use std::{collections::HashMap, sync::Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RuntimeTaskId(pub Uuid);

pub struct TaskHandle { pub id: RuntimeTaskId, pub token: CancellationToken }

#[derive(Default)]
pub struct TaskRegistry { tokens: Mutex<HashMap<RuntimeTaskId, CancellationToken>> }

impl TaskRegistry {
    pub fn register(&self) -> TaskHandle {
        let id = RuntimeTaskId(Uuid::new_v4());
        let token = CancellationToken::new();
        self.tokens.lock().expect("task registry poisoned").insert(id, token.clone());
        TaskHandle { id, token }
    }
    pub fn cancel(&self, id: RuntimeTaskId) -> bool {
        self.tokens.lock().expect("task registry poisoned").get(&id)
            .map(|token| { token.cancel(); true }).unwrap_or(false)
    }
    pub fn finish(&self, id: RuntimeTaskId) { self.tokens.lock().expect("task registry poisoned").remove(&id); }
}
```

Export the types from `lib.rs` and add workspace dependencies `tokio-util` and `uuid` to the crate.

- [ ] **Step 4: Run test**

Run: `cargo test -p dexo-runtime --test task_registry`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-runtime
git commit -m "feat(runtime): add cancellable task registry"
```

### Task 4: Add the first stable CLI contract

**Files:** `crates/dexo-cli/src/{args.rs,run.rs,lib.rs}`, `crates/dexo/src/main.rs`, `crates/dexo/tests/cli_smoke.rs`.

- [ ] **Step 1: Write failing CLI tests**

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_uses_stdout() {
    Command::cargo_bin("dexo").unwrap().arg("--version")
        .assert().success().stdout(predicate::str::starts_with("dexo 0.1.0"));
}

#[test]
fn doctor_is_non_interactive() {
    Command::cargo_bin("dexo").unwrap().args(["doctor", "--json"])
        .assert().success().stdout(predicate::str::contains("\"status\":\"ok\""));
}
```

- [ ] **Step 2: Verify failure**

Run: `cargo test -p dexo --test cli_smoke`

Expected: FAIL because the binary has no `doctor` command.

- [ ] **Step 3: Implement args and runner**

Create `crates/dexo-cli/src/args.rs`:

```rust
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "dexo", version, about = "Local-first terminal database workbench")]
pub struct Args { #[command(subcommand)] pub command: Option<Command> }

#[derive(Debug, Subcommand)]
pub enum Command { Doctor { #[arg(long)] json: bool } }
```

Create `crates/dexo-cli/src/run.rs`:

```rust
use crate::args::{Args, Command};

pub fn run(args: Args) -> anyhow::Result<()> {
    match args.command {
        Some(Command::Doctor { json: true }) => println!(r#"{{"status":"ok"}}"#),
        Some(Command::Doctor { json: false }) => println!("Dexo: ok"),
        None => println!("Dexo TUI is not available in Sprint 00"),
    }
    Ok(())
}
```

Export `args` and `run`; make `crates/dexo/src/main.rs` parse `Args` and call `run`. Add `assert_cmd` and `predicates` as dev-dependencies.

- [ ] **Step 4: Run tests**

Run: `cargo test -p dexo --test cli_smoke`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo crates/dexo-cli
git commit -m "feat(cli): establish version and doctor contracts"
```

### Task 5: Install formatting, lint, dependency and CI gates

**Files:** `rustfmt.toml`, `deny.toml`, `.config/nextest.toml`, `.github/workflows/ci.yml`.

- [ ] **Step 1: Run the intended gates before configuration**

Run: `cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo nextest run --workspace`

Expected: formatting/clippy pass; nextest may fail if not installed locally.

- [ ] **Step 2: Add deterministic configuration**

Create `rustfmt.toml`:

```toml
edition = "2024"
newline_style = "Unix"
use_field_init_shorthand = true
```

Create `.config/nextest.toml`:

```toml
[profile.default]
slow-timeout = { period = "30s", terminate-after = 2 }
fail-fast = false

[profile.ci]
fail-fast = true
junit.path = "junit.xml"
```

Create `deny.toml` allowing `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`, and denying unknown registries/git sources.

- [ ] **Step 3: Add native OS CI**

Create `.github/workflows/ci.yml` with a matrix of `ubuntu-latest`, `macos-latest`, `windows-latest`; run `cargo fmt`, Clippy, `cargo nextest run --workspace`, and `cargo deny check` on Ubuntu. Pin actions by full commit SHA during implementation, not floating tags.

- [ ] **Step 4: Run local gates**

Run: `cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace`

Expected: PASS with zero warnings.

- [ ] **Step 5: Commit**

```bash
git add rustfmt.toml deny.toml .config .github
git commit -m "ci: enforce Rust quality gates"
```

### Task 6: Add deterministic test support

**Files:** `crates/dexo-test-support/src/clock.rs`, `crates/dexo-test-support/src/lib.rs`.

- [ ] **Step 1: Write failing fake clock test**

```rust
#[cfg(test)]
mod tests {
    use super::{Clock, FakeClock};
    use std::time::{Duration, SystemTime};
    #[test]
    fn fake_clock_advances_deterministically() {
        let start = SystemTime::UNIX_EPOCH;
        let clock = FakeClock::new(start);
        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.now(), start + Duration::from_secs(5));
    }
}
```

- [ ] **Step 2: Verify failure**

Run: `cargo test -p dexo-test-support fake_clock_advances_deterministically`

Expected: FAIL because `FakeClock` is missing.

- [ ] **Step 3: Implement clock contract**

```rust
use std::{sync::Mutex, time::{Duration, SystemTime}};

pub trait Clock: Send + Sync { fn now(&self) -> SystemTime; }
pub struct SystemClock;
impl Clock for SystemClock { fn now(&self) -> SystemTime { SystemTime::now() } }
pub struct FakeClock(Mutex<SystemTime>);
impl FakeClock {
    pub fn new(now: SystemTime) -> Self { Self(Mutex::new(now)) }
    pub fn advance(&self, by: Duration) { let mut now = self.0.lock().unwrap(); *now += by; }
}
impl Clock for FakeClock { fn now(&self) -> SystemTime { *self.0.lock().unwrap() } }
```

- [ ] **Step 4: Run the full sprint gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-test-support
git commit -m "test: add deterministic clock support"
```

## Sprint exit

- [ ] `cargo metadata --no-deps` lists fourteen packages.
- [ ] `cargo test --workspace` passes without Docker.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `dexo --version` and `dexo doctor --json` match the public contract.
- [ ] CI contains native Linux, macOS and Windows jobs.
