# Keybind customization: Insert takes text first, Vim keys remap, editor motions are commands

Status: draft for review. Date: 2026-10-12. Issue: #13.

## Why

Issue #13 (a Colemak-DH user) found three things while writing a `keymap.toml`:

1. `[global] "r" = "focus.results"` fires in Vim's Insert mode, mid-word. The keymap
   resolves before Vim and before the editor (`update.rs::handle_key`, the block that
   builds `chord` and calls `model.keymap.resolve`), so a bare key bound anywhere beats
   typing. The default profile has the same hole: nothing stops a bare `[global]` key
   from eating a letter in a plain editor.
2. Vim's Normal and Visual keys cannot be changed. `screens/vim.rs` parses characters
   (`motion_of`, `parse`, `"xpPuiaIAoOvV.nN/:DCYX"`); `i` alone appears as the insert
   command, the `iw` text object and Visual's `i`. The user's fork changed one of them
   and nothing moved.
3. The Emacs profile's motions (`ctrl+a`, `ctrl+e`, `ctrl+f`, `ctrl+b`, `ctrl+n`,
   `ctrl+p`, `ctrl+d`, `ctrl+k`, `ctrl+v`, `alt+f`, `alt+b`, `alt+d`, `alt+v`, `alt+<`,
   `alt+>`) are read in `screens/editor.rs::emacs_key`, after the keymap and outside it.
   "The keymap has no commands for moving the cursor", so no profile can rebind them.

## Goal and success criteria

With only `keymap.toml`, and no code:

1. A bare key bound in `[global]` or `[editor]` works in Vim's Normal mode and is typed
   in Insert mode and in the default profile.
2. A Colemak-DH user moves with `m n e i` in Normal and Visual mode, enters Insert with
   `j`, and `dm`, `3n`, `vi`, `ciw` and `.` keep working.
3. An Emacs user rebinds `ctrl+n`; a default-profile user binds a key to "line end".
4. A file that asks for two things at once -- a key both remapped and bound -- is refused
   with the line, the two sides, and the way out.

Out of scope, by decision: a Helix mode, `[vim]` remaps shown in the F1 overlay, remaps
inside the `:` and `/` lines.

## 1. Insert takes text first

`update.rs::handle_key`, before the keymap block: while the editor **takes text**, a key
that **types** goes to the editor and never reaches the keymap.

- *takes text*: `effective_focus() == Focus::Editor`, the active document is not a
  table, `find` is closed, `pending_chord` is empty, and either Vim is not active or
  `vim::mode(model) == Mode::Insert`.
- *types*: modifiers are empty or `SHIFT` alone, and the code is `Char`, `Enter`, `Tab`,
  `BackTab`, `Backspace` or `Delete`. The same keys `TextInput::edit_for` calls plain.
- Arrows, Home, End, PageUp, PageDown, function keys and Esc keep going through the
  keymap first. No built-in profile binds them in `[editor]`, so nothing changes there,
  and they stay bindable.
- A pending chord (`ctrl+g` then `w`) still completes the chord: `pending_chord` must be
  empty for the rule to apply.

The control flow: the keymap block (prefix check, `resolve`, the `data.review` and
`grid_only` special cases, conflict message) runs only when the rule does not apply; the
rest of the function -- Vim, then `editor::handle_key` -- is unchanged.

Effect on the issue's file: `r` focuses the results in Normal mode; in Insert mode and in
the default profile it types an `r`.

## 2. `[vim]` remaps a key to the key Vim reads

### Format

```toml
[vim]
"m" = "h"
"n" = "j"
"e" = "k"
"i" = "l"
"j" = "i"
"N" = "J"
"ctrl+u" = "ctrl+r"
```

Both sides are keys, not commands. One character alone is taken as written, case kept:
`"N" = "J"` and `"shift+n" = "shift+j"` are the same line. Anything longer is the chord
syntax of the other sections (`ctrl+u`, `esc`, `space`). `""` on the right undoes a remap
an earlier line, or the base, made. The section is read only with the Vim profile; with
another it does nothing, and the docs say so.

### Keymap

`keymap.rs`:

- `Keymap` gains `pub vim: HashMap<String, KeySpec>`: the source key's `key_label` (the
  normalized spelling -- `Char('N')` and `shift+n` both label `shift+n`) to the target
  key. The built-in profiles have it empty.
- `entries()` returns the remaps beside the bindings: for `section == "vim"` each
  `spec = target` becomes a `Remap { label, spec, to: Option<KeySpec>, line }`, parsed with
  `parse_vim_key` (one char → `KeySpec { NONE, Char(c) }`; else `parse_key`). A target
  that is not a string, or a key that does not parse, is a `KeymapError` on its line, as
  for a binding.
- `merge_overlay` applies the remaps after the bindings: `remove(label)`, then `insert` if
  `to` is `Some`. Then it validates each remap that is `Some`: a binding in the merged
  keymap whose context is `Global` or `Editor` and whose chord's **first** key labels the
  same as the remap's source is refused --

  > `[vim] e`: `e` is bound to focus.explorer in [global], which wins over the remap;
  > unbind one with ""

  The first key covers the one-key chord and the prefix of a longer one (`g` would wait
  for `g g`), the same reason the "would never fire" refusal exists.
- `Keymap::vim_key(&self, key: KeyEvent) -> KeyEvent`: the target for `key`'s label, or
  `key` itself. A target `Char(c)` with `SHIFT` in its modifiers, or an uppercase `c`,
  becomes `Char(c.to_ascii_uppercase())`, which is what `vim.rs` pushes into `pending`.

### Vim

`screens/vim.rs::handle_key`: after the prompt check and before the Insert check,
`let key = model.keymap.vim_key(key);`. So:

- Normal and Visual read the remapped key: motions, operators, counts, text objects.
- In Insert mode only the Esc test sees it (`ctrl+c` → `esc` works); the typed keys reach
  the editor as `update` holds them, untranslated.
- `pending_keys` keeps the original keys. `.` replays them through `handle_key`, which
  translates again; the recording code does not change.
- The `:` and `/` lines are typed before this point and are not remapped.

## 3. Editor motions as commands

### Actions

`action.rs`, three variants:

```rust
EditorMove(EditorMotion),
EditorDeleteForward { word: bool },
EditorKillLine,
```

with `pub enum EditorMotion { Left, Right, Up, Down, WordLeft, WordRight, LineStart,
LineEnd, DocumentStart, DocumentEnd, PageUp, PageDown }` beside `Action`.

### Commands

`palette/registry.rs`, fifteen `CommandSpec`, `shortcut: None`, `requirements: &[]`,
`Dispatch(...)`:

| id | title | action |
| --- | --- | --- |
| `editor.left` | Cursor Left | `EditorMove(Left)` |
| `editor.right` | Cursor Right | `EditorMove(Right)` |
| `editor.up` | Cursor Up | `EditorMove(Up)` |
| `editor.down` | Cursor Down | `EditorMove(Down)` |
| `editor.word_left` | Word Left | `EditorMove(WordLeft)` |
| `editor.word_right` | Word Right | `EditorMove(WordRight)` |
| `editor.line_start` | Line Start | `EditorMove(LineStart)` |
| `editor.line_end` | Line End | `EditorMove(LineEnd)` |
| `editor.document_start` | Document Start | `EditorMove(DocumentStart)` |
| `editor.document_end` | Document End | `EditorMove(DocumentEnd)` |
| `editor.page_up` | Page Up | `EditorMove(PageUp)` |
| `editor.page_down` | Page Down | `EditorMove(PageDown)` |
| `editor.delete_forward` | Delete Character | `EditorDeleteForward { word: false }` |
| `editor.delete_word_forward` | Delete Word | `EditorDeleteForward { word: true }` |
| `editor.kill_line` | Kill Line | `EditorKillLine` |

`tests/command_palette_contract.rs` lists the fifteen ids in `COMMAND_IDS`.

### Handlers

`update.rs`, next to `Action::EditorUndo`, with its guard. They call `screens/editor.rs`:

- `pub(crate) fn motion(model, EditorMotion)`: `Left`/`Right` → `move_chars(±1, false,
  false)`; `WordLeft`/`WordRight` → `move_chars(±1, false, true)`; `Up`/`Down` → with the
  completion popup open `move_completion(∓1)`, else `move_vertical(±1, false)` (what
  `emacs_key` does for `ctrl+n`/`ctrl+p`); `LineStart`/`LineEnd` → `move_line_edge(home,
  false, false)`; `DocumentStart`/`DocumentEnd` → `move_line_edge(home, false, true)`;
  `PageUp`/`PageDown` → `page(∓1, false)`.
- `delete(model, word)` and `kill_line(model)` become `pub(crate)`.
- After a delete or kill, `refresh_intelligence(model, false)`, as `EditorUndo` does.

### Emacs

`EMACS_TOML [editor]` binds them:

```toml
"ctrl+a" = "editor.line_start"
"ctrl+e" = "editor.line_end"
"ctrl+b" = "editor.left"
"ctrl+f" = "editor.right"
"ctrl+n" = "editor.down"
"ctrl+p" = "editor.up"
"ctrl+d" = "editor.delete_forward"
"ctrl+k" = "editor.kill_line"
"ctrl+v" = "editor.page_down"
"alt+f" = "editor.word_right"
"alt+b" = "editor.word_left"
"alt+d" = "editor.delete_word_forward"
"alt+v" = "editor.page_up"
"alt+<" = "editor.document_start"
"alt+>" = "editor.document_end"
```

None of these chords is in the Emacs profile's `[global]` or `[editor]` today (`alt+b`,
`alt+d` and `ctrl+d` are the default profile's; Emacs left them out for this reason).
`emacs_key` and `kill_line`'s Emacs-only doc go; so does the `keymap.name == "emacs"`
test in `editor::handle_key`. `tests/editor_emacs_keys.rs` passes as it is: the same
functions run, reached through the keymap.

## 4. Docs, tests, release

- `docs/src/workbench.md`: "Keymap overlay" gains the `[vim]` section and the rule of
  section 1; "Vim mode" points to `[vim]` for other layouts; the editor table says the
  Emacs motions are commands any profile can bind.
- No new tests (the user writes their own). The tests the change touches are updated:
  `COMMAND_IDS`. The existing suites must pass.
- CHANGELOG is written by `publish.sh` from the commits; nothing to edit. One commit per
  section, `feat(tui)` / `fix(tui)`, in the order 1, 2, 3, 4.
- Validation: `cargo fmt --all`, `cargo clippy --all-features -- -D warnings`, `cargo
  build`, `cargo test -p dexo-tui`.
- A reply for issue #13: what changed, the Colemak-DH `keymap.toml`, and that
  single-key "minimodes" are chords -- `[editor] "space f" = "document.open"` waits for
  the second key in Normal mode and shows what can follow, and `space` types in Insert.
