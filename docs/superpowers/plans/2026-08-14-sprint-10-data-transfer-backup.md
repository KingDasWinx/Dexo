# Dexo Sprint 10: Data Transfer and Backup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Importar/exportar dados por streaming e integrar backup/restore nativos sem expor credenciais.

**Architecture:** Format codecs operate on bounded row streams; destinations use temporary files and atomic rename. Native tools are optional adapters with version discovery, sanitized process execution and cancellation.

**Tech Stack:** csv 1.4.0, serde_json 1.0.151, encoding_rs 0.8.35, tempfile 3.27, Tokio 1.53 process/io, driver bulk APIs.

---

## File map

- Create: `dexo-app/src/transfer/{codec.rs,detect.rs,map.rs,import.rs,export.rs,rejects.rs,native_tool.rs}`
- Create: `dexo-driver-api/src/transfer.rs` and driver bulk implementations
- Create: `dexo-tui/src/screens/transfer.rs`
- Extend: CLI `import|export`
- Test: codec goldens, memory bounds, native-tool fake process.

### Task 1: Implement loss-aware codecs

- [x] **Step 1:** Add goldens for CSV/TSV/JSON/JSONL/SQL containing NULL, empty string, quotes, newlines, Unicode, decimal, date and bytes.
- [x] **Step 2:** Run; expect codecs absent.
- [x] **Step 3:** Define `RowEncoder`/`RowDecoder` streaming traits and `FormatOptions { null, delimiter, header, encoding, binary }`; SQL encoder delegates literals to driver dialect.
- [x] **Step 4:** Round-trip all lossless formats and explicitly assert documented lossy cases.
- [x] **Step 5:** Commit with `git commit -m "feat(transfer): add streaming row codecs"`.

### Task 2: Export atomically with bounded memory

- [x] **Step 1:** Add test cancelling after 10k of 1m generated rows; destination must remain unchanged and temp file removed.
- [x] **Step 2:** Run; expect exporter absent.
- [x] **Step 3:** Stream query batches to `NamedTempFile` in destination directory, flush/sync per policy, then atomic persist; publish rows/bytes progress; never collect all rows.
- [x] **Step 4:** Run memory benchmark and cancellation test.
- [x] **Step 5:** Commit with `git commit -m "feat(transfer): export atomically with bounded memory"`.

### Task 3: Detect and preview imports

- [x] **Step 1:** Add fixtures for UTF-8/UTF-16, comma/tab/semicolon, header/no-header and ambiguous types; assert confidence plus user-overridable preview.
- [x] **Step 2:** Run; expect detector absent.
- [x] **Step 3:** Inspect bounded prefix only; detect BOM/encoding/delimiter/header; infer nullable bool/int/decimal/date/text without converting source; map source to target columns.
- [x] **Step 4:** Run fixtures and malformed input diagnostics with line/column.
- [x] **Step 5:** Commit with `git commit -m "feat(transfer): detect and preview import files"`.

### Task 4: Import batches with explicit error strategy

- [x] **Step 1:** Add shared driver test with one invalid row under `Stop`, `Skip`, `RejectFile`; assert committed count and reject content.
- [x] **Step 2:** Run; expect import service absent.
- [x] **Step 3:** Implement bounded batches, bound values, transaction/savepoint strategy, cancel, progress and `RejectedRow { line, safe_error, original_fields }`; reject files use atomic writes.
- [x] **Step 4:** Run both databases and all strategies.
- [x] **Step 5:** Commit with `git commit -m "feat(transfer): import batches with reject policies"`.

### Task 5: Wrap native backup and restore safely

- [x] **Step 1: Add fake process test**

```rust
#[tokio::test]
async fn password_never_appears_in_arguments_or_logs() {
    let result = fake_pg_dump("SUPER_SECRET_SENTINEL").await;
    assert!(!result.command_line.contains("SUPER_SECRET_SENTINEL"));
    assert!(!result.sanitized_log.contains("SUPER_SECRET_SENTINEL"));
}
```

- [x] **Step 2:** Run target; expect adapter absent.
- [x] **Step 3:** Detect executable/version; enforce compatible major; create permission-restricted temporary passfile/defaults-extra-file, pass its path via supported environment/flag, delete on all exits; capture bounded sanitized stdout/stderr; terminate process tree on cancel.
- [x] **Step 4:** Run sentinel, version mismatch and cancel tests on all OS abstractions.
- [x] **Step 5:** Commit with `git commit -m "feat(backup): wrap native tools without leaking secrets"`.

### Task 6: Add transfer TUI and CLI workflows

- [x] **Step 1:** Add CLI tests for stdin/file, mapping, formats and non-interactive error strategy; TUI snapshots for preview/progress/rejects.
- [x] **Step 2:** Run; expect absent workflows.
- [x] **Step 3:** Implement `dexo export`, `dexo import`, progress/cancel/retry-safe UI, local file chooser and native backup/restore actions in command palette.
- [x] **Step 4:** Run full sprint gate with 1m-row streaming fixture.
- [x] **Step 5:** Commit with `git commit -m "feat: expose import export backup and restore"`.

## Sprint exit

- [x] All formats cover NULL/empty/binary semantics.
- [x] Export cancellation preserves original destination.
- [x] Imports report exact line errors and strategy outcomes.
- [x] One-million-row test stays within recorded memory budget.
- [x] Secret sentinel is absent from args/env logs and artifacts after cleanup.
