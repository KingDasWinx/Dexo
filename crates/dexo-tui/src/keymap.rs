use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyContext {
    Global,
    Editor,
    Explorer,
    Results,
    /// The document tab strip. Its keys used to be special cases in `handle_key`, which
    /// is how Enter on `+` came to work from the editor and nowhere else.
    DocumentTabs,
    /// The pane the grid gives up to the console on a table document. It holds no
    /// navigation of its own -- only the keys that act on the pane itself.
    Console,
    Palette,
    Modal,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeySpec {
    pub modifiers: KeyModifiers,
    pub code: KeyCode,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Chord {
    pub keys: Vec<KeySpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    pub chord: Chord,
    pub command: String,
    pub context: KeyContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeymapConflict {
    pub chord: String,
    pub context: KeyContext,
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeymapError {
    pub field: String,
    pub reason: String,
}

impl std::fmt::Display for KeymapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    pub name: String,
    pub bindings: Vec<Binding>,
}

/// Selectable keymaps, as `(profile, label)`.
pub const PROFILES: &[(&str, &str)] = &[("default", "Default"), ("vim", "Vim"), ("emacs", "Emacs")];

pub fn profile_index(name: &str) -> usize {
    PROFILES
        .iter()
        .position(|(key, _)| *key == name)
        .unwrap_or(0)
}

/// Wraps in both directions, so a picker can step back as easily as forward.
pub fn step_profile(name: &str, delta: i32) -> &'static str {
    let len = PROFILES.len() as i32;
    PROFILES[((profile_index(name) as i32 + delta).rem_euclid(len)) as usize].0
}

impl Keymap {
    pub fn named(name: &str) -> Self {
        match name {
            "vim" => Self::vim_profile(),
            "emacs" => Self::emacs_profile(),
            _ => Self::default_profile(),
        }
    }

    pub fn default_profile() -> Self {
        parse_keymap(DEFAULT_TOML).expect("builtin default keymap")
    }

    pub fn vim_profile() -> Self {
        parse_keymap(VIM_TOML).expect("builtin vim keymap")
    }

    pub fn emacs_profile() -> Self {
        parse_keymap(EMACS_TOML).expect("builtin emacs keymap")
    }

    pub fn conflicts(&self) -> Vec<KeymapConflict> {
        let mut grouped: HashMap<(KeyContext, String), Vec<String>> = HashMap::new();
        for binding in &self.bindings {
            grouped
                .entry((binding.context, chord_label(&binding.chord)))
                .or_default()
                .push(binding.command.clone());
        }
        grouped
            .into_iter()
            .filter(|(_, commands)| commands.iter().any(|command| command != &commands[0]))
            .map(|((context, chord), mut commands)| {
                commands.sort();
                commands.dedup();
                KeymapConflict {
                    chord,
                    context,
                    commands,
                }
            })
            .collect()
    }

    pub fn resolve(
        &self,
        chord: &Chord,
        active: KeyContext,
    ) -> Result<Option<&str>, KeymapConflict> {
        let label = chord_label(chord);
        let mut matches = self
            .bindings
            .iter()
            .filter(|binding| {
                chord_label(&binding.chord) == label
                    && (binding.context == KeyContext::Global || binding.context == active)
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Ok(None);
        }
        matches.sort_by_key(|binding| match binding.context {
            KeyContext::Global => 1,
            _ => 0,
        });
        let specific: Vec<_> = matches
            .iter()
            .filter(|binding| binding.context == active)
            .copied()
            .collect();
        let pool = if specific.is_empty() {
            matches
        } else {
            specific
        };
        let command = pool[0].command.as_str();
        if pool.iter().any(|binding| binding.command != command) {
            return Err(KeymapConflict {
                chord: label,
                context: active,
                commands: {
                    let mut commands: Vec<_> = pool.iter().map(|b| b.command.clone()).collect();
                    commands.sort();
                    commands.dedup();
                    commands
                },
            });
        }
        Ok(Some(command))
    }

    pub fn command_ids(&self) -> Vec<&str> {
        self.bindings.iter().map(|b| b.command.as_str()).collect()
    }

    pub fn is_prefix(&self, chord: &Chord, active: KeyContext) -> bool {
        self.bindings.iter().any(|binding| {
            (binding.context == KeyContext::Global || binding.context == active)
                && binding.chord.keys.starts_with(&chord.keys)
                && binding.chord.keys.len() > chord.keys.len()
        })
    }

    pub fn help_sections(&self) -> Vec<(&'static str, Vec<(String, String)>)> {
        let mut buckets: [(KeyContext, Vec<(String, String)>); 6] = [
            (KeyContext::Editor, Vec::new()),
            (KeyContext::Results, Vec::new()),
            (KeyContext::Explorer, Vec::new()),
            (KeyContext::DocumentTabs, Vec::new()),
            (KeyContext::Global, Vec::new()),
            (KeyContext::Palette, Vec::new()),
        ];
        for binding in &self.bindings {
            let chord = chord_label(&binding.chord);
            let entry = (chord, binding.command.clone());
            match binding.context {
                KeyContext::Editor => buckets[0].1.push(entry),
                // the console is the results pane wearing a different hat
                KeyContext::Results | KeyContext::Console => buckets[1].1.push(entry),
                KeyContext::Explorer => buckets[2].1.push(entry),
                KeyContext::DocumentTabs => buckets[3].1.push(entry),
                KeyContext::Global => buckets[4].1.push(entry),
                KeyContext::Palette | KeyContext::Modal => buckets[5].1.push(entry),
            }
        }
        let names = [
            "Editor",
            "Results",
            "Explorer",
            "Tabs",
            "Workbench",
            "Overlays",
        ];
        buckets
            .into_iter()
            .zip(names)
            .filter_map(|((_, mut rows), name)| {
                if rows.is_empty() {
                    return None;
                }
                rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
                rows.dedup();
                Some((name, rows))
            })
            .collect()
    }
}

pub fn parse_keymap(src: &str) -> Result<Keymap, KeymapError> {
    let table: toml::Table = src.parse().map_err(|err: toml::de::Error| KeymapError {
        field: "keymap".into(),
        reason: err.message().to_string(),
    })?;
    let name = table
        .get("profile")
        .and_then(|v| v.as_str())
        .unwrap_or("custom")
        .to_string();
    let mut bindings = Vec::new();
    for (key, value) in &table {
        if key == "profile" {
            continue;
        }
        let context = parse_context(key)?;
        let Some(map) = value.as_table() else {
            return Err(KeymapError {
                field: key.clone(),
                reason: "context must be a table of chord = command".into(),
            });
        };
        for (chord, command) in map {
            let command = command.as_str().ok_or_else(|| KeymapError {
                field: format!("{key}.{chord}"),
                reason: "command id must be a string".into(),
            })?;
            bindings.push(Binding {
                chord: parse_chord(chord).map_err(|reason| KeymapError {
                    field: format!("{key}.{chord}"),
                    reason,
                })?,
                command: command.to_string(),
                context,
            });
        }
    }
    let keymap = Keymap { name, bindings };
    let conflicts = keymap.conflicts();
    if let Some(conflict) = conflicts.into_iter().next() {
        return Err(KeymapError {
            field: format!("{:?}.{}", conflict.context, conflict.chord),
            reason: format!("ambiguous commands {}", conflict.commands.join(" / ")),
        });
    }
    Ok(keymap)
}

pub fn parse_chord(spec: &str) -> Result<Chord, String> {
    let keys = spec
        .split_whitespace()
        .map(parse_key)
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        return Err("empty chord".into());
    }
    Ok(Chord { keys })
}

pub fn chord_from_event(event: KeyEvent) -> Chord {
    Chord {
        keys: vec![KeySpec {
            modifiers: event.modifiers,
            code: event.code,
        }],
    }
}

pub fn parse_key(spec: &str) -> Result<KeySpec, String> {
    let mut modifiers = KeyModifiers::NONE;
    let mut token = spec.trim().to_ascii_lowercase();
    loop {
        if let Some(rest) = token.strip_prefix("ctrl+") {
            modifiers |= KeyModifiers::CONTROL;
            token = rest.to_string();
            continue;
        }
        if let Some(rest) = token.strip_prefix("alt+") {
            modifiers |= KeyModifiers::ALT;
            token = rest.to_string();
            continue;
        }
        if let Some(rest) = token.strip_prefix("shift+") {
            modifiers |= KeyModifiers::SHIFT;
            token = rest.to_string();
            continue;
        }
        break;
    }
    let code = match token.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        // `f1`..`f12`; a bare `f` is the letter.
        other if other.starts_with('f') && (2..=3).contains(&other.len()) => {
            let n: u8 = other[1..]
                .parse()
                .map_err(|_| format!("unknown key `{spec}`"))?;
            KeyCode::F(n)
        }
        other if other.chars().count() == 1 => KeyCode::Char(other.chars().next().unwrap()),
        _ => return Err(format!("unknown key `{spec}`")),
    };
    Ok(KeySpec { modifiers, code })
}

fn parse_context(name: &str) -> Result<KeyContext, KeymapError> {
    match name {
        "global" => Ok(KeyContext::Global),
        "editor" => Ok(KeyContext::Editor),
        "explorer" => Ok(KeyContext::Explorer),
        "results" => Ok(KeyContext::Results),
        "console" => Ok(KeyContext::Console),
        "tabs" => Ok(KeyContext::DocumentTabs),
        "palette" => Ok(KeyContext::Palette),
        "modal" => Ok(KeyContext::Modal),
        other => Err(KeymapError {
            field: other.into(),
            reason: format!("unknown keymap context `{other}`"),
        }),
    }
}

pub fn chord_label(chord: &Chord) -> String {
    chord
        .keys
        .iter()
        .map(key_label)
        .collect::<Vec<_>>()
        .join(" ")
}

fn key_label(key: &KeySpec) -> String {
    let mut out = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        out.push_str("ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        out.push_str("alt+");
    }
    // A terminal without the extended keyboard protocol sends Alt+Shift+F as Alt and a
    // capital F, with no Shift of its own: the capital is the Shift.
    let shifted = key.modifiers.contains(KeyModifiers::SHIFT)
        || matches!(key.code, KeyCode::Char(c) if c.is_ascii_uppercase());
    if shifted && !matches!(key.code, KeyCode::Char(c) if !c.is_ascii_alphabetic()) {
        out.push_str("shift+");
    }
    out.push_str(&match key.code {
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(ch) => ch.to_ascii_lowercase().to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Esc => "esc".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        other => format!("{other:?}").to_ascii_lowercase(),
    });
    out
}

const DEFAULT_TOML: &str = r#"
profile = "default"
[global]
"ctrl+p" = "palette.open"
"ctrl+q" = "workbench.quit"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"ctrl+s" = "document.save"
"ctrl+o" = "document.open"
"alt+1" = "focus.explorer"
"alt+2" = "focus.editor"
"alt+3" = "focus.results"
"alt+0" = "focus.tabs"
"ctrl+w" = "document.close"
"ctrl+tab" = "document.next"
"ctrl+shift+tab" = "document.prev"
"f2" = "document.rename"
"ctrl+n" = "document.new"
"alt+e" = "layout.hide_explorer"
"alt+r" = "layout.hide_results"
"alt+left" = "document.prev_focus"
"alt+right" = "document.next_focus"
"alt+-" = "layout.results_shrink"
"alt+=" = "layout.results_grow"
"alt+[" = "layout.explorer_shrink"
"alt+]" = "layout.explorer_grow"
[explorer]
"enter" = "explorer.expand"
"n" = "connection.new"
"e" = "connection.edit"
"d" = "explorer.ddl"
"shift+d" = "connection.close_session"
"c" = "explorer.copy_name"
"a" = "explorer.actions"
"o" = "explorer.data"
"r" = "explorer.refresh"
"i" = "explorer.inspect"
"up" = "explorer.up"
"down" = "explorer.down"
"?" = "help.open"
"alt+=" = "layout.explorer_grow"
"alt+-" = "layout.explorer_shrink"
"alt++" = "layout.explorer_grow"
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[editor]
"ctrl+enter" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"alt+shift+f" = "editor.format"
"ctrl+shift+i" = "editor.format"
"ctrl+z" = "editor.undo"
"ctrl+y" = "editor.redo"
"ctrl+a" = "editor.select_all"
"ctrl+v" = "editor.paste"
"ctrl+c" = "editor.copy"
"ctrl+x" = "editor.cut"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"
[results]
"up" = "results.up"
"down" = "results.down"
"left" = "results.left"
"right" = "results.right"
"pageup" = "results.pageup"
"pagedown" = "results.pagedown"
"shift+up" = "results.extend_up"
"shift+down" = "results.extend_down"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"r" = "results.select_row"
"delete" = "data.toggle_delete"
"i" = "data.insert_row"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"c" = "results.select_column"
"[" = "results.prev_tab"
"]" = "results.next_tab"
"n" = "data.page_next"
"p" = "data.page_prev"
"b" = "data.nav_back"
"?" = "help.open"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[console]
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[tabs]
"left" = "document.tab_prev"
"right" = "document.tab_next"
"enter" = "document.activate_tab"
"ctrl+w" = "document.close"
"esc" = "focus.editor"
"?" = "help.open"
"#;

const VIM_TOML: &str = r#"
profile = "vim"
[global]
"ctrl+p" = "palette.open"
"ctrl+q" = "workbench.quit"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"alt+1" = "focus.explorer"
"alt+2" = "focus.editor"
"alt+3" = "focus.results"
"alt+0" = "focus.tabs"
"ctrl+w" = "document.close"
"ctrl+tab" = "document.next"
"ctrl+shift+tab" = "document.prev"
"f2" = "document.rename"
"ctrl+n" = "document.new"
"alt+e" = "layout.hide_explorer"
"alt+r" = "layout.hide_results"
"alt+left" = "document.prev_focus"
"alt+right" = "document.next_focus"
[editor]
"ctrl+enter" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"alt+shift+f" = "editor.format"
"ctrl+shift+i" = "editor.format"
"ctrl+z" = "editor.undo"
"ctrl+y" = "editor.redo"
"ctrl+a" = "editor.select_all"
"ctrl+v" = "editor.paste"
"ctrl+c" = "editor.copy"
"ctrl+x" = "editor.cut"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"
[explorer]
"enter" = "explorer.expand"
"n" = "connection.new"
"e" = "connection.edit"
"shift+d" = "connection.close_session"
"c" = "explorer.copy_name"
"a" = "explorer.actions"
"o" = "explorer.data"
"r" = "explorer.refresh"
"i" = "explorer.inspect"
"?" = "help.open"
"alt+=" = "layout.explorer_grow"
"alt+-" = "layout.explorer_shrink"
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[results]
"k" = "results.up"
"j" = "results.down"
"h" = "results.left"
"l" = "results.right"
"g g" = "results.top"
"delete" = "data.toggle_delete"
"i" = "data.insert_row"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"shift+k" = "results.extend_up"
"shift+j" = "results.extend_down"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"?" = "help.open"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[console]
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[tabs]
"left" = "document.tab_prev"
"right" = "document.tab_next"
"enter" = "document.activate_tab"
"ctrl+w" = "document.close"
"esc" = "focus.editor"
"?" = "help.open"
"#;

const EMACS_TOML: &str = r#"
profile = "emacs"
[global]
"alt+x" = "palette.open"
"ctrl+x ctrl+c" = "workbench.quit"
"ctrl+x ctrl+n" = "document.new"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"ctrl+c ctrl+c" = "query.execute_document"
"alt+1" = "focus.explorer"
"alt+2" = "focus.editor"
"alt+3" = "focus.results"
"alt+0" = "focus.tabs"
"ctrl+w" = "document.close"
"ctrl+tab" = "document.next"
"ctrl+shift+tab" = "document.prev"
"f2" = "document.rename"
"alt+e" = "layout.hide_explorer"
"alt+r" = "layout.hide_results"
"alt+left" = "document.prev_focus"
"alt+right" = "document.next_focus"
[editor]
"ctrl+enter" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"alt+shift+f" = "editor.format"
"ctrl+shift+i" = "editor.format"
"ctrl+z" = "editor.undo"
"ctrl+y" = "editor.redo"
"ctrl+a" = "editor.select_all"
"ctrl+v" = "editor.paste"
"alt+w" = "editor.copy"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"
[explorer]
"enter" = "explorer.expand"
"n" = "connection.new"
"e" = "connection.edit"
"shift+d" = "connection.close_session"
"c" = "explorer.copy_name"
"a" = "explorer.actions"
"o" = "explorer.data"
"r" = "explorer.refresh"
"i" = "explorer.inspect"
"?" = "help.open"
"alt+=" = "layout.explorer_grow"
"alt+-" = "layout.explorer_shrink"
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[results]
"ctrl+p" = "results.up"
"ctrl+n" = "results.down"
"delete" = "data.toggle_delete"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"left" = "results.left"
"right" = "results.right"
"shift+up" = "results.extend_up"
"shift+down" = "results.extend_down"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"?" = "help.open"
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[console]
"alt+up" = "layout.results_grow"
"alt+down" = "layout.results_shrink"

[tabs]
"left" = "document.tab_prev"
"right" = "document.tab_next"
"enter" = "document.activate_tab"
"ctrl+w" = "document.close"
"esc" = "focus.editor"
"?" = "help.open"
"#;

#[cfg(test)]
mod tests {
    use super::{KeyContext, Keymap, chord_from_event, parse_chord, parse_keymap};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn builtin_profiles_parse() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            assert!(keymap.conflicts().is_empty(), "{}", keymap.name);
        }
        assert!(
            Keymap::vim_profile()
                .resolve(&parse_chord("g g").unwrap(), KeyContext::Results)
                .unwrap()
                == Some("results.top")
        );
        assert!(
            Keymap::emacs_profile()
                .resolve(&parse_chord("ctrl+x ctrl+c").unwrap(), KeyContext::Editor)
                .unwrap()
                == Some("workbench.quit")
        );
        assert_eq!(
            Keymap::default_profile()
                .resolve(&parse_chord("f1").unwrap(), KeyContext::Editor)
                .unwrap(),
            Some("help.open")
        );
        assert_eq!(
            Keymap::default_profile()
                .resolve(&parse_chord("alt+3").unwrap(), KeyContext::Editor)
                .unwrap(),
            Some("focus.results")
        );
        let help = Keymap::default_profile().help_sections();
        assert!(help.iter().any(|(name, rows)| {
            *name == "Workbench"
                && rows
                    .iter()
                    .any(|(chord, cmd)| chord == "f1" && cmd == "help.open")
        }));
    }

    #[test]
    fn sql_execution_shortcuts_match_datagrip_in_every_profile() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            for (chord, command) in [
                ("ctrl+enter", "query.execute_statement"),
                ("ctrl+shift+f10", "query.execute_document"),
                ("ctrl+f2", "query.cancel"),
            ] {
                assert_eq!(
                    keymap
                        .resolve(&parse_chord(chord).unwrap(), KeyContext::Editor)
                        .unwrap(),
                    Some(command),
                    "profile {}",
                    keymap.name
                );
            }

            for chord in ["f5", "f8"] {
                assert_eq!(
                    keymap
                        .resolve(&parse_chord(chord).unwrap(), KeyContext::Editor)
                        .unwrap(),
                    None,
                    "legacy shortcut {chord} remains active in profile {}",
                    keymap.name
                );
            }
            // Ctrl+C copies now; what must never come back is Ctrl+C running a query.
            let ctrl_c = keymap
                .resolve(&parse_chord("ctrl+c").unwrap(), KeyContext::Editor)
                .unwrap();
            assert!(
                !ctrl_c.is_some_and(|command| command.starts_with("query.")),
                "ctrl+c runs {ctrl_c:?} in profile {}",
                keymap.name
            );
        }
    }

    /// Emacs keeps Ctrl+C as the prefix of Ctrl+C Ctrl+C, so it copies with its own
    /// Alt+W; the other profiles take the usual Ctrl+C and Ctrl+X.
    #[test]
    fn every_profile_can_copy_from_the_editor() {
        for (keymap, chord) in [
            (Keymap::default_profile(), "ctrl+c"),
            (Keymap::vim_profile(), "ctrl+c"),
            (Keymap::emacs_profile(), "alt+w"),
        ] {
            assert_eq!(
                keymap
                    .resolve(&parse_chord(chord).unwrap(), KeyContext::Editor)
                    .unwrap(),
                Some("editor.copy"),
                "profile {}",
                keymap.name
            );
        }
        assert_eq!(
            Keymap::default_profile()
                .resolve(&parse_chord("ctrl+x").unwrap(), KeyContext::Editor)
                .unwrap(),
            Some("editor.cut")
        );
    }

    #[test]
    fn side_pane_resize_shortcuts_exist_in_every_profile() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            for (context, chord, command) in [
                (KeyContext::Explorer, "alt+left", "layout.explorer_shrink"),
                (KeyContext::Explorer, "alt+right", "layout.explorer_grow"),
                (KeyContext::Results, "alt+up", "layout.results_grow"),
                (KeyContext::Results, "alt+down", "layout.results_shrink"),
                (KeyContext::Editor, "alt+up", "layout.results_grow"),
                (KeyContext::Editor, "alt+down", "layout.results_shrink"),
                // the console is that same pane on a table document
                (KeyContext::Console, "alt+up", "layout.results_grow"),
                (KeyContext::Console, "alt+down", "layout.results_shrink"),
            ] {
                assert_eq!(
                    keymap
                        .resolve(&parse_chord(chord).unwrap(), context)
                        .unwrap(),
                    Some(command),
                    "profile {} in {context:?}",
                    keymap.name
                );
            }
        }
    }

    #[test]
    fn hide_panel_shortcuts_exist_in_every_profile() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            for (chord, command) in [
                ("alt+e", "layout.hide_explorer"),
                ("alt+r", "layout.hide_results"),
            ] {
                assert_eq!(
                    keymap
                        .resolve(&parse_chord(chord).unwrap(), KeyContext::Editor)
                        .unwrap(),
                    Some(command),
                    "profile {}",
                    keymap.name
                );
            }
        }
    }

    #[test]
    fn sidebar_disconnect_uses_uppercase_d_without_replacing_ddl() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            assert_eq!(
                keymap
                    .resolve(&parse_chord("shift+d").unwrap(), KeyContext::Explorer)
                    .unwrap(),
                Some("connection.close_session"),
                "profile {}",
                keymap.name
            );
        }
        assert_eq!(
            Keymap::default_profile()
                .resolve(&parse_chord("d").unwrap(), KeyContext::Explorer)
                .unwrap(),
            Some("explorer.ddl")
        );
    }

    #[test]
    fn same_key_allowed_in_disjoint_contexts() {
        let keymap = parse_keymap(
            r#"
profile = "overlap"
[explorer]
"c" = "explorer.copy_name"
"a" = "explorer.actions"
[editor]
"c" = "query.execute_document"
"#,
        )
        .unwrap();
        assert_eq!(
            keymap
                .resolve(&parse_chord("c").unwrap(), KeyContext::Explorer)
                .unwrap(),
            Some("explorer.copy_name")
        );
        assert_eq!(
            keymap
                .resolve(&parse_chord("c").unwrap(), KeyContext::Editor)
                .unwrap(),
            Some("query.execute_document")
        );
    }

    #[test]
    fn same_context_conflict_is_exact() {
        let err = parse_keymap(
            r#"
[editor]
"ctrl+p" = "palette.open"
"Ctrl+P" = "query.execute_document"
"#,
        )
        .unwrap_err();
        assert!(err.field.contains("ctrl+p") || err.reason.contains("ambiguous"));
        assert!(
            err.reason.contains("palette.open") && err.reason.contains("query.execute_document")
                || err.field.contains("editor")
        );
    }

    #[test]
    fn active_context_ambiguity_is_reported() {
        let keymap = Keymap {
            name: "broken".into(),
            bindings: vec![
                super::Binding {
                    chord: parse_chord("x").unwrap(),
                    command: "query.execute_document".into(),
                    context: KeyContext::Editor,
                },
                super::Binding {
                    chord: parse_chord("x").unwrap(),
                    command: "workbench.quit".into(),
                    context: KeyContext::Editor,
                },
            ],
        };
        let err = keymap
            .resolve(&parse_chord("x").unwrap(), KeyContext::Editor)
            .unwrap_err();
        assert_eq!(err.chord, "x");
        assert!(err.commands.contains(&"query.execute_document".into()));
        assert!(err.commands.contains(&"workbench.quit".into()));
    }

    fn assert_registered(ids: impl IntoIterator<Item = impl AsRef<str>>) {
        let registered: std::collections::BTreeSet<_> = crate::palette::command_specs()
            .into_iter()
            .map(|spec| spec.id)
            .collect();
        for id in ids {
            let id = id.as_ref();
            assert!(registered.contains(id), "unregistered command: {id}");
        }
    }

    #[test]
    fn every_bound_command_is_registered() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            assert_registered(keymap.command_ids());
        }
    }

    #[test]
    fn n_opens_connection_form_only_in_the_explorer_context() {
        for keymap in [
            Keymap::default_profile(),
            Keymap::vim_profile(),
            Keymap::emacs_profile(),
        ] {
            let chord = parse_chord("n").unwrap();
            assert_eq!(
                keymap
                    .resolve(&chord, KeyContext::Explorer)
                    .expect("resolve"),
                Some("connection.new"),
                "profile {}",
                keymap.name
            );
            assert_eq!(
                keymap.resolve(&chord, KeyContext::Editor).expect("resolve"),
                None,
                "`n` must stay typable in the editor, profile {}",
                keymap.name
            );
        }
    }

    #[test]
    fn chord_from_single_key_event() {
        let chord = chord_from_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(super::chord_label(&chord), "ctrl+p");
    }
}
