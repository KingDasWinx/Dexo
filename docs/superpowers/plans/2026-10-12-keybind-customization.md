# Keybind Customization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `keymap.toml` alone makes bare keys safe to bind (typed while the editor takes text), remaps the keys Vim reads for another layout, and binds the editor's motions in any profile.

**Architecture:** One guard in `update.rs::handle_key` keeps the keymap from reading a key that types while the editor takes text. `keymap.rs` reads a `[vim]` section into `Keymap::vim`, a key-label → key map that `vim::handle_key` applies before parsing. The Emacs profile's hardcoded motions become `Action`s dispatched by fifteen `editor.*` commands, bound in `EMACS_TOML`.

**Tech Stack:** Rust 2024, crossterm 0.29 (`KeyEvent`, `KeyCode`, `KeyModifiers`), toml 1.1 (`DeTable`), the crate's own `palette` registry.

**Spec:** `docs/superpowers/specs/2026-10-12-keybind-customization-design.md`

## Global Constraints

- The user writes their own tests: **no new `#[test]` functions**, except the one in Task 2 (the `[vim]` parser has no other way to be checked without the TUI). Update only the tests a change breaks.
- Every task ends green on `cargo fmt --all`, `cargo clippy -p dexo-tui --all-features -- -D warnings`, `cargo build -p dexo-tui`, and the test runs named in the task.
- One commit per task, message in the repo's style (`fix(tui): ...`, `feat(tui): ...`, `docs(workbench): ...`), ending with the session line `Claude-Session: https://claude.ai/code/session_014NYSWisD33UHaLTrJZYUtc`. No push.
- Comments match the file's voice: a sentence that says why, not what. No `ponytail:` marks are needed here; nothing cuts a corner.
- Config and doc files carry the setting, not prose about it.

## Review Focus

1. A chord of bare keys begun in Normal mode (`space` of `space f`) then a switch to Insert mode before the second key: `pending_chord` is non-empty, so `editor_takes_text` is false and the second key completes the chord, as before. — Task 1 keeps `pending_chord.keys.is_empty()` in the guard; `cargo test -p dexo-tui vim` covers the chord path indirectly.
2. `[vim]` with a profile other than Vim: the section parses and is kept, and nothing reads it (`vim::active` is false, so `vim_key` is never called). — Task 2's test asserts `Keymap::default_profile().vim` and `Keymap::vim_profile().vim` are empty and an overlay with `[vim]` merges.
3. A remap whose source is a key the `[results]` section binds (`k` → `results.up`): not refused, since Vim never runs there. — Task 2's validation checks `Global | Editor` only; the test binds in `[results]` and expects `Ok`.
4. `ctrl+h` on a terminal without the kitty protocol arrives as Ctrl+Backspace (see the top of `handle_key`): with `CONTROL` set it never counts as typing, so the editor's word delete still runs. — Task 1's `types` requires modifiers ⊆ `SHIFT`.
5. `Action::EditorMove` run from the palette with the results focused moves the editor's cursor out of sight, as `editor.undo` already does. — Accepted; the commands are hidden from the palette (Task 3), so only a keymap reaches them, and a keymap binds them in `[editor]`.

---

### Task 1: A key that types is typed while the editor takes text

**Files:**
- Modify: `crates/dexo-tui/src/update.rs:6416-6427` (the chord build, prefix check and `resolve` in `handle_key`)
- Modify: `crates/dexo-tui/src/update.rs:6514` (add two helpers before `active_key_context`)

**Interfaces:**
- Consumes: `crate::screens::vim::{active, mode, Mode}`, `Model::effective_focus`, `Model::active_document().kind.is_table()`, `model.find.open`, `model.pending_chord`.
- Produces: `fn editor_takes_text(model: &Model) -> bool`, `fn types(key: &KeyEvent) -> bool` (private to `update.rs`).

- [ ] **Step 1: Short-circuit the keymap while typing**

In `crates/dexo-tui/src/update.rs`, replace

```rust
    let spec = crate::keymap::KeySpec {
        modifiers: key.modifiers,
        code: key.code,
    };
    let mut chord = model.pending_chord.clone();
    chord.keys.push(spec);
    let ctx = active_key_context(model);
    if model.keymap.is_prefix(&chord, ctx) {
        model.pending_chord = chord;
        return Vec::new();
    }
    match model.keymap.resolve(&chord, ctx) {
```

with

```rust
    let spec = crate::keymap::KeySpec {
        modifiers: key.modifiers,
        code: key.code,
    };
    let mut chord = model.pending_chord.clone();
    chord.keys.push(spec);
    let ctx = active_key_context(model);
    // While the editor takes text, a key that types is typed, whatever the keymap binds
    // it to: `[global] "r" = "focus.results"` left the results focused mid-word in Insert
    // mode, and in the default profile.
    let typing = editor_takes_text(model) && types(&key);
    if !typing && model.keymap.is_prefix(&chord, ctx) {
        model.pending_chord = chord;
        return Vec::new();
    }
    let resolved = if typing {
        Ok(None)
    } else {
        model.keymap.resolve(&chord, ctx)
    };
    match resolved {
```

The rest of the `match` (`Ok(Some(command))`, `Ok(None)`, `Err(conflict)`) stays as it is.

- [ ] **Step 2: Add the two helpers**

In `crates/dexo-tui/src/update.rs`, directly above `pub(crate) fn active_key_context(model: &Model) -> crate::keymap::KeyContext {`, insert:

```rust
/// Whether a key the keymap leaves alone would be typed: the editor has the focus on a
/// document, with no bar, prompt or chord in the way, and Vim -- when it is on -- is in
/// Insert mode.
fn editor_takes_text(model: &Model) -> bool {
    model.effective_focus() == Focus::Editor
        && !model.active_document().kind.is_table()
        && !model.find.open
        && model.pending_chord.keys.is_empty()
        && (!crate::screens::vim::active(model)
            || crate::screens::vim::mode(model) == crate::screens::vim::Mode::Insert)
}

/// A key that puts text in or takes it out: a character, Enter, Tab, Backspace or
/// Delete, with at most Shift. The arrows and the function keys are not, so a keymap can
/// still bind them.
fn types(key: &KeyEvent) -> bool {
    key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        && matches!(
            key.code,
            KeyCode::Char(_)
                | KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Backspace
                | KeyCode::Delete
        )
}
```

- [ ] **Step 3: Build, lint, run the suites the dispatch touches**

Run:
```bash
cargo fmt --all
cargo clippy -p dexo-tui --all-features -- -D warnings
cargo test -p dexo-tui vim
cargo test -p dexo-tui --test editor_emacs_keys
cargo test -p dexo-tui --test data_flow
```
Expected: all pass. The Vim tests type through `update` in Insert mode and must read the same text as before.

- [ ] **Step 4: Commit**

```bash
git add crates/dexo-tui/src/update.rs
git commit -m "fix(tui): a key that types is typed while the editor takes text, whatever the keymap binds it to

A bare key bound in [global] or [editor] -- \"r\" = \"focus.results\" -- fired in Vim's
Insert mode and in the default profile, mid-word. The keymap now reads a key that types
only where the editor does not take text: Vim's Normal and Visual modes, the other panes.

Claude-Session: https://claude.ai/code/session_014NYSWisD33UHaLTrJZYUtc"
```

---

### Task 2: `[vim]` in keymap.toml remaps the keys Vim reads

**Files:**
- Modify: `crates/dexo-tui/src/keymap.rs:60-64` (`Keymap`), `:291-383` (`Entry`, `entries`, `parse_keymap`), `:419-479` (`merge_overlay`), `:495-513` (add `parse_vim_key` beside `parse_chord`), `:1350` (the `Keymap` literal in a test), and the test module.
- Modify: `crates/dexo-tui/src/screens/vim.rs:140-205` (`handle_key`).

**Interfaces:**
- Consumes: `key_label(&KeySpec) -> String`, `chord_label`, `section_name`, `parse_key`, `KeymapError`.
- Produces: `Keymap { vim: HashMap<String, KeySpec> }`, `Keymap::vim_key(&self, KeyEvent) -> KeyEvent`, `fn parse_vim_key(&str) -> Result<KeySpec, String>` (private), `struct Remap` (private), `entries` returning `(Option<String>, Vec<Entry>, Vec<Remap>)`.

- [ ] **Step 1: The field**

In `crates/dexo-tui/src/keymap.rs`, replace

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    pub name: String,
    pub bindings: Vec<Binding>,
}
```

with

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    pub name: String,
    pub bindings: Vec<Binding>,
    /// `[vim]`: the key Vim reads for a key pressed, by the pressed key's label. Empty in
    /// the built-in profiles; keymap.toml fills it for another layout.
    pub vim: HashMap<String, KeySpec>,
}
```

- [ ] **Step 2: `vim_key` on `Keymap`**

In the `impl Keymap` block, directly after `pub fn command_ids(&self) -> Vec<&str> { ... }`, add:

```rust
    /// The key Vim reads for `key`: what `[vim]` remaps it to, or `key` itself. On a
    /// Colemak layout `m` is `h`.
    pub fn vim_key(&self, key: KeyEvent) -> KeyEvent {
        let spec = KeySpec {
            modifiers: key.modifiers,
            code: key.code,
        };
        let Some(to) = self.vim.get(&key_label(&spec)) else {
            return key;
        };
        // Vim reads a capital as the shifted command: `J` joins, where `j` moves.
        let code = match to.code {
            KeyCode::Char(ch) if to.modifiers.contains(KeyModifiers::SHIFT) => {
                KeyCode::Char(ch.to_ascii_uppercase())
            }
            code => code,
        };
        KeyEvent::new(code, to.modifiers)
    }
```

- [ ] **Step 3: `Remap`, and `entries` reading `[vim]`**

Replace the `Entry` struct's neighbourhood: directly after

```rust
/// One `chord = "command"` of a keymap file, with the line it is on.
struct Entry {
    section: String,
    spec: String,
    binding: Binding,
    line: usize,
}
```

add

```rust
/// One `key = "key"` of a `[vim]` section: the pressed key as written and by its label,
/// the key Vim reads for it -- `None` for `""`, which undoes a remap -- and the line.
struct Remap {
    spec: String,
    label: String,
    to: Option<KeySpec>,
    line: usize,
}
```

Then change `entries`. Its doc comment and signature become

```rust
/// A keymap file's `profile` name, its entries and its `[vim]` remaps, in file order. An
/// entry whose command is `""` (an overlay's unbind) is kept, with that empty command.
fn entries(src: &str) -> Result<(Option<String>, Vec<Entry>, Vec<Remap>), KeymapError> {
```

After `let mut entries = Vec::new();` add `let mut remaps = Vec::new();`. Inside the `for (key, value) in sections` loop, directly after the `if section == "profile" { ... continue; }` block and before `let context = parse_context(&section)...`, insert:

```rust
        if section == "vim" {
            let toml::de::DeValue::Table(map) = value.get_ref() else {
                return Err(KeymapError {
                    field: section,
                    reason: "a section must be a table of key = key".into(),
                    line: Some(line),
                });
            };
            let mut map: Vec<_> = map.iter().collect();
            map.sort_by_key(|(spec, _)| spec.span().start);
            for (spec, target) in map {
                let line = line_of(spec.span().start);
                let spec = spec.get_ref().to_string();
                let field = format!("[vim] {spec}");
                let target = target.get_ref().as_str().ok_or_else(|| KeymapError {
                    field: field.clone(),
                    reason: "the key Vim reads must be a string".into(),
                    line: Some(line),
                })?;
                let from = parse_vim_key(&spec).map_err(|reason| KeymapError {
                    field: field.clone(),
                    reason,
                    line: Some(line),
                })?;
                let to = if target.is_empty() {
                    None
                } else {
                    Some(parse_vim_key(target).map_err(|reason| KeymapError {
                        field: field.clone(),
                        reason,
                        line: Some(line),
                    })?)
                };
                remaps.push(Remap {
                    label: key_label(&from),
                    spec,
                    to,
                    line,
                });
            }
            continue;
        }
```

Change the function's last line from `Ok((profile, entries))` to `Ok((profile, entries, remaps))`.

- [ ] **Step 4: `parse_keymap` keeps the remaps**

Replace

```rust
pub fn parse_keymap(src: &str) -> Result<Keymap, KeymapError> {
    let (profile, entries) = entries(src)?;
    Ok(Keymap {
        name: profile.unwrap_or_else(|| "custom".into()),
        bindings: entries.into_iter().map(|entry| entry.binding).collect(),
    })
}
```

with

```rust
pub fn parse_keymap(src: &str) -> Result<Keymap, KeymapError> {
    let (profile, entries, remaps) = entries(src)?;
    Ok(Keymap {
        name: profile.unwrap_or_else(|| "custom".into()),
        bindings: entries.into_iter().map(|entry| entry.binding).collect(),
        vim: remaps
            .into_iter()
            .filter_map(|remap| Some((remap.label, remap.to?)))
            .collect(),
    })
}
```

- [ ] **Step 5: `merge_overlay` applies and validates the remaps**

In `merge_overlay`, change `let (_, entries) = entries(src)?;` to `let (_, entries, remaps) = entries(src)?;`. Then replace its final `Ok(keymap)` with:

```rust
    for remap in &remaps {
        keymap.vim.remove(&remap.label);
        if let Some(to) = &remap.to {
            keymap.vim.insert(remap.label.clone(), to.clone());
        }
    }
    // A remapped key the keymap also binds, alone or as the start of a chord, in the
    // editor or everywhere: the keymap reads it first, and Vim would never see it.
    for remap in remaps.iter().filter(|remap| remap.to.is_some()) {
        if let Some(bound) = keymap.bindings.iter().find(|binding| {
            matches!(binding.context, KeyContext::Global | KeyContext::Editor)
                && key_label(&binding.chord.keys[0]) == remap.label
        }) {
            return Err(KeymapError {
                field: format!("[vim] {}", remap.spec),
                reason: format!(
                    "`{}` is bound to {} in [{}], which wins over the remap; unbind one with \"\"",
                    chord_label(&bound.chord),
                    bound.command,
                    section_name(bound.context)
                ),
                line: Some(remap.line),
            });
        }
    }
    Ok(keymap)
```

Update the doc comment above `merge_overlay` by appending one sentence: `A [vim] line replaces the base's remap of that key, and "" removes it; a remapped key the editor or global section binds is refused, since the keymap reads it first.`

- [ ] **Step 6: `parse_vim_key`**

Directly above `pub fn parse_chord(spec: &str) -> Result<Chord, String> {`, add:

```rust
/// A key as a `[vim]` line writes it: one character is that character, case kept --
/// `"N" = "J"` -- and anything longer is a chord's key, `ctrl+u` or `esc`.
fn parse_vim_key(spec: &str) -> Result<KeySpec, String> {
    let mut chars = spec.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => Ok(KeySpec {
            modifiers: KeyModifiers::NONE,
            code: KeyCode::Char(ch),
        }),
        _ => parse_key(spec),
    }
}
```

- [ ] **Step 7: The `Keymap` literal in the existing test**

In the test `active_context_ambiguity_is_reported`, the literal `let keymap = Keymap { name: "broken".into(), bindings: vec![ ... ], };` gains `vim: Default::default(),` after `name: "broken".into(),`.

- [ ] **Step 8: Vim reads the remapped key**

In `crates/dexo-tui/src/screens/vim.rs::handle_key`, replace

```rust
    if model.vim.prompt.is_some() {
        return prompt_key(model, key);
    }
    if model.vim.mode == Mode::Insert {
```

with

```rust
    if model.vim.prompt.is_some() {
        return prompt_key(model, key);
    }
    // The key as keymap.toml's `[vim]` has Vim read it: `m` is `h` on a Colemak layout.
    // Insert mode types the key as pressed; only the Esc that leaves it is read here.
    // `.` replays the keys as pressed, through this same translation.
    let pressed = key;
    let key = model.keymap.vim_key(key);
    if model.vim.mode == Mode::Insert {
```

and, lower in the same function, replace

```rust
        KeyCode::Char(ch) => {
            model.vim.pending.push(ch);
            model.vim.pending_keys.push(key);
            command(model)
        }
```

with

```rust
        KeyCode::Char(ch) => {
            model.vim.pending.push(ch);
            model.vim.pending_keys.push(pressed);
            command(model)
        }
```

Update the module doc at the top of `vim.rs`: after the sentence ending `In Insert mode it lets the keys through to the plain editor.` add ` A \`[vim]\` section in keymap.toml renames the keys it reads, for another layout.`

- [ ] **Step 9: The one test for the parser**

In `crates/dexo-tui/src/keymap.rs`'s `mod tests`, after `an_unreadable_overlay_says_so`, add:

```rust
    /// `[vim]` remaps the keys Vim reads: a character as written, a chord's key, `""` to
    /// undo one the base made; the built-in profiles have none; a remapped key that
    /// [global] or [editor] binds is refused, one that [results] binds is not.
    #[test]
    fn a_vim_section_remaps_the_keys_vim_reads() {
        assert!(Keymap::default_profile().vim.is_empty());
        assert!(Keymap::vim_profile().vim.is_empty());
        let vim = Keymap::vim_profile();
        let merged = super::merge_overlay(
            &vim,
            "[vim]\n\"m\" = \"h\"\n\"N\" = \"J\"\n\"ctrl+u\" = \"ctrl+r\"\n",
        )
        .unwrap();
        let read = |code, modifiers| merged.vim_key(KeyEvent::new(code, modifiers));
        assert_eq!(
            read(KeyCode::Char('m'), KeyModifiers::NONE).code,
            KeyCode::Char('h')
        );
        assert_eq!(
            read(KeyCode::Char('N'), KeyModifiers::SHIFT).code,
            KeyCode::Char('J')
        );
        let redo = read(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(
            (redo.code, redo.modifiers),
            (KeyCode::Char('r'), KeyModifiers::CONTROL)
        );
        assert_eq!(
            read(KeyCode::Char('e'), KeyModifiers::NONE).code,
            KeyCode::Char('e')
        );
        // `""` undoes a remap the base made.
        let base = parse_keymap("profile = \"vim\"\n[vim]\n\"e\" = \"k\"\n").unwrap();
        assert_eq!(base.vim.len(), 1);
        let undone = super::merge_overlay(&base, "[vim]\n\"e\" = \"\"\n").unwrap();
        assert!(undone.vim.is_empty());
        let error = super::merge_overlay(
            &vim,
            "[global]\n\"e\" = \"focus.explorer\"\n[vim]\n\"e\" = \"k\"\n",
        )
        .unwrap_err();
        assert_eq!((error.line, error.field.as_str()), (Some(4), "[vim] e"));
        assert!(error.reason.contains("focus.explorer"), "{error}");
        assert!(
            super::merge_overlay(&vim, "[results]\n\"k\" = \"results.up\"\n[vim]\n\"k\" = \"n\"\n")
                .is_ok()
        );
        let error = super::merge_overlay(&vim, "[vim]\n\"m\" = 3\n").unwrap_err();
        assert_eq!(error.line, Some(2));
    }
```

- [ ] **Step 10: Build, lint, run**

Run:
```bash
cargo fmt --all
cargo clippy -p dexo-tui --all-features -- -D warnings
cargo test -p dexo-tui keymap
cargo test -p dexo-tui vim
```
Expected: all pass, the new test included.

- [ ] **Step 11: Commit**

```bash
git add crates/dexo-tui/src/keymap.rs crates/dexo-tui/src/screens/vim.rs
git commit -m "feat(tui): [vim] in keymap.toml remaps the keys Vim reads, for another layout

Each line is the key pressed and the key Vim reads for it -- a character as written
(\"N\" = \"J\") or a chord's key (\"ctrl+u\" = \"ctrl+r\") -- and \"\" undoes one. The
translation runs before Vim parses, so operators, counts, text objects and . follow:
with \"m\" = \"h\", dm deletes to the left. A remapped key that [global] or [editor] binds
is refused, since the keymap reads it first and Vim would never see it.

Claude-Session: https://claude.ai/code/session_014NYSWisD33UHaLTrJZYUtc"
```

---

### Task 3: The editor's motions are commands; the Emacs profile binds its own

**Files:**
- Modify: `crates/dexo-tui/src/action.rs:11` (add `EditorMotion` before `pub enum Action`), `:583` (three variants after `EditorRedo,`)
- Modify: `crates/dexo-tui/src/palette/registry.rs:1570-1577` (fifteen specs after `editor.select_all`), `:1640-1676` (`hidden`)
- Modify: `crates/dexo-tui/src/screens/editor.rs:1064-1073` (drop the Emacs call), `:1368` (`delete` visibility), `:1697-1750` (`kill_line` visibility, `emacs_key` removed, `motion` added)
- Modify: `crates/dexo-tui/src/update.rs:2370-2374` (three handlers after `Action::EditorRedo`)
- Modify: `crates/dexo-tui/src/keymap.rs` `EMACS_TOML` `[editor]`
- Modify: `crates/dexo-tui/tests/command_palette_contract.rs:112-116`, `:265-266`

**Interfaces:**
- Consumes: `screens::editor::{move_chars, move_vertical, move_line_edge, page, move_completion, delete, kill_line, refresh_intelligence}` with the signatures `move_chars(model, delta: i32, shift: bool, word: bool)`, `move_vertical(model, delta: i32, shift: bool)`, `move_line_edge(model, home: bool, shift: bool, document: bool)`, `page(model, direction: i32, shift: bool)`, `move_completion(model, delta: i32)`, `delete(model, word: bool)`, `kill_line(model)`.
- Produces: `pub enum EditorMotion` in `action.rs`; `Action::EditorMove(EditorMotion)`, `Action::EditorDeleteForward { word: bool }`, `Action::EditorKillLine`; `pub(crate) fn motion(model: &mut Model, motion: EditorMotion)` in `screens/editor.rs`; command ids `editor.left`, `editor.right`, `editor.up`, `editor.down`, `editor.word_left`, `editor.word_right`, `editor.line_start`, `editor.line_end`, `editor.document_start`, `editor.document_end`, `editor.page_up`, `editor.page_down`, `editor.delete_forward`, `editor.delete_word_forward`, `editor.kill_line`.

- [ ] **Step 1: The actions**

In `crates/dexo-tui/src/action.rs`, directly above `#[derive(Clone, Debug, PartialEq)]\npub enum Action {`, add:

```rust
/// Where `Action::EditorMove` takes the cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditorMotion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    PageUp,
    PageDown,
}

```

In `pub enum Action`, directly after the line `    EditorRedo,`, add:

```rust
    /// A motion of the plain editor, for a keymap that moves without the arrows: the
    /// Emacs profile's Ctrl+N, Ctrl+P and the rest.
    EditorMove(EditorMotion),
    /// The character after the cursor, or with `word` the word.
    EditorDeleteForward {
        word: bool,
    },
    /// Emacs's Ctrl+K: to the end of the line, or at the end the line break.
    EditorKillLine,
```

- [ ] **Step 2: The editor runs them**

In `crates/dexo-tui/src/screens/editor.rs`:

(a) In `pub fn handle_key`, delete the three lines

```rust
    if model.keymap.name == "emacs" && emacs_key(model, key) {
        return true;
    }
```

(b) Change `fn delete(model: &mut Model, word: bool) {` to `pub(crate) fn delete(model: &mut Model, word: bool) {`.

(c) Replace the `kill_line` doc comment and signature

```rust
/// Ctrl+K in the Emacs profile: from the cursor to the end of the line, and at the end
/// of a line the newline after it, so the next line comes up.
fn kill_line(model: &mut Model) {
```

with

```rust
/// Kill Line, Emacs's Ctrl+K: from the cursor to the end of the line, and at the end of
/// a line the newline after it, so the next line comes up.
pub(crate) fn kill_line(model: &mut Model) {
```

(d) Replace the whole of `emacs_key` -- from its doc comment `/// The Emacs profile's motion and kill keys. ...` through its closing `}` (the lines `fn emacs_key(model: &mut Model, key: KeyEvent) -> bool { ... true }`) -- with:

```rust
/// A motion a keymap binds -- the Emacs profile's Ctrl+N, Ctrl+P and the rest -- as the
/// arrow keys make it. With the completion popup open, Up and Down walk the popup, as
/// the arrows do. The keymap reads these keys first, so they never type.
pub(crate) fn motion(model: &mut Model, motion: crate::action::EditorMotion) {
    use crate::action::EditorMotion;
    let popup = model.editor.completion_open;
    match motion {
        EditorMotion::Left => move_chars(model, -1, false, false),
        EditorMotion::Right => move_chars(model, 1, false, false),
        EditorMotion::WordLeft => move_chars(model, -1, false, true),
        EditorMotion::WordRight => move_chars(model, 1, false, true),
        EditorMotion::Up if popup => move_completion(model, -1),
        EditorMotion::Down if popup => move_completion(model, 1),
        EditorMotion::Up => move_vertical(model, -1, false),
        EditorMotion::Down => move_vertical(model, 1, false),
        EditorMotion::LineStart => move_line_edge(model, true, false, false),
        EditorMotion::LineEnd => move_line_edge(model, false, false, false),
        EditorMotion::DocumentStart => move_line_edge(model, true, false, true),
        EditorMotion::DocumentEnd => move_line_edge(model, false, false, true),
        EditorMotion::PageUp => page(model, -1, false),
        EditorMotion::PageDown => page(model, 1, false),
    }
}
```

- [ ] **Step 3: The handlers**

In `crates/dexo-tui/src/update.rs`, directly after

```rust
        Action::EditorRedo => {
            crate::screens::editor::redo(model);
            crate::screens::editor::refresh_intelligence(model, false);
            Vec::new()
        }
```

add

```rust
        Action::EditorMove(motion) => {
            crate::screens::editor::motion(model, motion);
            crate::screens::vim::after_pass(model);
            Vec::new()
        }
        Action::EditorDeleteForward { word } => {
            crate::screens::editor::delete(model, word);
            crate::screens::editor::refresh_intelligence(model, false);
            Vec::new()
        }
        Action::EditorKillLine => {
            crate::screens::editor::kill_line(model);
            crate::screens::editor::refresh_intelligence(model, false);
            Vec::new()
        }
```

(`update()` already calls `follow_cursor` after every action, so the view follows.)

- [ ] **Step 4: The commands**

In `crates/dexo-tui/src/palette/registry.rs`, directly after the spec

```rust
        CommandSpec {
            id: "editor.select_all",
            title: "Select All",
            keywords: &["selection", "everything"],
            shortcut: Some("Ctrl+A"),
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorSelectAll),
        },
```

add (the `use crate::action::{Action, FocusTarget};` at the top becomes `use crate::action::{Action, EditorMotion, FocusTarget};`):

```rust
        CommandSpec {
            id: "editor.left",
            title: "Cursor Left",
            keywords: &["move", "character"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::Left)),
        },
        CommandSpec {
            id: "editor.right",
            title: "Cursor Right",
            keywords: &["move", "character"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::Right)),
        },
        CommandSpec {
            id: "editor.up",
            title: "Cursor Up",
            keywords: &["move", "line"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::Up)),
        },
        CommandSpec {
            id: "editor.down",
            title: "Cursor Down",
            keywords: &["move", "line"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::Down)),
        },
        CommandSpec {
            id: "editor.word_left",
            title: "Word Left",
            keywords: &["move", "word"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::WordLeft)),
        },
        CommandSpec {
            id: "editor.word_right",
            title: "Word Right",
            keywords: &["move", "word"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::WordRight)),
        },
        CommandSpec {
            id: "editor.line_start",
            title: "Line Start",
            keywords: &["move", "home"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::LineStart)),
        },
        CommandSpec {
            id: "editor.line_end",
            title: "Line End",
            keywords: &["move", "end"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::LineEnd)),
        },
        CommandSpec {
            id: "editor.document_start",
            title: "Document Start",
            keywords: &["move", "top"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(
                EditorMotion::DocumentStart,
            )),
        },
        CommandSpec {
            id: "editor.document_end",
            title: "Document End",
            keywords: &["move", "bottom"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::DocumentEnd)),
        },
        CommandSpec {
            id: "editor.page_up",
            title: "Page Up",
            keywords: &["move", "scroll"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::PageUp)),
        },
        CommandSpec {
            id: "editor.page_down",
            title: "Page Down",
            keywords: &["move", "scroll"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorMove(EditorMotion::PageDown)),
        },
        CommandSpec {
            id: "editor.delete_forward",
            title: "Delete Character",
            keywords: &["delete", "forward"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorDeleteForward { word: false }),
        },
        CommandSpec {
            id: "editor.delete_word_forward",
            title: "Delete Word",
            keywords: &["delete", "word"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorDeleteForward { word: true }),
        },
        CommandSpec {
            id: "editor.kill_line",
            title: "Kill Line",
            keywords: &["delete", "line", "emacs"],
            shortcut: None,
            requirements: &[],
            invocation: PaletteInvocation::Dispatch(Action::EditorKillLine),
        },
```

In `fn hidden(id: &str) -> bool`, after the line `            | "explorer.open"` and before the comment `// the document strip's own cursor`, add:

```rust
            | "editor.left"
            | "editor.right"
            | "editor.up"
            | "editor.down"
            | "editor.word_left"
            | "editor.word_right"
            | "editor.line_start"
            | "editor.line_end"
            | "editor.document_start"
            | "editor.document_end"
            | "editor.page_up"
            | "editor.page_down"
            | "editor.delete_forward"
            | "editor.delete_word_forward"
            | "editor.kill_line"
```

- [ ] **Step 5: The Emacs profile binds them**

In `crates/dexo-tui/src/keymap.rs`, in `EMACS_TOML`'s `[editor]` section, directly after `"alt+w" = "editor.copy"` and before `"alt+up" = "layout.results_grow"`, add:

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

- [ ] **Step 6: The contract**

In `crates/dexo-tui/tests/command_palette_contract.rs`, in `COMMAND_IDS`, directly after `    "editor.paste",` add:

```rust
    "editor.left",
    "editor.right",
    "editor.up",
    "editor.down",
    "editor.word_left",
    "editor.word_right",
    "editor.line_start",
    "editor.line_end",
    "editor.document_start",
    "editor.document_end",
    "editor.page_up",
    "editor.page_down",
    "editor.delete_forward",
    "editor.delete_word_forward",
    "editor.kill_line",
```

In `registry_contains_each_command_exactly_once`, change both `191` to `206`. `palette_shows_only_the_curated_subset` keeps `163`: the fifteen are hidden.

- [ ] **Step 7: Build, lint, run**

Run:
```bash
cargo fmt --all
cargo clippy -p dexo-tui --all-features -- -D warnings
cargo test -p dexo-tui --test command_palette_contract
cargo test -p dexo-tui --test editor_emacs_keys
cargo test -p dexo-tui keymap
cargo test -p dexo-tui status
```
Expected: all pass. `editor_emacs_keys` exercises every Emacs motion through the keymap; `keymap::builtin_profiles_parse` proves the new chords conflict with nothing; `keymap::every_bound_command_is_registered` proves the ids exist.

- [ ] **Step 8: Commit**

```bash
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/palette/registry.rs crates/dexo-tui/src/screens/editor.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/keymap.rs crates/dexo-tui/tests/command_palette_contract.rs
git commit -m "feat(tui): the editor's motions are commands any keymap binds; the Emacs profile binds its own

editor.left, right, up, down, word_left, word_right, line_start, line_end,
document_start, document_end, page_up, page_down, delete_forward, delete_word_forward
and kill_line. The Emacs profile's Ctrl+A, Ctrl+E, Ctrl+N, Alt+F and the rest were read
in the editor after the keymap, where no profile could change them; they are its [editor]
bindings now.

Claude-Session: https://claude.ai/code/session_014NYSWisD33UHaLTrJZYUtc"
```

---

### Task 4: Docs

**Files:**
- Modify: `docs/src/workbench.md:42-56` (Keymap overlay), `:58-68` (SQL editor table), `:74-76` (Vim mode)

- [ ] **Step 1: Keymap overlay**

In `docs/src/workbench.md`, directly after the paragraph that ends `The palette shows the keys the overlay gives.`, add:

```markdown
While the editor takes text -- always with the Default and Emacs keymaps, in Insert mode with Vim -- a key that types (a character, Enter, Tab, Backspace or Delete, with at most Shift) is typed, whatever the overlay binds it to: `[global] "r" = "focus.results"` works in Vim's Normal mode and types an `r` in Insert mode. A chord of bare keys works the same way: `[editor] "space f" = "document.open"` waits for the `f` in Normal mode, listing what can follow, and `space` is a space in Insert mode.

With the Vim keymap, a `[vim]` section changes the keys Vim reads, for another layout: each line is the key pressed and the key Vim reads for it, a character as written (`"N" = "J"`) or a chord's key (`"ctrl+u" = "ctrl+r"`), and `""` undoes a remap. Operators, counts, text objects and `.` follow -- with the lines below, `dm` deletes to the left and `3n` goes down three lines. A remapped key that `[global]` or `[editor]` also binds, alone or as the start of a chord, is refused: the keymap would read it first, and Vim never see it.

```toml
[vim]
"m" = "h"
"n" = "j"
"e" = "k"
"i" = "l"
"j" = "i"
```
```

- [ ] **Step 2: The editor's motions**

Directly after the SQL editor table (after the row `| Ctrl+E | Edit the document in ... (ctrl+x ctrl+e in the Emacs keymap) |`) and before the paragraph `The editor underlines what is wrong as you type`, add:

```markdown
The Emacs keymap moves with Ctrl+A, Ctrl+E, Ctrl+F, Ctrl+B, Ctrl+N, Ctrl+P, Alt+F, Alt+B, Alt+<, Alt+>, Ctrl+V and Alt+V, deletes forward with Ctrl+D and Alt+D, and kills the line with Ctrl+K. These are the commands `editor.line_start`, `editor.line_end`, `editor.right`, `editor.left`, `editor.down`, `editor.up`, `editor.word_right`, `editor.word_left`, `editor.document_start`, `editor.document_end`, `editor.page_down`, `editor.page_up`, `editor.delete_forward`, `editor.delete_word_forward` and `editor.kill_line`, which any keymap binds in `[editor]`.
```

- [ ] **Step 3: Vim mode**

At the end of the "Vim mode" paragraph (after `Esc then a key typed fast is two keys, as in Vim, not an Alt chord.`), append: ` Another layout changes the keys Vim reads with a `[vim]` section in `keymap.toml` (see Keymap overlay).`

- [ ] **Step 4: Check the book builds, if mdbook is installed**

Run: `command -v mdbook >/dev/null && mdbook build docs -d /tmp/claude-1000/-home-winx-Documents-github-Dexo/88f3b8cd-969c-40a8-ab43-bc54d8a08806/scratchpad/book || echo "no mdbook; skipped"`
Expected: a build, or the skip line.

- [ ] **Step 5: Commit**

```bash
git add docs/src/workbench.md
git commit -m "docs(workbench): [vim] remaps, typing before the keymap, the editor's motions as commands

Claude-Session: https://claude.ai/code/session_014NYSWisD33UHaLTrJZYUtc"
```

---

### Task 5: Full validation and the issue reply

**Files:**
- Create: `/tmp/claude-1000/-home-winx-Documents-github-Dexo/88f3b8cd-969c-40a8-ab43-bc54d8a08806/scratchpad/issue-13-reply.md` (not committed)

- [ ] **Step 1: The whole workspace**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features -- -D warnings
cargo test -p dexo-tui
```
Expected: clean, all green.

- [ ] **Step 2: Write the reply for issue #13**

Write the file with this content (the user posts it):

```markdown
Thanks for the detailed report -- all three things were real, and they're on `development` now.

**Insert mode wins.** While the editor takes text (Insert mode with Vim; always with the Default and Emacs keymaps), a key that types -- a character, Enter, Tab, Backspace, Delete, with at most Shift -- is typed, whatever `keymap.toml` binds it to. So your `[global]` lines now do exactly what you expected: `r`, `q`, `e` switch panes in Normal mode and type in Insert mode.

**Vim's keys are remappable.** Changing `i` in `vim.rs` didn't work because `i` is read in four places (insert, the `iw` text object, Visual's `i`...). Instead of per-key commands there's a `[vim]` section that renames the key *before* Vim parses it -- Vim's own `noremap` idea -- so operators, counts, text objects and `.` all follow. Colemak-DH:

```toml
[vim]
"m" = "h"
"n" = "j"
"e" = "k"
"i" = "l"
"j" = "i"
"k" = "n"
"l" = "e"
"N" = "J"
```

A character is taken as written (`"N" = "J"`), longer keys use the chord syntax (`"ctrl+u" = "ctrl+r"`), and `""` undoes a remap. If a remapped key is also bound in `[global]` or `[editor]` the file is refused with the line and both sides, since the binding would win.

**Editor navigation is bindable in every profile.** The Emacs motions used to be hardcoded; they're commands now -- `editor.left`, `editor.right`, `editor.up`, `editor.down`, `editor.word_left`, `editor.word_right`, `editor.line_start`, `editor.line_end`, `editor.document_start`, `editor.document_end`, `editor.page_up`, `editor.page_down`, `editor.delete_forward`, `editor.delete_word_forward`, `editor.kill_line` -- so `[editor] "ctrl+n" = "editor.down"` works in any keymap.

**Minimodes.** Single-key menus already work as chords: `[editor] "space f" = "document.open"` waits for the `f` in Normal mode, shows what can follow while it waits, and `space` is a space in Insert mode. A full Helix mode (select-first) is a different editor and isn't planned, but with Insert fixed and `[vim]` in place you can get most of the way with chords.

Docs: the "Keymap overlay" section of the workbench page.
```

- [ ] **Step 3: Report**

Tell the user: the commits (four), the test kept in Task 2 and why, and the path of the reply.
