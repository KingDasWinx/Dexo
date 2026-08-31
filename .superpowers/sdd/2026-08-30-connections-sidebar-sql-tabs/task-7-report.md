# Task 7 report

## Completed

- Bound `Ctrl+N` to `document.new` in the editor context for the default, Vim, and Emacs keymaps.
- Kept Emacs `Ctrl+N` mapped to `results.down` in the results context.
- Advertised `Ctrl+N` as the New Document accelerator in the command palette.
- Added focused sidebar and editor footer hints.
- Added a keyboard-flow test proving `Ctrl+N` creates a unique document bound to the active connection.

## TDD

The new keyboard-flow test failed before the binding was added because the active document remained unbound. It passes after the implementation.

## Tests

- `cargo test -p dexo-tui --test workbench_sidebar_flow ctrl_n_creates_a_document_bound_to_the_active_connection`
- `cargo test -p dexo-tui keymap::tests::builtin_profiles_parse`
- `cargo test -p dexo-tui --lib widgets::status`

All targeted tests passed. `cargo test -p dexo-tui` has one unrelated pre-existing failure:
`palette::tests::every_current_action_is_in_palette` expects 131 registry entries, while the current
registry has 133. The existing `unreachable pattern` warning in `crates/dexo-tui/src/update.rs`
also remains unrelated to this task.

## Round 1 corrections

- Scoped connection shortcuts to the sidebar Connections focus.
- Restored the Catalog footer's contextual table and expand/collapse hints.
- Made the editor footer available on every workbench tab while the editor has focus.
- Added focused unit coverage for Connections versus Catalog footer selection and the editor footer.

### Round 1 tests

- `cargo test -p dexo-tui widgets::status::tests`
- `cargo test -p dexo-tui --test workbench_sidebar_flow ctrl_n_creates_a_document_bound_to_the_active_connection`

Both passed. The full-suite palette entry-count failure and unrelated `update.rs` warning above remain.
