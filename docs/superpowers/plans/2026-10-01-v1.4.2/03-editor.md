# 1.4.2 Section 3: Editor Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The SQL editor stops losing to the basics: find and replace, line editing, an external editor, psql's backslash commands, a real Vim mode, and diagnostics as you type.

**Architecture:** Everything edits `dexo_sql::SqlDocument` (a rope with grouped undo, char-indexed cursor and selection) through the helpers in `screens/editor.rs`. Find is one engine (`screens/find.rs`) that the find bar, Vim's `/ n N` and `:s` share. Overlays -- matches and diagnostics -- are painted onto the buffer after the editor paragraph renders, so they compose with syntax highlighting instead of replacing it. Backslash commands are answered in `dexo-app` from the session's `CatalogReader` and come back through the same result events a query uses. The external editor is a request on the model that the event loop, which owns the terminal, carries out between frames.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 3 (B1, B2, B3, B6, B5, B4). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- From section 1: a new statement form gets an answer from `is_read` and `destructive`; backslash commands are reads (never sent to the server).
- Tests only where an edit's correctness has no other check (the find engine, line edits, Vim operators, backslash parsing).

## Order

B1 → B2 → B3 → B6 → B5 → B4. B5 after B1 because `/ n N` and `:s` use B1's engine.

## Rulings made from the code

- **Keys.** Checked against `keymap.rs` in all three profiles. Ctrl+F find and Ctrl+H find-and-replace (editor context, every profile). Without the kitty keyboard protocol a terminal sends Ctrl+Backspace as ^H, which is Ctrl+H; Alt+Backspace still deletes a word there. Toggle comment Ctrl+/ (also Ctrl+_ and Ctrl+7, what terminals send for it). Duplicate line Ctrl+Shift+D. Move line Ctrl+Shift+Up/Down (Alt+Up/Down already resize the results pane). External editor Ctrl+E in Default and Vim, `ctrl+x ctrl+e` in Emacs, as in bash.
- **Find bar.** A row (two with replace) at the bottom of the editor pane. Enter or F3 next, Shift+Enter or Shift+F3 previous, Alt+C case-sensitive, Alt+W whole word, Tab moves between find and replace, Enter in the replace field replaces the current match, Alt+A replaces all as one undo step, Esc closes. Patterns are literal text; `:s` uses the same literal engine.
- **External editor.** `$VISUAL`, then `$EDITOR`, then `notepad` on Windows and `vi` elsewhere, run through the shell so `code --wait` works, on a temp file with a `.sql` suffix. The event loop leaves the alternate screen and raw mode, drops its input stream so the child gets the keys, waits, then re-enters and redraws. A non-zero exit or an unreadable file leaves the buffer as it was.
- **Backslash commands.** A statement starting with `\` ends at the end of its line. `dexo_app::meta_command` parses `\dt \dv \di \dn \df [pattern]`, `\d name`, `\l`, `\x`, `\?`; patterns are psql's `*` and `?` wildcards, case-insensitive, matched on the object name or `schema.name`. Answers are columns and rows from `CatalogReader`, run in `query_runner` instead of the driver. `\x` toggles the result's record view.
- **Vim.** `screens/vim.rs` holds the state machine; `update` gives it the key after the keymap has had its chance at Ctrl/Alt/F-key chords. The mode is in the status bar; `:` and `/` type into the status line.
- **Diagnostics.** Parse errors come from the per-statement sqlparser pass; unknown tables and columns are reported only when the catalog has loaded that schema's tables (or that table's columns) -- never guessed. A failed run's server position (`DriverError::position`, 1-based chars into the statement) becomes a diagnostic and moves the cursor.

## Tasks

### Task 1 (B1): Find and replace
- `screens/find.rs`: `find_all(text, query, options) -> Vec<Range<usize>>` (char ranges, case and whole-word options), `nearest(matches, cursor, forward)`.
- `FindState` on the model; actions `OpenFind { replace }`, key handling in the bar, palette commands `editor.find` and `editor.replace`, match overlay in `widgets/editor.rs`, count `3/12` in the bar.
- Commit: `feat(editor): find and replace`.

### Task 2 (B2): Line editing
- Toggle `--` comments on the line or every line of the selection (all commented → uncomment), duplicate line or selection, move line or selected lines up and down; each one undo step; palette commands with their keys.
- Commit: `feat(editor): toggle comments, duplicate and move lines`.

### Task 3 (B3): External editor
- `Action::EditExternally`, `Model::external_edit`, the event loop's suspend and resume, `Action::ExternalEditFinished { document, text }` replacing the buffer as one undo step.
- Commit: `feat(editor): edit the document in $EDITOR`.

### Task 4 (B6): psql backslash commands
- `dexo_sql`: backslash statements split at end of line, are reads, and are never destructive.
- `dexo_app::meta_command`: parse and answer; `query_runner` answers them without the driver; `\x` toggles the record view; `\?` lists them.
- Commit: `feat(editor): psql's \d commands, answered from the catalog`.

### Task 5 (B5): Vim mode
- Normal, Insert, Visual, Visual-line; motions `h j k l w b e 0 ^ $ gg G` with counts; operators `d c y` with motions, `dd yy cc`, `x p P u Ctrl+R`, `i a I A o O`, `v V`, `.`, `/ n N`, `:s/old/new/[g]` and `:%s`, `:w :q :wq`. Mode in the status bar; block cursor outside Insert.
- Commit: `feat(editor): a modal Vim mode`.

### Task 6 (B4): Live diagnostics
- `Diagnostic`s computed with the highlights; underlined in the editor; the message in the status line with the cursor on one; a failed run's position becomes one and moves the cursor.
- Commit: `feat(editor): errors are underlined as you type`.

## Review Focus

1. Replace all, comment toggles, line moves and the external edit are each exactly one undo step, and never corrupt multi-byte text.
2. The external editor never leaves the terminal in raw mode or the alternate screen, and its keys never reach Dexo.
3. A backslash command is never sent to the server, on any driver, including inside a multi-statement run.
4. Vim's Normal mode never inserts text; Ctrl chords (palette, run, quit) still work in every mode.
5. A diagnostic never claims a table or column is unknown when the catalog simply has not loaded it.
