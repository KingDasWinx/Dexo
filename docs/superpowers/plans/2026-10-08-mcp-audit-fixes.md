# MCP Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix every finding of `docs/audits/2026-10-08-mcp-audit.md` (C1..L29), security first, so the MCP server, its CLI and the Agents screen do what the docs say.

**Architecture:** Each task fixes one layer and compiles on its own: the SQL guard (dexo-sql), the database opening (dexo-storage), the grant ledger and write path (dexo-app, dexo-storage, dexo-mcp), the read tools and server handler (dexo-mcp), the CLI (dexo-cli), the core library (dexo-app), the TUI (dexo-tui), then the docs. Tests are added only where a security or data-loss property has no other check; everything else is verified with `cargo build`, the existing suites, and a run.

**Tech Stack:** Rust 1.93 (edition 2024), sqlparser 0.62, rmcp 3.1.2, rusqlite 0.40 (bundled SQLite), ratatui, insta.

**Spec:** `docs/audits/2026-10-08-mcp-audit.md` (the findings, each with file and fix) and `docs/superpowers/specs/2026-10-07-agents-access-redesign-design.md` (the model the fixes must keep).

## Global Constraints

- Every commit passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo deny check` and `cargo test --workspace`. One commit per task, no push.
- Config files get the setting only, never a comment block.
- No new tests except where a task says so (a security or data-loss property). Existing tests that break because behavior changed on purpose are updated, not deleted.
- No new crate. `libc` is already in `Cargo.lock` through tokio; adding it as a direct dependency of `crates/dexo` is allowed.
- Scratch files, scratch databases and tmux sessions go under `/tmp/claude-1000/-home-winx-Documents-github-Dexo/3362019c-44c3-4f42-9ae5-c427590a3d80/scratchpad/`. Every run of a dev `dexo` exports `DEXO_DATA_HOME` and `HOME` to a scratch dir. Never touch `~/.local/share/dexo`.
- The model stays: a grant is a profile's write access to one connection, kinds `rows`/`schema`/`sessions`, one active per (profile, connection), every write asks. A grant never widens what the profile sees.
- Hidden objects and denied tools answer `not found`.

## Review Focus

1. A Postgres `U&"…"` identifier anywhere in a read, a data write or a DDL — the guard must refuse it, never run it (Task 1 test).
2. A database written by a newer Dexo opened by an older one — the older one must refuse with a message, and never move or copy the file (Task 2 test).
3. A write access revoked or expired while a client is connected — the client's tool list must change (Task 4 test).
4. A legacy grant from before the redesign — after migration 20 it must be revoked, never silently widened (Task 3 test).
5. A destructive DDL without `confirm_target` — the agent must be told to type the target, not that it waits for approval (Task 5 test).

---

### Task 1: SQL guard — refuse Unicode-escaped identifiers, list FK targets, deny `currval`

Fixes C1, M1, L1.

**Files:**
- Modify: `crates/dexo-sql/src/statement_guard.rs` (`parse_one` ~569, `inspect_schema_write` ~196, `DENIED_FUNCTIONS` ~109, tests module ~740)

**Interfaces:**
- Produces: `GuardRejection::UnicodeEscape` (new variant), `pub fn inspect_schema_write` now lists `foreign_table` of every FK constraint.

- [ ] **Step 1: Write the failing tests** at the end of the `mod tests` in `statement_guard.rs`:

```rust
    /// sqlparser does not decode `U&"…"` identifiers: it reads `U & f(args)` as a bitwise
    /// op, so the function name is compared raw while Postgres decodes it. Every escaped
    /// identifier is refused before parsing; the plain spellings are caught as before.
    #[test]
    fn unicode_escaped_identifiers_are_refused() {
        for sql in [
            r#"select U&"pg\005fread\005ffile"('/etc/hostname')"#,
            r#"select u&"pg_sleep"(1)"#,
            r#"select U&'\0041' as a"#,
            r#"select * from U&"stores""#,
        ] {
            assert_eq!(
                refused(sql),
                GuardRejection::UnicodeEscape,
                "{sql}"
            );
            assert_eq!(
                inspect_data_write(sql, Dialect::Postgres).unwrap_err(),
                GuardRejection::UnicodeEscape,
                "{sql}"
            );
        }
        assert_eq!(
            refused(r#"select "PG_READ_FILE"('/etc/hostname')"#),
            GuardRejection::Function("pg_read_file".into())
        );
        // MySQL has no U& syntax: the text is an ordinary (unparsable) identifier.
        assert!(inspect_read(r#"select U&"x" from t"#, Dialect::Mysql).is_err());
        assert_eq!(refused("select currval('s')"), GuardRejection::Function("currval".into()));
    }

    /// A foreign key couples a table to the one it references: the referenced table is
    /// a target of the DDL, so a schema grant is held to it too.
    #[test]
    fn foreign_key_targets_are_schema_write_targets() {
        let ddl = |sql: &str| inspect_schema_write(sql, Dialect::Postgres).unwrap().relations;
        assert_eq!(
            ddl("alter table a add constraint fk foreign key (x) references secret(id)"),
            vec![path(&["a"]), path(&["secret"])]
        );
        assert_eq!(
            ddl("create table a (x int references secret(id), y int, constraint fk2 foreign key (y) references other(id))"),
            vec![path(&["a"]), path(&["other"]), path(&["secret"])]
        );
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dexo-sql unicode_escaped_identifiers_are_refused foreign_key_targets_are_schema_write_targets`
Expected: compile error (`UnicodeEscape` does not exist) — then, after adding only the variant, both tests FAIL.

- [ ] **Step 3: Add the variant and the pre-parse refusal**

In `GuardRejection` add:

```rust
    #[error("Unicode-escaped identifiers (U&\"…\") are not allowed")]
    UnicodeEscape,
```

Add `"currval",` after `"nextval",` in `DENIED_FUNCTIONS`.

Add above `parse_one`:

```rust
/// Whether `sql` holds a Postgres Unicode-escaped identifier or string, `U&"…"` or
/// `U&'…'`, in any case, outside an ordinary quote. sqlparser keeps the raw spelling
/// while the server decodes it, so a denied name written that way slips every list.
// ponytail: refuses every U& escape rather than decoding them; decode in `path()` if an
// agent ever needs one.
fn has_unicode_escape(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'\'' | b'"' | b'`' => quote = Some(byte),
                b'U' | b'u'
                    if bytes.get(index + 1) == Some(&b'&')
                        && matches!(bytes.get(index + 2), Some(b'"') | Some(b'\''))
                        && !index
                            .checked_sub(1)
                            .is_some_and(|at| bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') =>
                {
                    return true;
                }
                _ => {}
            },
        }
        index += 1;
    }
    false
}
```

At the top of `parse_one`, before `let mut statements`:

```rust
    if matches!(dialect, Dialect::Postgres | Dialect::Duckdb) && has_unicode_escape(sql) {
        return Err(GuardRejection::UnicodeEscape);
    }
```

- [ ] **Step 4: List foreign-key targets in `inspect_schema_write`**

Add to the `use sqlparser::ast::{…}` line: `AlterTableOperation, TableConstraint`.

Replace the `let named: Vec<ObjectName> = match &statement { … };` block with:

```rust
    let mut named: Vec<ObjectName> = match &statement {
        Statement::CreateTable(table) => referenced_tables(&table.constraints),
        Statement::AlterTable(alter) => alter
            .operations
            .iter()
            .filter_map(|operation| match operation {
                AlterTableOperation::AddConstraint { constraint, .. } => Some(constraint),
                _ => None,
            })
            .flat_map(|constraint| referenced_tables(std::slice::from_ref(constraint)))
            .collect(),
        Statement::CreateIndex(_) => Vec::new(),
        Statement::Drop { names, .. } => names.clone(),
        Statement::CreateView(view) => vec![view.name.clone()],
        _ => return Err(GuardRejection::WrongKind),
    };
    if let Statement::CreateTable(table) = &statement {
        // A column's own `REFERENCES other(id)` is a constraint of the column.
        for column in &table.columns {
            for option in &column.options {
                if let sqlparser::ast::ColumnOption::ForeignKey { foreign_table, .. } = &option.option {
                    named.push(foreign_table.clone());
                }
            }
        }
    }
```

and add the helper above the function:

```rust
/// The tables a set of constraints points at: sqlparser visits no FK's `REFERENCES`
/// name as a relation, so a schema write to a table is held to the tables it couples to.
fn referenced_tables(constraints: &[TableConstraint]) -> Vec<ObjectName> {
    constraints
        .iter()
        .filter_map(|constraint| match constraint {
            TableConstraint::ForeignKey(key) => Some(key.foreign_table.clone()),
            _ => None,
        })
        .collect()
}
```

If `ColumnOption::ForeignKey` has different field names in sqlparser 0.62 (`cargo build` says so), read `~/.cargo/registry/src/*/sqlparser-0.62.0/src/ast/ddl.rs` for the variant and name the field it uses for the referenced table.

- [ ] **Step 5: Run the guard suite**

Run: `cargo test -p dexo-sql`
Expected: all pass, the two new tests included. If `schema_writes_name_their_targets` now sees an FK target it did not list before, that is the fix working: update its expectation.

- [ ] **Step 6: Commit**

```bash
git add crates/dexo-sql/src/statement_guard.rs
git commit -m "fix(sql): the guard refuses Unicode-escaped identifiers, holds DDL to the tables it references, denies currval

sqlparser reads U&\"pg\\005fread\\005ffile\" as U & pg\\005fread\\005ffile(...) and keeps the name raw while Postgres decodes it, so every denied function was one escape away."
```

---

### Task 2: An older Dexo refuses a newer database instead of archiving it

Fixes C2 (with its WAL part), L29; `mcp_backend` opens the database once.

**Files:**
- Modify: `crates/dexo-storage/src/database.rs:53-64, 110-188` and its tests (~215-260, ~320-335)
- Modify: `crates/dexo-cli/src/mcp_backend.rs:12-36, 78-115`
- Modify: `crates/dexo-cli/src/plugin.rs` (`summary`, reached from `dexo doctor`) — read it first with `grep -n "pub fn summary" -A 40 crates/dexo-cli/src/plugin.rs`

**Interfaces:**
- Produces: `Database::open` returns `Err` for a schema newer than `LATEST_SCHEMA_VERSION`; `dexo_storage::leftover_archives(data_dir) -> Vec<PathBuf>`.

- [ ] **Step 1: Replace the two recovery tests** in `database.rs` (`open_recovers_from_unsupported_schema_with_compatible_backup`, `open_resets_unsupported_schema_without_compatible_backup`) with one:

```rust
    /// A database a newer Dexo wrote is left exactly as it is: the older build refuses
    /// it and says to upgrade. Archiving it and restoring the backup forked the data under
    /// the processes still writing to it.
    #[test]
    fn open_refuses_a_database_from_a_newer_dexo_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        Database::open(&path).unwrap();
        fs::copy(&path, backup_path(&path)).unwrap();
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES(23, datetime('now'))",
                [],
            )
            .unwrap();
        }
        let before: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        let error = Database::open(&path).unwrap_err().to_string();
        assert!(error.contains("newer Dexo") && error.contains("schema 23"), "{error}");
        let after: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(before, after, "nothing moved, copied or created");
        assert_eq!(read_schema_version(&rusqlite::Connection::open(&path).unwrap()), 23);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p dexo-storage open_refuses_a_database_from_a_newer_dexo`
Expected: FAIL — `open` returns `Ok` and the directory gained `dexo.db.unsupported-v23`.

- [ ] **Step 3: Replace the recovery**

Delete `unsupported_archive_path`, `unique_archive_path`, `archive_incompatible_database` and the whole `recover_unsupported_schema`. In their place:

```rust
/// A database a newer Dexo wrote is refused, and left as it is. Every migration here is
/// additive, but an older build cannot know that; moving the file aside and restoring a
/// backup forked the data under the processes still writing to it.
fn refuse_newer_schema(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let version = read_schema_version_at_path(path)?;
    anyhow::ensure!(
        version <= LATEST_SCHEMA_VERSION,
        "{} was written by a newer Dexo (schema {version}); this build reads schema {LATEST_SCHEMA_VERSION}. Upgrade Dexo, or run the newer one: an older `dexo` on PATH and a newer one in a project both open this file.",
        path.display()
    );
    Ok(())
}

/// The copies an older Dexo moved aside before this check existed: `dexo.db.unsupported-v*`
/// beside the database. They are the person's data; `dexo doctor` names them.
pub fn leftover_archives(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut archives: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".unsupported-v"))
        })
        .collect();
    archives.sort();
    archives
}
```

In `open`, replace `recover_unsupported_schema(path)?;` with `refuse_newer_schema(path)?;`. In `move_database_bundle` (kept for `backup_before_destructive_migration` if it uses it — `grep -n move_database_bundle` says), replace `let _ = fs::rename(from_extra, to_extra);` with `fs::rename(from_extra, to_extra)?;`; if nothing else calls it, delete it. Export `leftover_archives` from `crates/dexo-storage/src/lib.rs` next to `apply_pending`.

- [ ] **Step 4: Open the database once in the MCP backend**

In `crates/dexo-cli/src/mcp_backend.rs` replace the struct, `new` and `saved`:

```rust
/// Hands the MCP adapter the keychain, the drivers and SQLite. The database is opened
/// once, when the server starts: opened per call, an older build ran its schema check
/// on every tool call, against a file a newer Dexo was writing.
pub struct CliMcpBackend {
    registry: DriverRegistry,
    db: std::sync::Mutex<Database>,
}

impl CliMcpBackend {
    pub fn new(registry: DriverRegistry, db: Database) -> Self {
        Self {
            registry,
            db: std::sync::Mutex::new(db),
        }
    }

    fn with_db<T>(&self, read: impl FnOnce(&Database) -> Result<T, AppError>) -> Result<T, AppError> {
        let db = self.db.lock().map_err(|_| AppError::new(ErrorCategory::Storage, "the database lock was poisoned"))?;
        read(&db)
    }

    fn saved(&self, name: &str) -> Result<ConnectionProfile, AppError> {
        self.with_db(|db| {
            ConnectionRepository::new(db.connection())
                .get_by_name(name)
                .map_err(storage)?
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::Configuration,
                        format!("unknown connection '{name}'"),
                    )
                })
        })
    }
}
```

Every `let db = Database::open(&self.database).map_err(storage)?;` in the file becomes a `self.with_db(|db| { … })` block around the lines that use `db` (the lock is never held across an `.await`: in `catalog_snapshot` lock for `load_latest`, drop, `await refresh_catalog`, lock again for `replace_snapshot`). Change `drop(db)` in `mcp_serve` (`run.rs` ~2085) to pass the `Database` into `CliMcpBackend::new(registry, db)`. Fix the backend's own `#[cfg(test)]` constructor the same way.

- [ ] **Step 5: `dexo doctor` names the leftover archives**

In `crates/dexo-cli/src/plugin.rs` `summary`, after whatever it prints about the database path (read the function), add:

```rust
    let archives = dexo_storage::leftover_archives(&paths.data_dir);
    if !archives.is_empty() {
        println!(
            "older copies of the database, moved aside by an earlier Dexo: {}. They hold what was written before each move; delete them once you no longer need them.",
            archives.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
        );
    }
```

If `summary` has a `json` branch, add `"database_archives": archives` to it.

- [ ] **Step 6: Build, test, and see the message end to end**

Run: `cargo build -p dexo && cargo test -p dexo-storage -p dexo-cli`
Expected: pass.

Then, with `S=/tmp/claude-1000/-home-winx-Documents-github-Dexo/3362019c-44c3-4f42-9ae5-c427590a3d80/scratchpad/newer; mkdir -p $S/dh $S/home`, run `DEXO_DATA_HOME=$S/dh HOME=$S/home target/debug/dexo doctor` once (creates the DB), then `sqlite3 $S/dh/dexo.db "INSERT INTO schema_migrations(version, applied_at) VALUES(99, datetime('now'))"`, then `DEXO_DATA_HOME=$S/dh HOME=$S/home target/debug/dexo mcp doctor` and `… target/debug/dexo` in a tmux pane. Expected: both print `dexo.db was written by a newer Dexo (schema 99)…` and exit; `ls $S/dh` shows no `unsupported` file. If the TUI shows nothing and exits silently, find where `Database::open` is called at startup (`grep -n "Database::open" crates/dexo-tui/src/event.rs crates/dexo-tui/src/runtime/*.rs crates/dexo/src/main.rs`) and print the error to stderr before exiting.

- [ ] **Step 7: Commit**

```bash
git add crates/dexo-storage crates/dexo-cli
git commit -m "fix(storage): a database from a newer Dexo is refused, not moved aside and replaced by its backup

An older dexo on PATH, started by an agent, archived the live file under the newer TUI's feet on every tool call. The MCP backend now opens the database once, and dexo doctor names the copies earlier builds left."
```

---

### Task 3: Migration 20 revokes write access given before the redesign

Fixes H1.

**Files:**
- Modify: `crates/dexo-storage/src/migrations.rs` (`LATEST_SCHEMA_VERSION`, after `MIGRATION_19`, `apply_up_to` match)
- Modify: `crates/dexo-storage/src/mcp/mod.rs` tests (`a_spent_grant_does_not_come_back_with_migration_19`)
- Modify: `crates/dexo-storage/src/database.rs` tests asserting `schema_version() == 19`

- [ ] **Step 1: Write the failing test** — replace `a_spent_grant_does_not_come_back_with_migration_19` in `crates/dexo-storage/src/mcp/mod.rs`:

```rust
    /// Write access given before the redesign was under other terms: its own tables, no
    /// approval. Migration 19 dropped those terms; migration 20 revokes what it widened,
    /// so a person gives it again on the new terms.
    #[test]
    fn migrations_19_and_20_revoke_every_grant_made_before_the_redesign() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_up_to(&conn, 18).unwrap();
        for (id, uses, ask) in [
            ("00000000-0000-0000-0000-000000000001", 0, 0),
            ("00000000-0000-0000-0000-000000000002", 1, 0),
            ("00000000-0000-0000-0000-000000000003", 4294967295i64, 120),
        ] {
            conn.execute(
                "INSERT INTO mcp_grants (id, profile_name, connection_name, capability, tools_json,
                    selectors_json, expires_at, remaining_uses, revision, revoked, ask_secs)
                 VALUES (?1, 'assistant', 'local', 'data_write', '[\"data_insert\"]',
                    '[\"db.public.items\"]', 9223372036854775807, ?2, 1, 0, ?3)",
                rusqlite::params![id, uses, ask],
            )
            .unwrap();
        }
        crate::migrations::apply_pending(&conn).unwrap();
        assert!(super::grant_repo::list_active(&conn, "assistant", 0).unwrap().is_empty());
        let revoked: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_grants WHERE revoked = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(revoked, 3);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p dexo-storage migrations_19_and_20`
Expected: FAIL — two of the three stay active.

- [ ] **Step 3: Add the migration**

In `migrations.rs`: `pub const LATEST_SCHEMA_VERSION: u32 = 20;`, then after `MIGRATION_19`:

```rust
/// Write access given before 1.5 covered tables of its own and did not ask; migration 19
/// dropped those terms, which widened every live grant to all the profile sees. They are
/// revoked: a person gives write access again, on the new terms.
pub const MIGRATION_20: &str = r#"
BEGIN;
UPDATE mcp_grants SET revoked = 1, revision = revision + 1 WHERE revoked = 0;
INSERT INTO schema_migrations(version, applied_at) VALUES(20, datetime('now'));
COMMIT;
"#;
```

and `20 => MIGRATION_20,` in `apply_up_to`. In `database.rs` tests, `19` → `20` wherever `schema_version()` is asserted.

- [ ] **Step 4: Run the storage suite**

Run: `cargo test -p dexo-storage`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add crates/dexo-storage
git commit -m "fix(storage): migration 20 revokes write access given before the redesign

Migration 19 dropped each grant's own tables, which widened a grant for one table to everything its profile sees, until revoked."
```

---

### Task 4: The tool list changes when the tools change; `subscriptions/listen` is answered

Fixes H2, M7, M8.

**Files:**
- Modify: `crates/dexo-mcp/src/server.rs` (`Inner`, `new`, `visible_tools`, `on_initialized`, `ServerHandler` impl)
- Modify: `crates/dexo-app/src/mcp/ledger.rs` (remove `revision`), `crates/dexo-storage/src/mcp/mod.rs` and `grant_repo.rs` (remove `revision`)
- Modify: `crates/dexo-app/src/mcp/grant.rs` (`Grant::lists_sessions`)
- Modify: `crates/dexo-mcp/src/schema.rs:96`
- Modify: `crates/dexo-mcp/tests/protocol.rs` (`a_grant_publishes_its_tool_and_revoking_removes_it`)

**Interfaces:**
- Produces: `impl Inner { pub(crate) fn visible_tools(&self) -> Vec<String> }`; `Grant::lists_sessions(&self) -> bool`; `GrantLedger` loses `fn revision`.

- [ ] **Step 1: Extend the protocol test** `a_grant_publishes_its_tool_and_revoking_removes_it` in `crates/dexo-mcp/tests/protocol.rs`: after `ledger.revoke(id).unwrap(); client.notifications.clear();` (read the test; keep its existing assertions) add, before the test ends:

```rust
    // The second change notifies too: the list is compared, not a counter that stalled.
    assert!(
        client
            .saw_notification("notifications/tools/list_changed")
            .await,
        "revoking did not announce the change"
    );
    assert!(!client.tool_names().await.contains(&"data_insert".to_string()));
    client.notifications.clear();
    let again = Grant::new(&profile(), "local", &[WriteKind::Rows], dexo_mcp::tools_write::now_secs(), DEFAULT_TTL_SECS, 120).unwrap();
    ledger.insert_grant(again).unwrap();
    assert!(client.saw_notification("notifications/tools/list_changed").await);
    assert!(client.tool_names().await.contains(&"data_insert".to_string()));
```

This test uses `MemoryGrantLedger`, whose counter works, so it passes before and after; it pins the behavior the SQLite poller must now share. Run it once to see it pass.

- [ ] **Step 2: Compare the tool list in the poller**

In `server.rs`, `Inner`: replace `last_revision: Mutex<u64>` with `last_tools: Mutex<Vec<String>>`, initialised `Mutex::new(Vec::new())` in `new`. Move the body of `DexoMcpServer::visible_tools` into `impl Inner`:

```rust
impl Inner {
    /// What a client may see right now: the profile's reads, plus the writes an active
    /// grant publishes on a connection that takes them (MCP-002, MCP-004), and the
    /// sessions list while a grant lets sessions be ended. `tools/call` is gated by the
    /// same list.
    pub(crate) fn visible_tools(&self) -> Vec<String> {
        let profile = &self.service.profile;
        let now = now_secs();
        let mut tools: Vec<String> = advertised_tools(profile)
            .into_iter()
            .map(str::to_string)
            .collect();
        let accepts_writes = |name: &str| {
            self.router
                .connections()
                .find(|connection| connection.name == name)
                .is_some_and(|connection| connection.accepts_writes().is_ok())
        };
        tools.extend(
            write_tool_names(self.ledger.as_ref(), &profile.name, now, &accepts_writes)
                .into_iter()
                .filter(|tool| profile.tool_allowed(tool)),
        );
        let lists_sessions = self
            .ledger
            .active_grants(&profile.name, now)
            .iter()
            .any(|grant| grant.lists_sessions() && accepts_writes(&grant.connection));
        if lists_sessions
            && profile.tool_allowed("admin_list_sessions")
            && !tools.iter().any(|tool| tool == "admin_list_sessions")
        {
            tools.push("admin_list_sessions".into());
        }
        tools
    }
}
```

and make `DexoMcpServer::visible_tools` `self.inner.visible_tools()`. In `on_initialized`, set the baseline before the loop and compare lists:

```rust
        let inner = Arc::clone(&self.inner);
        let peer = context.peer.clone();
        *inner.last_tools.lock().expect("tools") = inner.visible_tools();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = inner.stop.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
                let tools = inner.visible_tools();
                let changed = {
                    let mut last = inner.last_tools.lock().expect("tools");
                    let changed = tools != *last;
                    *last = tools;
                    changed
                };
                if changed {
                    let _ = peer.notify_tool_list_changed().await;
                }
            }
        });
```

Add to `grant.rs`:

```rust
    /// It lets sessions be ended, so the sessions list is published with it: the
    /// `session_id` those tools take comes from `admin_list_sessions`.
    pub fn lists_sessions(&self) -> bool {
        self.kinds().contains(&WriteKind::Sessions)
    }
```

- [ ] **Step 3: Remove `revision` from the ledger**

Delete `fn revision(&self) -> u64;` from the `GrantLedger` trait, its two impls (`ledger.rs:121-123`, `storage/mcp/mod.rs:66-69`), `grant_repo::revision`, and the `revision` field of `Inner` in the memory ledger if nothing else reads it (`grep -rn "\.revision\b" crates/dexo-app/src/mcp crates/dexo-storage/src/mcp` — the `Grant.revision` column stays).

- [ ] **Step 4: Answer `subscriptions/listen`**

In `server.rs` add `SubscriptionFilter` to the `rmcp::model::{…}` import and, inside `impl ServerHandler for DexoMcpServer`:

```rust
    /// A client on protocol 2026-07-28 listens for list changes here instead of polling;
    /// every category it asks for is accepted, and the SDK keeps what the capabilities
    /// advertise (tools' list_changed). Unanswered, it was method-not-found every poll.
    fn accepted_subscription_filter(&self, requested: &SubscriptionFilter) -> Option<SubscriptionFilter> {
        Some(requested.clone())
    }
```

Reword `schema.rs:96`: `/// Server session to act on, from `admin_list_sessions`, which is listed while this profile may end sessions.`

- [ ] **Step 5: Probe the SQLite path by hand** (the regression only shows with the SQLite ledger)

Build, then with a scratch `DEXO_DATA_HOME`/`HOME` (connection `pg-dev`, profile `assistant` enabled on it), run `target/debug/dexo mcp serve --profile assistant` in a tmux pane fed by a FIFO, send `initialize` (protocolVersion `2026-07-28`) + `notifications/initialized`, then in another shell run `dexo mcp grant create --profile assistant --connection pg-dev --allow rows`, `grant revoke …`, `grant create …`, `grant revoke …`, 1 s apart. Expected: four `notifications/tools/list_changed` lines on the server's stdout, none before the first command. Then send `{"jsonrpc":"2.0","id":9,"method":"subscriptions/listen","params":{"notifications":{"toolsListChanged":true}}}`: expected no `-32601` and no `WARN … subscriptions/listen` in `$DEXO_DATA_HOME/logs/dexo.log`.

- [ ] **Step 6: Run the suite, then commit**

Run: `cargo test -p dexo-mcp -p dexo-app -p dexo-storage`

```bash
git add crates/dexo-mcp crates/dexo-app crates/dexo-storage
git commit -m "fix(mcp): the tool list is announced on every change, sessions can be listed while they may be ended, and subscriptions/listen is answered

The poller compared MAX(revision), which stalled at 2 after the first revoke, so a client kept write tools it had lost and never saw new ones."
```

---

### Task 5: The write path — confirmations before approval, canonical idempotency, abandoned operations, approval re-checked

Fixes M3, M4, M5, L8, L9, L10, L11, L12, L13.

**Files:**
- Modify: `crates/dexo-mcp/src/tools_write.rs` (`write` ~36-70, `await_approval` ~110-150, `call_write_tool` ~455-520, `apply_ddl` ~775-795, `admin_action` ~845-852)
- Modify: `crates/dexo-app/src/mcp/operation.rs:34`, `crates/dexo-app/src/mcp/ledger.rs` (trait + memory impl), `crates/dexo-app/src/mcp/grant.rs` (`kinds_for`)
- Modify: `crates/dexo-storage/src/mcp/{mod.rs, grant_repo.rs, approval_repo.rs, operation_repo.rs}`
- Modify: `crates/dexo-cli/src/run.rs` (`kinds_words`), `crates/dexo-tui/src/runtime/mod.rs` (`access_on`)

**Interfaces:**
- Produces: `GrantLedger::abandon_running_operations(&self)`, `Grant::kinds_for(&self, profile: &McpProfile) -> Vec<WriteKind>`, `pub(crate) fn ddl_confirmed(arguments: &Map<String, Value>) -> Result<(), AppError>`, `pub(crate) fn session_confirmed(arguments: &Map<String, Value>) -> Result<(), AppError>`.

- [ ] **Step 1: Write the failing test** in the `mod tests` of `tools_write.rs` (it has `call`, `grant`, `APPROVED` helpers):

```rust
    /// A destructive DDL without its confirm_target is told to type the target, before
    /// anyone is asked to approve it: told it waits for approval, the agent retried for ever.
    #[tokio::test]
    async fn an_unconfirmed_destructive_ddl_is_told_what_to_type_first() {
        let ledger = MemoryGrantLedger::default();
        ledger.insert_grant(grant(WriteKind::Schema)).unwrap();
        let connection = connection("local");
        let session = FakeSession::default();
        let error = call_write_tool(
            &service(),
            &ledger,
            Some((&connection, &session as &dyn dexo_driver_api::Session)),
            "s",
            "schema_apply_ddl",
            json!({"operation_id":"op-d","target":"db.public.items","sql":"DROP TABLE items"})
                .as_object()
                .cloned()
                .unwrap(),
            0,
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("type db.public.items as confirm_target"), "{error}");
    }

    /// A retry whose JSON keys arrive in another order is the same write.
    #[test]
    fn the_payload_hash_ignores_key_order() {
        use dexo_app::mcp::operation::payload_hash;
        assert_eq!(
            payload_hash(&json!({"values":{"id":7,"name":"x"},"target":"t"})),
            payload_hash(&json!({"target":"t","values":{"name":"x","id":7}}))
        );
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p dexo-mcp an_unconfirmed_destructive_ddl the_payload_hash_ignores_key_order`
Expected: both FAIL ("waits for a person's approval"; different hashes).

- [ ] **Step 3: Confirmations as functions, checked before approval**

In `tools_write.rs` add near `write_targets`:

```rust
/// A destructive DDL needs `confirm_target` equal to `target`, typed by the client.
pub(crate) fn ddl_confirmed(arguments: &Map<String, Value>) -> Result<(), AppError> {
    let target = arguments.get("target").and_then(Value::as_str).unwrap_or_default();
    let sql = arguments.get("sql").and_then(Value::as_str).unwrap_or_default();
    let risk = classify_raw_sql(sql);
    if (risk.destructive || risk.data_loss)
        && arguments.get("confirm_target").and_then(Value::as_str) != Some(target)
    {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {target} as confirm_target to confirm"),
        ));
    }
    Ok(())
}

/// Ending a session needs `confirm_target` equal to `session_id`.
pub(crate) fn session_confirmed(arguments: &Map<String, Value>) -> Result<(), AppError> {
    let session = arguments.get("session_id").and_then(Value::as_str).unwrap_or_default();
    if arguments.get("confirm_target").and_then(Value::as_str) != Some(session) {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {session} as confirm_target to confirm"),
        ));
    }
    Ok(())
}

/// What a write must carry before anyone is asked about it.
fn confirmed(name: &str, arguments: &Map<String, Value>) -> Result<(), AppError> {
    match name {
        "schema_apply_ddl" => ddl_confirmed(arguments),
        "admin_terminate_session" => session_confirmed(arguments),
        _ => Ok(()),
    }
}
```

In `await_approval`, replace the `if name == "schema_apply_ddl" { … return Ok(None); }` block with:

```rust
        // A write refused where it runs -- an unconfirmed destructive DDL, a connection
        // that no longer takes writes -- is not put to a person first: they approved a
        // write that then failed.
        if confirmed(name, arguments).is_err() || connection.accepts_writes().is_err() {
            return Ok(None);
        }
```

In `call_write_tool`, right after `connection.accepts_writes()?;` add `confirmed(name, &arguments)?;`. In `apply_ddl` and the terminate branch of the admin action, replace the inline checks with `ddl_confirmed(value.as_object().unwrap_or(&Map::new()))?;` and `session_confirmed(…)?;` respectively (read both to fit the local variable names; `value` there is `&Value`).

- [ ] **Step 4: Canonical payload hash** — in `operation.rs`:

```rust
/// The payload with its object keys sorted at every depth: a retry whose client sent the
/// same fields in another order is the same write.
fn canonical(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical).collect())
        }
        other => other.clone(),
    }
}

pub fn payload_hash(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(&canonical(value)).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}
```

- [ ] **Step 5: Abandoned operations, approval re-checked, cancel-after-approve audited**

Trait `GrantLedger` (ledger.rs): add after `finish_operation`:

```rust
    /// Operations a server left `running` when it died: marked unknown, so a retry is
    /// refused instead of replayed with no result.
    fn abandon_running_operations(&self);
```

Memory impl: iterate `inner.operations.values_mut()`, set `state = OperationState::Unknown` where `state == OperationState::Running`. SQLite impl (`storage/mcp/mod.rs`) calls a new `operation_repo::abandon_running(conn)`:

```rust
pub fn abandon_running(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_operations SET state = 'unknown' WHERE state = 'running'",
        [],
    )?;
    Ok(())
}
```

(check `state_name(OperationState::Running)` spells `running` and `Unknown` `unknown`; use those strings). In `DexoMcpServer::new`, after `ledger.sweep_approvals(now_secs());` add `ledger.abandon_running_operations();`.

In `call_write_tool`, after the `covering_grant` lookup and before `if approved.is_none()`:

```rust
    // An approval stands only for the grant it was given under, and only while it is
    // still approved: a revoke between the answer and the run denies it.
    if let Some(id) = approved
        && !ledger
            .approval(id)
            .is_some_and(|approval| approval.decision == ApprovalDecision::Approved && approval.grant == grant.id)
    {
        audit(ledger, service, name, Some(operation_id), target_name, "deny", Some(&grant.id.to_string()), "the approval no longer stands", now, None);
        return Err(AppError::new(ErrorCategory::McpPolicy, "the approval for this write no longer stands"));
    }
```

In `write()`, the `if cancel.is_cancelled()` branch: when `approved.is_some()`, write an audit line first:

```rust
        if cancel.is_cancelled() {
            if approved.is_some() {
                let target = arguments.get("target").and_then(Value::as_str).unwrap_or_default();
                audit(
                    self.inner.ledger.as_ref(),
                    &self.inner.service,
                    name,
                    arguments.get("operation_id").and_then(Value::as_str),
                    target,
                    "deny",
                    None,
                    "cancelled by the agent after approval",
                    now_secs(),
                    None,
                );
            }
            return crate::error::app_error(&cancelled_by_agent());
        }
```

- [ ] **Step 6: Storage hygiene** — `approval_repo::sweep`: wrap the body in `let tx = conn.unchecked_transaction()?; … tx.commit()?;` using `tx` in place of `conn` for every statement. `grant_repo::revoke`:

```rust
pub fn revoke(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    let changed = conn.execute(
        "UPDATE mcp_grants SET revoked = 1, revision = revision + 1 WHERE id = ?1 AND revoked = 0",
        params![id.to_string()],
    )?;
    anyhow::ensure!(changed == 1, "no live grant has the id {id}");
    Ok(())
}
```

Audit of grant changes: in `SqliteGrantLedger::insert_grant`, `revoke`, `revoke_connection`, `revoke_profile`, `revoke_all` (inside the `revoking` closure, same transaction) and in the memory ledger's matching methods, record:

```rust
fn grant_event(profile: &str, connection: &str, what: &str, grant_id: Option<&str>) -> AuditEvent {
    AuditEvent {
        timestamp: now_secs_here(), // the same Unix-seconds helper the crate uses
        request: format!("grant {what}"),
        operation_id: None,
        profile: profile.into(),
        client: "dexo".into(),
        target: connection.into(),
        decision: "allow".into(),
        grant_id: grant_id.map(str::to_string),
        duration_ms: 0,
        rows: 0,
        bytes: 0,
        status: what.into(),
        sql: None,
    }
}
```

with `what` = `"create"`, `"revoke"`; for `revoke_profile`/`revoke_all` one event per revoked row (select `connection_name, id` of the live rows first). Use `super::audit_repo::insert(&tx, &event)` in storage and `inner.audits.push(event)` in memory.

- [ ] **Step 7: Kinds against the profile** — in `grant.rs`:

```rust
    /// The kinds its tools are of, less the ones the profile denies every tool of now:
    /// what the profile can actually do with it.
    pub fn kinds_for(&self, profile: &McpProfile) -> Vec<WriteKind> {
        WriteKind::ALL
            .into_iter()
            .filter(|kind| {
                kind.tools()
                    .iter()
                    .any(|tool| self.tools.iter().any(|t| t == tool) && profile.tool_allowed(tool))
            })
            .collect()
    }
```

Use it in `run.rs` `kinds_words` (give it the `&McpProfile` the caller loaded) and in `runtime/mod.rs` `access_on` (pass `&profile`).

- [ ] **Step 8: Build, test, commit**

Run: `cargo test -p dexo-app -p dexo-storage -p dexo-mcp -p dexo-cli -p dexo-tui`
Expected: pass, the two new tests included.

```bash
git add crates/dexo-app crates/dexo-storage crates/dexo-mcp crates/dexo-cli crates/dexo-tui
git commit -m "fix(mcp): a write is confirmed before it is put to a person, retried the same in any key order, and refused once its approval or server is gone

Grant changes are audited, a revoke of an unknown id is an error, the approval sweep is one transaction, and a grant's kinds are read against the profile's rules."
```

---

### Task 6: Read tools — navigable catalog, right names, right error categories

Fixes H3, M9, M6, L7, L18 (tool descriptions), L22.

**Files:**
- Modify: `crates/dexo-mcp/src/tools_read.rs` (`objects_result` ~61, `describe` heading ~207, `relationships` ~252, `catalog_search` description ~429, `list_connections` text ~315, `schema_diff` ~593), `crates/dexo-mcp/src/schema.rs:106` (and `DiffInput`), `crates/dexo-mcp/src/prompts.rs:30`, `crates/dexo-mcp/src/server.rs:30` (`TOOL_SCHEMA_VERSION`), `crates/dexo-mcp/src/backend.rs` (`schema_snapshot` signature) and `crates/dexo-cli/src/mcp_backend.rs`
- Modify: `crates/dexo-app/src/query_service.rs:88-96`, `crates/dexo-app/src/mcp/service.rs:391` (`guard_error`), `:337` (stale comment)
- Rename: `crates/dexo-mcp/tests/snapshots/protocol__tools_v3.snap` → `protocol__tools_v4.snap`

- [ ] **Step 1: Ids and names**

`objects_result`:

```rust
fn objects_result(objects: &[CatalogObject]) -> CallToolResult {
    let rows = objects
        .iter()
        .map(|object| {
            vec![
                object.id.to_string(),
                object.kind.as_str().to_string(),
                dexo_app::mcp::ObjectRef::from_catalog_object(object).to_string(),
            ]
        })
        .collect();
    rows_result(&RowsPage::new(
        ["id", "kind", "name"].map(String::from).to_vec(),
        rows,
    ))
}
```

(`object.id` is the driver's `ObjectId`; use whatever `Display`/`as_str` it has — `grep -n "pub struct ObjectId" -A 6 crates/dexo-driver-api/src/catalog.rs`.) In `describe`'s heading use `dexo_app::mcp::ObjectRef::from_catalog_object(&object)` and `object.kind.as_str()`; in `relationships` the same for `related` and `related.kind.as_str()`. `schema.rs:106`: `/// `id` of the node whose children to list, from an earlier catalog_list; omit for the roots.` `prompts.rs:30` stays true once ids exist.

- [ ] **Step 2: Descriptions** — `tools_read.rs` `list_connections` column text "with a grant" → "while the profile has write access there"; `catalog_search` doc comment: "Tables, views, columns, indexes and constraints by name, or by what their notes say, in the connection's indexed catalog." Delete the stale comment at `service.rs:337` ("Tasks 8–12 add their names here").

- [ ] **Step 3: Snapshots scoped to the profile** — add `connection: Option<String>` to `DiffInput` in `schema.rs` (doc: `/// Connection the snapshots were taken from; optional when the profile has exactly one.`); in `schema_diff` resolve it like every other tool (`self.inner.router.resolve(input.connection.as_deref())`) and pass the connection's name to `backend.schema_snapshot(name, &connection.name)`; change the `McpBackend::schema_snapshot` signature to `fn schema_snapshot(&self, name: &str, connection: &str)` and in `mcp_backend.rs` filter `SchemaSnapshotStore::load_by_name(name)` by the snapshot's connection (read `SchemaSnapshot`'s fields with `grep -n "pub struct SchemaSnapshot" -A 12 crates/dexo-app/src/schema_diff/*.rs`; return `Ok(None)` when it belongs to another connection).

- [ ] **Step 4: Error categories** — `query_service.rs`:

```rust
pub fn map_driver_error(error: DriverError) -> AppError {
    let category = match error.category() {
        DriverErrorCategory::Cancelled => ErrorCategory::Cancelled,
        DriverErrorCategory::Timeout => ErrorCategory::Timeout,
        DriverErrorCategory::Permission => ErrorCategory::Permission,
        DriverErrorCategory::Network | DriverErrorCategory::Transport => ErrorCategory::Network,
        DriverErrorCategory::Syntax => ErrorCategory::Syntax,
        DriverErrorCategory::Authentication => ErrorCategory::Authentication,
        DriverErrorCategory::Configuration => ErrorCategory::Configuration,
        DriverErrorCategory::Capability => ErrorCategory::Capability,
        DriverErrorCategory::Conflict => ErrorCategory::Conflict,
        DriverErrorCategory::Internal => ErrorCategory::Internal,
    };
    AppError::new(category, error.to_string())
}
```

`service.rs` `guard_error`:

```rust
fn guard_error(rejection: GuardRejection) -> AppError {
    let category = match rejection {
        GuardRejection::Unparsed(_) | GuardRejection::NotSingleStatement => ErrorCategory::Syntax,
        _ => ErrorCategory::McpPolicy,
    };
    AppError::new(category, rejection.to_string())
}
```

Check `crates/dexo-mcp/src/error.rs` maps `Authentication` to `CONNECTION_FAILED` (the docs' word) and `Capability` to `UNSUPPORTED`; add the arms if missing.

- [ ] **Step 5: Bump the tool schema** — `TOOL_SCHEMA_VERSION = 4`; `git mv crates/dexo-mcp/tests/snapshots/protocol__tools_v3.snap crates/dexo-mcp/tests/snapshots/protocol__tools_v4.snap`; run `cargo test -p dexo-mcp`, inspect the `.snap.new` diff (ids column, `connection` on schema_diff, the reworded descriptions, nothing else), accept it by moving it over the `.snap` without the `assertion_line:` header. `grep -rn "schema v3\|tool schema v3" docs README.md crates/dexo-app/src/mcp/clients.rs` and update any mention.

- [ ] **Step 6: Test, commit**

Run: `cargo test -p dexo-mcp -p dexo-app -p dexo-cli`

```bash
git add crates/dexo-mcp crates/dexo-app crates/dexo-cli
git commit -m "fix(mcp): catalog_list carries the ids it says to follow, names are the paths objects occupy, errors keep their category

A schema row read db.public.public; schema_diff takes the connection its snapshots are of; a parse error is INVALID_INPUT, an authentication failure CONNECTION_FAILED. Tool schema v4."
```

---

### Task 7: Reads pin `search_path`

Fixes M2.

**Files:**
- Modify: `crates/dexo-app/src/mcp/service.rs` (`execute_read` ~181-204)

- [ ] **Step 1: Pin the schema inside the read transaction**

After `transactions.begin(TransactionMode::ReadOnly).await.map_err(map_driver_error)?;` add:

```rust
        // The policy judged bare names as `public.*`; the server resolves them by the
        // role's search_path. Pinned, both mean the same table.
        // ponytail: reads only; writes and EXPLAIN run outside this transaction.
        if connection.dialect == Dialect::Postgres {
            let mut set = session
                .execute(QueryRequest::write("SET LOCAL search_path = public"))
                .await
                .map_err(map_driver_error)?;
            while let Some(event) = set.next().await {
                event.map_err(map_driver_error)?;
            }
        }
```

(`Dialect` is `dexo_sql::Dialect`; `QueryRequest` is already imported for `collect_rows`.) If the `begin` fails after this point the existing `rollback` path covers it.

- [ ] **Step 2: Build and run the live suite if a Postgres is reachable**

Run: `cargo build -p dexo-app && cargo test -p dexo-app`. With `docker ps` showing a Postgres from `dexo-test-support` (see `crates/dexo/tests/mcp_write.rs` for how `DatabasePair` starts one), run `cargo test -p dexo --test mcp_write`; else note in the commit that the live suite was not run.

- [ ] **Step 3: Commit**

```bash
git add crates/dexo-app/src/mcp/service.rs
git commit -m "fix(mcp): a read pins search_path to public, the schema the allowlist judged its bare names by"
```

---

### Task 8: CLI — the truth about writes, and the rest of the CLI findings

Fixes H4, M10, M11, M15, M16, L14, L15, L16, L17, L18 (CLI parts), L19, L20, L21.

**Files:**
- Modify: `crates/dexo-cli/src/run.rs` (`span_words` ~1654, `mcp_policy` ~1663, `mcp_doctor` ~1715, `Enable` ~1506, `Set` ~1537, `mcp_setup` ~1824, `mcp_config_print` ~2042, `run_mcp_grant` Revoke/RevokeAll ~2150, `mcp_audit` ~2180, `mcp_profile_names`), `crates/dexo-cli/src/args.rs` (McpCommand, McpProfileCommand::Set, McpConfigCommand), `crates/dexo-cli/src/mcp_backend.rs:48`, `crates/dexo-cli/tests/fixtures/help.txt`
- Modify: `crates/dexo-app/src/mcp/grant.rs` (`GrantRequest::issue` ~285), `crates/dexo-app/src/mcp/profile.rs` (`validate_limits` ~118, `:90`), `crates/dexo-app/src/mcp/clients.rs` (`write_with_backup` ~565, `SetUp`, `by_hand` ~162)
- Modify: `crates/dexo-tui/src/screens/mcp_profiles.rs` (`duration_words` ~15), `crates/dexo/src/main.rs`, `crates/dexo/Cargo.toml`

- [ ] **Step 1: Durations** — `run.rs` `span_words` and `mcp_profiles.rs` `duration_words` both become:

```rust
    match secs {
        i64::MIN..=0 => "now".into(),
        1..=89 => format!("{secs} s"),
        90..=5399 => format!("{} min", (secs + 30) / 60),
        _ => {
            let minutes = (secs + 30) / 60;
            match (minutes / 60, minutes % 60) {
                (hours, 0) => format!("{hours} h"),
                (hours, rest) => format!("{hours} h {rest} min"),
            }
        }
    }
```

Run `cargo test -p dexo-cli -p dexo-tui` and fix any expectation that asserted `N h` for a value with minutes.

- [ ] **Step 2: `policy`, `show`, `doctor` say what may be written** — in `run.rs` add a helper and use it:

```rust
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `pg-dev  rows, schema  ends in 2 h, each write waiting up to 2 min`
fn grant_line(grant: &dexo_app::mcp::Grant, profile: &McpProfile, now: i64) -> String {
    format!(
        "{}  {}  {}, each write waiting up to {}",
        grant.connection,
        kinds_words(grant, profile),
        grant_lasts(grant, now),
        span_words(i64::from(grant.ask_secs))
    )
}
```

In `mcp_policy`, after loading the profile: `let ledger = SqliteGrantLedger::open(&paths.database)?; let now = now_secs(); let grants = ledger.active_grants(&profile.name, now);` and print:

```rust
    let access = if grants.is_empty() {
        "read-only access".to_string()
    } else {
        format!(
            "writes on {}",
            grants.iter().map(|g| g.connection.as_str()).collect::<Vec<_>>().join(", ")
        )
    };
    println!("Profile {}: {}, {access}", profile.name, if profile.enabled { "enabled" } else { "disabled" });
    …
    for grant in &grants {
        println!("writes: {}", grant_line(grant, &profile, now));
    }
    let mut tools: Vec<String> = advertised_tools(&profile).into_iter().map(str::to_string).collect();
    for grant in &grants {
        for tool in &grant.tools {
            if profile.tool_allowed(tool) && !tools.contains(tool) {
                tools.push(tool.clone());
            }
        }
        if grant.lists_sessions() && !tools.iter().any(|t| t == "admin_list_sessions") {
            tools.push("admin_list_sessions".into());
        }
    }
    println!("tools: {}", tools.join(", "));
```

`mcp_doctor`: open the ledger once; per profile `let grants = ledger.active_grants(&p.name, now);` JSON `"access": if grants.is_empty() { "read_only" } else { "writes" }`, `"grants": grants.iter().map(|g| json!({"connection": g.connection, "allow": g.kinds_for(p).iter().map(|k| k.as_str()).collect::<Vec<_>>(), "expires_at": g.expires_at, "approval_timeout_secs": g.ask_secs})).collect::<Vec<_>>()`, and `"tools"` built as above; the text line prints `  writes: {grant_line}` after each profile summary. `Enable` prints `objects:` instead of `scopes:`. `grant list` reuses `grant_line`.

- [ ] **Step 3: `profile set` revokes what it drops; `grant create` needs a connection** — in the `Set` arm, before mutating, `let before = profile.connections.clone();` and after `repo.save(&profile)?`:

```rust
            let ledger = SqliteGrantLedger::open(&paths.database)?;
            for connection in before.iter().filter(|c| !profile.connections.contains(c)) {
                if ledger.revoke_connection(&name, connection)? > 0 {
                    println!("{name} may no longer write on {connection}: it no longer uses it");
                }
            }
```

In `grant.rs` `GrantRequest::issue`, replace the `if !profile.connections.is_empty() && !…` block with:

```rust
        if profile.connections.is_empty() {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                format!(
                    "the profile {} uses no connection yet: dexo mcp profile set --name {} --connection {}",
                    profile.name, profile.name, self.connection
                ),
            ));
        }
        if !profile.connections.iter().any(|name| name == &self.connection) {
            … (the existing "does not use the connection" error)
        }
```

- [ ] **Step 4: `setup` says what it did; `revoke-all`/`revoke`/`audit` check their names; the hint; `config print`; quoting**

`clients.rs`: `write_with_backup` returns `Result<Written, AppError>` with

```rust
/// What writing a client's file came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Written {
    /// The file already said this.
    Unchanged,
    /// A new file.
    Created,
    /// Replaced, the old one copied here first.
    Replaced(PathBuf),
}
```

(`Ok(None)` → `Ok(Written::Unchanged)`; the no-existing path → `Created`; the backup path → `Replaced(backup)`). `SetUp.backup: Option<PathBuf>` becomes `pub written: Written`; its skill file write gets its own `Written` or stays as is if it uses another helper. `run.rs` `mcp_setup` prints `already set up: {}` / `wrote {}` / `wrote {} (the old one is {})`; `crates/dexo-tui/src/runtime/mod.rs` `write_agent_config` does the same in its lines. `by_hand` and `mcp_config_print` shell-quote the executable: add in `clients.rs`

```rust
/// `path` as a shell word: quoted when it holds a space or a quote.
pub fn shell_word(path: &str) -> String {
    if path.chars().any(|ch| ch.is_whitespace() || ch == '\'' || ch == '"') {
        format!("'{}'", path.replace('\'', r"'\''"))
    } else {
        path.to_string()
    }
}
```

and use `shell_word(command)` in `by_hand`'s `serve` and in `mcp_config_print`'s `claude mcp add` line. `mcp_config_print`: `--client` gets `value_parser = ["claude-code", "codex", "cursor", "claude-desktop", "gemini-cli", "windsurf", "vscode"]` in `args.rs`; for `claude-code` print the `claude mcp add` line, else `client.merged(None, &exe, &args)` (the file content that client takes — TOML for Codex, `servers` for VS Code). `revoke-all` and `audit` call `load_profile` first; `revoke` checks `ConnectionRepository::new(db.connection()).get_by_name(&connection)?.is_some()` else `bail!("unknown connection '{connection}'")`. `mcp_backend.rs:48`: `dexo connections test --name {connection}`.

- [ ] **Step 5: Limits, vocabulary, help, broken pipe**

`profile.rs` `validate_limits`: one `ensure`-style check per field with its range in the message, e.g. `"max_rows must be 1 to 1000000"`, `"max_bytes must be 1 to 33554432"`, `"timeout_secs must be 1 to 3600"`, `"max_concurrency must be 1 to 32"`; `:90` → `"a tool rule names one tool; * and all are refused"`. `args.rs`: on `McpGrantCommand::Create::allow` add `value_parser = |s: &str| if s.trim().is_empty() { Err("name what it may change: rows, schema or sessions") } else { Ok(s.trim().to_string()) }`-style validation (clap `value_parser!` with a function `fn kind_word(s: &str) -> Result<String, String>`), and `///` docs on every option of `Allow`, `Doctor`, `Profile set`, `Config print` (one line each saying what it takes). Then `cargo test -p dexo-cli --test help`; it fails with the new text — copy the `got` output into `crates/dexo-cli/tests/fixtures/help.txt` (the test replaces the version with `VERSION`; keep that). Broken pipe: `crates/dexo/Cargo.toml` gets `libc = "0.2"` under `[dependencies]` (add `libc = "0.2"` to the workspace `[workspace.dependencies]` if the repo keeps versions there, and `libc.workspace = true` in the crate); in `main.rs` first line of `main`:

```rust
    // `dexo … | head` ended in a panic on the closed pipe; the default disposition ends
    // the process quietly, as every other CLI does.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
```

- [ ] **Step 6: Test, commit**

Run: `cargo test -p dexo-cli -p dexo-app -p dexo-tui`; then with a scratch data home: `grant create --allow rows`, `mcp policy --profile assistant` shows `writes on pg-dev` and a `writes:` line; `profile set --name assistant --clear-connections --connection my-dev` prints `may no longer write on pg-dev`; `setup --client claude-code --profile assistant` twice prints `already set up` the second time; `mcp profile show --name assistant | head -1` ends quietly.

```bash
git add crates/dexo-cli crates/dexo-app crates/dexo-tui crates/dexo
git commit -m "fix(cli): policy, show and doctor say what a profile may write, and the mcp commands say the truth and check their names

A dropped connection takes its writes with it; grant create needs a connection the profile uses; setup says when nothing changed; durations say 1 h 30 min; paths with spaces are quoted; limits name their range; dexo | head ends quietly."
```

---

### Task 9: Core library — selectors, profile repository, skill text, client state

Fixes L3, L4, L5, L6.

**Files:**
- Modify: `crates/dexo-cli/src/run.rs` (`mcp_allow`), `crates/dexo-storage/src/mcp_profile.rs` (`save`, `delete`), `crates/dexo-storage/src/mcp/mod.rs` (`pub(crate) mod grant_repo`), `crates/dexo-app/src/mcp/clients.rs` (`skill_text` ~624, `set_up` ~188, `configured` ~311), callers of `skill_text`/`set_up` in `run.rs` and `crates/dexo-tui/src/runtime/mod.rs` (`write_agent_config`, `connect_mcp_agent`, `save_mcp_profile`)

- [ ] **Step 1: Selectors with uppercase get a note** — in `mcp_allow` (run.rs), after the rule is parsed and before saving:

```rust
    if selector.chars().any(|ch| ch.is_ascii_uppercase()) {
        println!(
            "note: a selector is matched exactly as written; Postgres stores an unquoted name in lowercase, so `{selector}` matches only a name created with quotes"
        );
    }
```

- [ ] **Step 2: Profile repository in one transaction, delete takes the grants** — `mcp_profile.rs` `save`: `let tx = self.conn.unchecked_transaction()?;` at the top, every `self.conn.execute` → `tx.execute`, `tx.commit()?;` before `Ok(())`. `delete`: same, plus before deleting the profile row `crate::mcp::grant_repo::revoke_profile(&tx, name)?;` (make `mod grant_repo` `pub(crate)` in `crates/dexo-storage/src/mcp/mod.rs`) and `tx.execute("UPDATE mcp_approvals SET decision = 'denied', statement = '' WHERE profile_name = ?1 AND decision = 'pending'", params![name])?;`.

- [ ] **Step 3: Skill text from the profile** — `skill_text(client: McpClient, profile: &McpProfile) -> String`: the `## Reading` list is built from `crate::mcp::advertised_tools(profile)`: keep the sentences for the tools present (`catalog_search`/`catalog_list`/`object_describe`/`object_get_ddl`/`object_relationships`/`data_read` always; the `query_*` sentence only when `query_execute_read` is advertised; add `- `schema_diff` compares two schema snapshots the person saved.` and, when advertised, `- `admin_list_sessions` lists the server's sessions.`). `set_up(self, places, command, profile: &McpProfile, skill)` takes the profile and passes it on; `name` inside is `&profile.name`. Update the callers: `run.rs` `mcp_setup` (has `profile`), `runtime/mod.rs` `write_agent_config(client, profile: &McpProfile, skill)` — `connect_mcp_agent` and `save_mcp_profile` both hold the loaded/made `McpProfile`, pass it. The test `the_skill_names_no_fixed_wait` builds a profile: `McpProfile::new("agent")`.

- [ ] **Step 4: A non-object root is unparsable** — in `configured`, after `let value = serde_json::from_str…?;`: `if !value.is_object() { return Err(unparsed("its root is not an object".into())); }`.

- [ ] **Step 5: Test, commit**

Run: `cargo test -p dexo-app -p dexo-storage -p dexo-cli -p dexo-tui`

```bash
git add crates/dexo-app crates/dexo-storage crates/dexo-cli crates/dexo-tui
git commit -m "fix(mcp): a deleted profile takes its grants and waiting writes, a profile saves in one transaction, the skill file names the profile's own tools

An uppercase selector gets a note, and a config whose root is not an object is left alone from the start."
```

---

### Task 10: TUI — one section per view, no double submit, fresh numbers, honest Edit

Fixes H5, M12, M13, M14, L23, L24, L25, L26, L27, L28, L18 (TUI text), plus L11's `access_on` wiring from Task 5.

**Files:**
- Modify: `crates/dexo-tui/src/update.rs` (view switch ~5697-5709, `mouse_agents` ~4665, `mouse_mcp_profiles` ~4395-4412, `OpenMcpWritesForm` ~2070, `McpFormDone` ~2085, `ScreenTick` ~2715, `run_when_loaded` ~4522, `submit_profiles_form`, `open_*` forms, `WhenLoaded::RevokeAll` arm)
- Modify: `crates/dexo-tui/src/screens/mcp_profiles.rs` (`Access` ~40, `McpProfilesScreen::current`, `ask_revoke_all` ~402, `WritesForm::new` text ~1190, `load_current` ~1221, Sees key handling ~945-970)
- Modify: `crates/dexo-tui/src/screens/mcp_clients.rs` (`McpClients.loaded`)
- Modify: `crates/dexo-tui/src/screen/agents.rs` (`who_may_ask` ~103, sheet access text, profiles list state)
- Modify: `crates/dexo-tui/src/runtime/mod.rs` (`access_on` ~2946, `mcp_profiles_loaded` ~2975, `mcp_clients` ~3048, `save_mcp_profile` ~3121, `audit_line` ~2885, `revoke_mcp`/`revoke_all_mcp` Err arms), `crates/dexo-storage/src/mcp/grant_repo.rs` `revoke_all`
- Modify tests: `crates/dexo-tui/tests/agents_profiles.rs`, `admin_settings_mcp_flow.rs` (`Access` fields)

- [ ] **Step 1: One section per view, and no switch under a form** — in `update.rs` where `view` is applied (keys) and in `mouse_agents` (mouse):

```rust
        // A view's keys start on its list: the detail walked on Profiles is not the
        // detail read on Approvals.
        model.sections[crate::model::Screen::Agents.index()] = crate::screen::Section::List;
        model.screen_button = 0;
        model.agents_view = view;
```

and in `mouse_agents`, before switching: `if crate::screen::held(model).is_some() { return Vec::new(); }`.

- [ ] **Step 2: Busy forms ignore clicks** — in `mouse_mcp_profiles`, right after `if let Some(form) = model.mcp_profiles.form.as_mut() {`: `if *form.busy() && !matches!(hit, Some(HitTarget::FooterCancel)) { return Vec::new(); }`.

- [ ] **Step 3: Access keeps when it expires, drawn against now** — `mcp_profiles.rs`:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Access {
    pub kinds: Vec<WriteKind>,
    /// Unix seconds; `UNTIL_REVOKED` for one that does not expire.
    pub expires_at: i64,
}

impl Access {
    pub fn until_revoked(&self) -> bool {
        self.expires_at == dexo_app::mcp::grant::UNTIL_REVOKED
    }

    /// How long is left at `now`: `28 min left`, `until revoked`.
    pub fn left(&self, now: i64) -> String {
        if self.until_revoked() {
            "until revoked".into()
        } else {
            format!("{} left", duration_words(self.expires_at.saturating_sub(now)))
        }
    }

    /// `rows, schema · 28 min left`.
    pub fn words(&self, now: i64) -> String {
        format!("{} · {}", kinds_words(&self.kinds), self.left(now))
    }
}
```

`runtime/mod.rs` `access_on(grants, connection, profile)` fills `kinds: last.kinds_for(profile)` (union over `there` as now) and `expires_at: last.expires_at`; drop `now` from it. Every `.left()`/`.words()`/`.until_revoked` use in `agents.rs` and `mcp_profiles.rs` passes `model.mcp_audit.now` (in `WritesForm` text drawn in `agents.rs`, the same). Tests: `Access { kinds, expires_at: 1700, .. }` with `model.mcp_audit.now` left at 0 still reads `28 min left`. `ScreenTick`: reload profiles on Agents whenever `profiles.form.is_none() && profiles.confirm.is_none()`, whatever the view (drop the `agents_view == Profiles` condition).

- [ ] **Step 4: Edit revokes what it drops, for `any` profiles too; connections with grants are shown** — `runtime/mod.rs` `save_mcp_profile`: open the ledger with `?` (a failure is the error), `let now = unix_seconds();` and

```rust
    let mut had: Vec<String> = ledger
        .active_grants(&save.name, now)
        .into_iter()
        .map(|grant| grant.connection)
        .collect();
    had.sort();
    had.dedup();
    for connection in had.iter().filter(|c| !save.connections.contains(c)) {
        if ledger.revoke_connection(&save.name, connection).map_err(text)? > 0 {
            lines.push(format!("✓ it may no longer write on {connection}"));
        }
    }
```

in place of the `dropped` block (do it after `repo.save`). In `mcp_profiles_loaded`: `names` is `profile.connections.clone()` for every profile (no more "every served connection" for `any`), then every connection of `grants` not in `names` is pushed as a `ConnectionLine { name, refused: Some("not one of its connections any more: r takes the writes back".into()), access: access_on(...), ..served line or Default }`. `WritesForm::new` on a profile with no connections already errs; make its text `"{} uses no connection yet: Enter edits it and picks them."`.

- [ ] **Step 5: Small TUI fixes**

- `load_current` (`mcp_profiles.rs`): the `else` branch sets `self.lasts = 1;` too; the `if` branch sets `self.lasts = if access.until_revoked() { LASTS.len() - 1 } else { 1 };`.
- `submit_profiles_form` (`update.rs`), `Form::Profile` Ok arm, before returning the effect: `if !form.editing { model.mcp_profiles.pick = Some(save.name.clone()); }` (restructure so `model.mcp_profiles` is borrowed after `form`); drop the `pick` assignment from `McpFormDone`.
- `OpenMcpWritesForm`: condition `if !model.mcp_profiles.loaded` instead of `model.screen != Agents`; `run_when_loaded`: `waiting_agents = when_loaded == Some(Agent) && !model.mcp_clients.loaded`; `McpClients` gets `pub loaded: bool`, set `true` in the `McpClientsLoaded` handler; `mcp_clients()` on `Places::discover()` failure returns every `McpClient::ALL` as `ClientRow { client, path: client.id().into(), skill: None, state: ClientState::Unusable(error.to_string()), found: false }` with `command: "dexo".into()`.
- `open_writes_form`, `open_agent_form`, `open_edit_profile`: `model.mcp_profiles.confirm = None;` first; the `WhenLoaded::RevokeAll` arm: `model.mcp_profiles.form = None;` before `ask_revoke_all()`.
- `grant_repo::revoke_all` returns distinct pairs: run `SELECT COUNT(*) FROM (SELECT DISTINCT profile_name, connection_name FROM mcp_grants WHERE revoked = 0)` first and return that; `revoke_mcp`/`revoke_all_mcp` `Err` arms chain `mcp_profiles_loaded()`.
- `audit_line`: order the arms `("ask", _)`, `("approved", _)`, `("deny", true) => format!("refused: {words}")`, `("deny", _) => format!("refused: {}", event.status)`, then the rest.
- `who_may_ask` and the list's `✎ writes`: `.filter(|profile| profile.enabled)`.
- Sees: in the `Left`/`Right`/`Space` arms for `ProfileRow::Sees` and in `pick`, when the value changes between `Except` and `Only`, `self.checked.clear();`.
- `McpProfilesScreen::current`: `let rows = self.rows(clients); rows.get(self.row.min(rows.len().saturating_sub(1))).copied()`.
- `WritesForm::new` denied-kinds text: `"{} denies every write tool: undo it with dexo mcp profile set --name {} --allow-tool data_insert"`.

- [ ] **Step 6: Test, render, commit**

Run: `cargo test -p dexo-tui` (update `Access` constructions in tests; `cargo insta`-style snapshots unaffected unless text moved — accept any `.snap.new` whose only change is intended). Then the real binary in tmux with a scratch home: Profiles → Tab → `1` → Down moves the pick, Esc goes back to the list; Edit with a connection unchecked prints `may no longer write` and `dexo mcp grant list` agrees.

```bash
git add crates/dexo-tui crates/dexo-storage
git commit -m "fix(tui): each Agents view starts on its list, a busy form takes no second click, numbers follow the clock, and Edit takes back what it says it does

Writes left on a connection a profile no longer lists are shown and revoked; a dropdown's choice does not carry 'until revoked' to the next connection; the palette's forms wait for both loads."
```

---

### Task 11: Docs

Fixes L2, L18 (docs), M8's doc line, the `BUSY` code, the FK rule, the search_path pin, `dexo doctor`'s archives, and the CLI break note.

**Files:**
- Modify: `docs/src/mcp.md`, `README.md:155-160`, `docs/src/cli.md` (grant/doctor lines), `docs/src/troubleshooting.md`

- [ ] **Step 1: `docs/src/mcp.md`**
  - Selectors: add `A selector matches names exactly as written; Postgres stores an unquoted name in lowercase. A wide allow such as `db.*` also reaches `pg_catalog`, `information_schema` and `pg_largeobject`; give the agent's role only what it may read when that matters.`
  - Tools table: `admin_list_sessions` row says `With `--allow-tool admin_list_sessions`, or while the profile may end sessions`; `data_execute_sql` etc. unchanged.
  - Reads: after the parse paragraph add `A Unicode-escaped identifier (`U&"…"`) is refused outright: the parser keeps it raw while the server decodes it. On Postgres the read runs with `search_path` pinned to `public`, the schema a bare name was judged by.`
  - Writes: add `A DDL that adds a foreign key is held to the table it references too.` and `A destructive DDL or a session end without its `confirm_target` is refused before anyone is asked to approve it.`
  - Errors: a short list of codes: `NOT_FOUND` (hidden or missing), `INVALID_INPUT` (arguments, SQL that does not parse, more than one statement), `POLICY_DENIED` (the allowlist, the denylist, a refused approval), `CONNECTION_FAILED` (the database or its password), `UNSUPPORTED`, `BUSY` (more calls at once than `max_concurrency`), `INTERNAL`.
  - Set up: add `dexo doctor` names old copies an earlier Dexo moved aside (`dexo.db.unsupported-v*`).
  - A `## Upgrading to 1.5` note: the grant commands changed (`--allow`, `revoke --connection`), write access given before 1.5 is revoked by migration 20 and must be given again, and an older `dexo` refuses a database a newer one wrote instead of moving it aside.

- [ ] **Step 2: README and cli.md** — README `:158` comment: `# or codex, cursor, claude-desktop, gemini-cli, windsurf, vscode`; `docs/src/cli.md` grant/doctor lines match Task 8's output. `troubleshooting.md`: one entry `"dexo.db was written by a newer Dexo"` → run the newer `dexo`, or upgrade the one on PATH.

- [ ] **Step 3: Commit**

```bash
git add docs README.md
git commit -m "docs(mcp): error codes, the U& and search_path rules, sessions listed with a sessions access, and what 1.5 changes for grants and old databases"
```

---

## Self-review notes

- Every finding C1–L29 is in a task: C1/M1/L1 → 1; C2/L29 → 2; H1 → 3; H2/M7/M8 → 4; M3/M4/M5/L8/L9/L10/L11/L12/L13 → 5; H3/M9/M6/L7/L22/L18(tools) → 6; M2 → 7; H4/M10/M11/M15/M16/L14–L17/L18(CLI)/L19–L21 → 8; L3–L6 → 9; H5/M12/M13/M14/L23–L28/L18(TUI) → 10; L2 and the docs of every behavior change → 11.
- Names used across tasks: `Inner::visible_tools` (4), `Grant::lists_sessions` (4, 8), `Grant::kinds_for` (5, 8, 10), `GrantLedger::abandon_running_operations` (5), `Written` and `shell_word` (8), `skill_text(client, &McpProfile)` and `set_up(…, &McpProfile, …)` (9, callers in 8's `write_agent_config`), `Access { kinds, expires_at }` with `left(now)`/`words(now)` (10), `dexo_storage::leftover_archives` (2).
- Review Focus 1–5 are pinned by the tests in Tasks 1, 2, 4, 3 and 5.
