# Dexo Sprint 14: UX Recovery and Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Completar personalização, acessibilidade, recuperação de sessão e diagnósticos locais sanitizados.

**Architecture:** Theme/keymap/layout are validated data, not widget-specific state. Recovery checkpoints are transactional and logs flow through one redaction layer shared by TUI/CLI/MCP.

**Tech Stack:** Ratatui 0.30.2/Crossterm 0.29, TOML 1.1/Serde 1.0, tracing-appender 0.2.5, SQLite migration 7, terminal capability detection.

---

## File map

- Create: `dexo-tui/src/{theme.rs,keymap.rs,capabilities.rs,accessibility.rs}`
- Create: `dexo-storage/src/{layout.rs,session_recovery.rs,diagnostic.rs}` and migration 7
- Create: `dexo-app/src/{recovery_service.rs,diagnostic_service.rs}`
- Create: `dexo-tui/src/screens/{settings.rs,recovery.rs,mcp_audit.rs}`
- Test: theme/keymap/property/snapshot/crash/sentinel tests.

### Task 1: Validate themes and terminal capabilities

- [x] **Step 1:** Add snapshots for 16/256/true-color, ASCII fallback and `NO_COLOR`; assert production/error/selection remain distinguishable by text/symbol.
- [x] **Step 2:** Run; expect theme model absent.
- [x] **Step 3:** Implement semantic roles instead of raw colors, capability detector and built-in light/dark/low-color themes. Invalid custom theme reports exact TOML field and falls back without overwriting file.
- [x] **Step 4:** Approve snapshots on all capability profiles.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): add accessible terminal themes"`.

### Task 2: Add conflict-free configurable keymaps

- [x] **Step 1:** Add tests for default/Vim/Emacs profiles, multi-key chords, context overlap and exact conflict diagnostics.
- [x] **Step 2:** Run; expect keymap absent.
- [x] **Step 3:** Parse keymap TOML to stable command IDs; allow same key in disjoint contexts; reject ambiguity in same active context; command palette remains fallback for every action.
- [x] **Step 4:** Run property test ensuring every registered command is palette-reachable.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): add validated keymap profiles"`.

### Task 3: Persist project layouts and settings

- [x] **Step 1:** Add migration/round-trip tests for panel visibility/sizes, tabs, focused panel, theme/keymap/mouse/animation/Unicode settings.
- [x] **Step 2:** Run; expect repository absent.
- [x] **Step 3:** Add migration 7; clamp restored geometry to current terminal; unknown future fields remain in TOML config; database layout has explicit version.
- [x] **Step 4:** Run v6->v7 and resize/compact restoration tests.
- [x] **Step 5:** Commit with `git commit -m "feat(storage): persist workbench layouts and preferences"`.

### Task 4: Recover safely from crashes

- [x] **Step 1:** Add subprocess test killing Dexo after document/layout checkpoint and reopening; assert recovery offered, secret/parameter absent, active transaction shown as lost/unknown not active.
- [x] **Step 2:** Run; expect incomplete recovery.
- [x] **Step 3:** Checkpoint debounced document/layout state with clean-shutdown marker; startup offers recover/discard; never serialize session handles, secrets or parameter values; terminal panic hook restores terminal before diagnostic.
- [x] **Step 4:** Run kill/reopen tests on native OS CI.
- [x] **Step 5:** Commit with `git commit -m "feat(recovery): restore non-sensitive session state"`.

### Task 5: Centralize sanitized local observability

- [x] **Step 1:** Add sentinel tests across connection, SQL parameters, MCP, native tools and panic contexts.
- [x] **Step 2:** Run; expect leaks or missing diagnostics.
- [x] **Step 3:** Define structured safe fields; secret wrappers never implement Display; rotate local logs by size/count; diagnostic bundle includes versions/capabilities/config-with-redaction/log-tail and user preview, never automatic upload.
- [x] **Step 4:** Run sentinel scan over database, TOML, logs, audit and diagnostic zip.
- [x] **Step 5:** Commit with `git commit -m "feat(observability): produce sanitized local diagnostics"`.

### Task 6: Finish settings and MCP operational screens

- [x] **Step 1:** Add full/compact snapshots for settings, recovery, MCP profile scopes, tool/resource preview, grants countdown, audit and revoke-all.
- [x] **Step 2:** Run; expect missing states.
- [x] **Step 3:** Bind screens to existing services; every change shows effective diff; destructive reset/revoke actions use confirmation; mouse can be fully disabled.
- [x] **Step 4:** Run accessibility navigation solely by keyboard and no-color snapshot suite.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): complete settings recovery and MCP screens"`.

### Task 7: Verify no feature is mouse/color/Unicode-only

- [x] **Step 1:** Add automated action registry audit and manual test script `docs/testing/accessibility-checklist.md` with exact commands/expected screens.
- [x] **Step 2:** Run audit; record failures.
- [x] **Step 3:** Add missing textual labels, ASCII symbols, focus order and command-palette actions; document terminal capability diagnostics.
- [x] **Step 4:** Run full workspace, snapshots and native recovery jobs.
- [x] **Step 5:** Commit with `git commit -m "test(tui): enforce terminal accessibility contracts"`.

## Sprint exit

- [x] Themes/keymaps/layouts validate and round-trip.
- [x] Crash recovery contains documents/layout only.
- [x] Secret sentinel absent from all persisted/diagnostic artifacts.
- [x] All actions work without mouse, color or Unicode.
- [x] MCP grants/audit can be inspected/revoked locally.
