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
    /// The 1-based line of the file it is about.
    pub line: Option<usize>,
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

/// One `chord = "command"` of a keymap file, with the line it is on.
struct Entry {
    section: String,
    spec: String,
    binding: Binding,
    line: usize,
}

/// A keymap file's `profile` name and its entries, in file order. An entry whose
/// command is `""` (an overlay's unbind) is kept, with that empty command.
fn entries(src: &str) -> Result<(Option<String>, Vec<Entry>), KeymapError> {
    let line_of = |offset: usize| crate::theme::line_at(src, offset);
    let table = toml::de::DeTable::parse(src).map_err(|error| KeymapError {
        field: "keymap".into(),
        reason: error.message().to_string(),
        line: error.span().map(|span| line_of(span.start)),
    })?;
    let mut profile = None;
    let mut entries = Vec::new();
    // The map is sorted by key; the file's order is what its lines are read in.
    let mut sections: Vec<_> = table.get_ref().iter().collect();
    sections.sort_by_key(|(key, _)| key.span().start);
    for (key, value) in sections {
        let section = key.get_ref().to_string();
        let line = line_of(key.span().start);
        if section == "profile" {
            profile = value.get_ref().as_str().map(str::to_string);
            continue;
        }
        let context = parse_context(&section).map_err(|error| KeymapError {
            line: Some(line),
            ..error
        })?;
        let toml::de::DeValue::Table(map) = value.get_ref() else {
            return Err(KeymapError {
                field: section,
                reason: "a section must be a table of chord = command".into(),
                line: Some(line),
            });
        };
        let mut chords: Vec<(Chord, String, String, usize)> = Vec::new();
        let mut map: Vec<_> = map.iter().collect();
        map.sort_by_key(|(spec, _)| spec.span().start);
        for (spec, command) in map {
            let line = line_of(spec.span().start);
            let spec = spec.get_ref().to_string();
            let field = format!("[{section}] {spec}");
            let command = command.get_ref().as_str().ok_or_else(|| KeymapError {
                field: field.clone(),
                reason: "a command id must be a string".into(),
                line: Some(line),
            })?;
            let chord = parse_chord(&spec).map_err(|reason| KeymapError {
                field: field.clone(),
                reason,
                line: Some(line),
            })?;
            // `"ctrl+p"` and `"Ctrl+P"` are two keys to TOML and one chord to Dexo.
            if let Some((_, first, bound, at)) = chords
                .iter()
                .find(|(seen, _, bound, _)| *seen == chord && bound != command)
            {
                return Err(KeymapError {
                    field,
                    reason: format!(
                        "is the chord `{first}` on line {at} binds to {bound}, and here to {command}; keep one"
                    ),
                    line: Some(line),
                });
            }
            chords.push((chord.clone(), spec.clone(), command.to_string(), line));
            entries.push(Entry {
                section: section.clone(),
                spec,
                binding: Binding {
                    chord,
                    command: command.to_string(),
                    context,
                },
                line,
            });
        }
    }
    Ok((profile, entries))
}

pub fn parse_keymap(src: &str) -> Result<Keymap, KeymapError> {
    let (profile, entries) = entries(src)?;
    Ok(Keymap {
        name: profile.unwrap_or_else(|| "custom".into()),
        bindings: entries.into_iter().map(|entry| entry.binding).collect(),
    })
}

/// `profile` with the user's overlay, `<data dir>/keymap.toml`, merged over it, and
/// what was wrong with the overlay when it could not be used -- naming the file and the
/// line; the profile is then used as it is.
pub fn load(profile: &str, data_dir: &std::path::Path) -> (Keymap, Option<String>) {
    let base = Keymap::named(profile);
    let path = data_dir.join("keymap.toml");
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (base, None),
        Err(error) => {
            let message = format!(
                "{} could not be read ({error}); the {} keymap is used as it is",
                path.display(),
                base.name
            );
            return (base, Some(message));
        }
    };
    match merge_overlay(&base, &src) {
        Ok(keymap) => (keymap, None),
        Err(error) => {
            let place = match error.line {
                Some(line) => format!("{} line {line}", path.display()),
                None => path.display().to_string(),
            };
            let message = format!(
                "{place}: {error}; the {} keymap is used as it is",
                base.name
            );
            (base, Some(message))
        }
    }
}

/// An overlay in the profiles' format over `base`: a chord it binds replaces the
/// profile's binding of that chord in that section, and `""` unbinds it. A chord that
/// starts a longer one where both apply is refused: Dexo would wait for the rest of the
/// longer chord, and the shorter would never fire.
pub fn merge_overlay(base: &Keymap, src: &str) -> Result<Keymap, KeymapError> {
    let (_, entries) = entries(src)?;
    let mut keymap = base.clone();
    for entry in &entries {
        let command = entry.binding.command.as_str();
        if !command.is_empty() && crate::palette::command_spec(command).is_none() {
            return Err(KeymapError {
                field: format!("[{}] {}", entry.section, entry.spec),
                reason: format!("no command is called `{command}`"),
                line: Some(entry.line),
            });
        }
        keymap.bindings.retain(|binding| {
            binding.context != entry.binding.context || binding.chord != entry.binding.chord
        });
        if !command.is_empty() {
            keymap.bindings.push(entry.binding.clone());
        }
    }
    for entry in entries
        .iter()
        .filter(|entry| !entry.binding.command.is_empty())
    {
        let mine = &entry.binding;
        let together = |other: &Binding| {
            other.context == mine.context
                || other.context == KeyContext::Global
                || mine.context == KeyContext::Global
        };
        let starts = |short: &Chord, long: &Chord| {
            long.keys.len() > short.keys.len() && long.keys.starts_with(&short.keys)
        };
        if let Some(other) = keymap.bindings.iter().find(|other| {
            together(other)
                && (starts(&mine.chord, &other.chord) || starts(&other.chord, &mine.chord))
        }) {
            let (short, long) = if starts(&mine.chord, &other.chord) {
                (mine, other)
            } else {
                (other, mine)
            };
            return Err(KeymapError {
                field: format!("[{}] {}", entry.section, entry.spec),
                reason: format!(
                    "`{}` ({}) starts `{}` ({}) in [{}], so it would never fire; unbind one with \"\"",
                    chord_label(&short.chord),
                    short.command,
                    chord_label(&long.chord),
                    long.command,
                    section_name(other.context)
                ),
                line: Some(entry.line),
            });
        }
    }
    Ok(keymap)
}

/// The section a context is written as in a keymap file.
fn section_name(context: KeyContext) -> &'static str {
    match context {
        KeyContext::Global => "global",
        KeyContext::Editor => "editor",
        KeyContext::Explorer => "explorer",
        KeyContext::Results => "results",
        KeyContext::Console => "console",
        KeyContext::DocumentTabs => "tabs",
        KeyContext::Palette => "palette",
        KeyContext::Modal => "modal",
    }
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
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
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
            field: format!("[{other}]"),
            reason: "is no keymap section; use global, editor, explorer, results, console, tabs, palette or modal".into(),
            line: None,
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
    // BackTab is Shift+Tab as the terminal spells it: with Ctrl it is Ctrl+Shift+Tab,
    // the chord the keymap names, and Ctrl+Shift+Tab found no command.
    let shifted = key.modifiers.contains(KeyModifiers::SHIFT)
        || key.code == KeyCode::BackTab
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
        KeyCode::Tab | KeyCode::BackTab => "tab".into(),
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
"alt+o" = "editor.open_saved_query"
"ctrl+alt+a" = "mcp.audit"
"ctrl+p" = "palette.open"
"ctrl+q" = "workbench.quit"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"f7" = "explain.open"
"shift+f7" = "explain.analyze"
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
"alt+," = "layout.explorer_shrink"
"alt+." = "layout.explorer_grow"
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
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[editor]
"alt+s" = "editor.save_query"
"ctrl+enter" = "query.execute_statement"
"ctrl+j" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"ctrl+f" = "editor.find"
"ctrl+h" = "editor.replace"
"ctrl+/" = "editor.toggle_comment"
"ctrl+_" = "editor.toggle_comment"
"ctrl+7" = "editor.toggle_comment"
"ctrl+shift+d" = "editor.duplicate_line"
"alt+shift+down" = "editor.duplicate_line"
"ctrl+shift+up" = "editor.move_line_up"
"ctrl+shift+down" = "editor.move_line_down"
"ctrl+e" = "editor.external"
"ctrl+d" = "editor.duplicate_line"
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
"w" = "data.filter"
"o" = "data.sort"
"s" = "results.sort_column"
"shift+s" = "results.sort_add_column"
"t" = "results.count"
"f" = "data.related"
"up" = "results.up"
"down" = "results.down"
"left" = "results.left"
"right" = "results.right"
"pageup" = "results.pageup"
"pagedown" = "results.pagedown"
"shift+up" = "results.extend_up"
"shift+down" = "results.extend_down"
"home" = "results.first_column"
"end" = "results.last_column"
"ctrl+home" = "results.top"
"ctrl+end" = "results.bottom"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"ctrl+c" = "data.copy.cell"
"r" = "results.select_row"
"delete" = "data.toggle_delete"
"f2" = "data.edit_cell"
"i" = "data.insert_row"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"x" = "results.record_view"
"c" = "results.select_column"
"[" = "results.prev_tab"
"]" = "results.next_tab"
"n" = "data.page_next"
"p" = "data.page_prev"
"b" = "data.nav_back"
"?" = "help.open"
"esc" = "results.collapse"
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
"alt+o" = "editor.open_saved_query"
"ctrl+alt+a" = "mcp.audit"
"ctrl+p" = "palette.open"
"ctrl+q" = "workbench.quit"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"f7" = "explain.open"
"shift+f7" = "explain.analyze"
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
"alt+s" = "editor.save_query"
"ctrl+enter" = "query.execute_statement"
"ctrl+j" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"ctrl+f" = "editor.find"
"ctrl+h" = "editor.replace"
"ctrl+/" = "editor.toggle_comment"
"ctrl+_" = "editor.toggle_comment"
"ctrl+7" = "editor.toggle_comment"
"ctrl+shift+d" = "editor.duplicate_line"
"alt+shift+down" = "editor.duplicate_line"
"ctrl+shift+up" = "editor.move_line_up"
"ctrl+shift+down" = "editor.move_line_down"
"ctrl+e" = "editor.external"
"alt+shift+f" = "editor.format"
"ctrl+shift+i" = "editor.format"
"ctrl+c" = "editor.copy"
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
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[results]
"w" = "data.filter"
"o" = "data.sort"
"s" = "results.sort_column"
"shift+s" = "results.sort_add_column"
"t" = "results.count"
"f" = "data.related"
"b" = "data.nav_back"
"k" = "results.up"
"j" = "results.down"
"h" = "results.left"
"l" = "results.right"
"g g" = "results.top"
"delete" = "data.toggle_delete"
"f2" = "data.edit_cell"
"i" = "data.insert_row"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"x" = "results.record_view"
"shift+k" = "results.extend_up"
"shift+j" = "results.extend_down"
"home" = "results.first_column"
"end" = "results.last_column"
"ctrl+home" = "results.top"
"ctrl+end" = "results.bottom"
"shift+g" = "results.bottom"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"ctrl+c" = "data.copy.cell"
"?" = "help.open"
"esc" = "results.collapse"
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
"alt+o" = "editor.open_saved_query"
"ctrl+alt+a" = "mcp.audit"
"alt+x" = "palette.open"
"ctrl+x ctrl+c" = "workbench.quit"
"ctrl+x ctrl+n" = "document.new"
"f1" = "help.open"
"f10" = "layout.cycle"
"ctrl+f2" = "query.cancel"
"f7" = "explain.open"
"shift+f7" = "explain.analyze"
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
"alt+s" = "editor.save_query"
"ctrl+enter" = "query.execute_statement"
"ctrl+j" = "query.execute_statement"
"ctrl+shift+f10" = "query.execute_document"
"ctrl+space" = "editor.complete"
"ctrl+f" = "editor.find"
"ctrl+h" = "editor.replace"
"ctrl+/" = "editor.toggle_comment"
"ctrl+_" = "editor.toggle_comment"
"ctrl+7" = "editor.toggle_comment"
"ctrl+shift+d" = "editor.duplicate_line"
"alt+shift+down" = "editor.duplicate_line"
"ctrl+shift+up" = "editor.move_line_up"
"ctrl+shift+down" = "editor.move_line_down"
"ctrl+x ctrl+e" = "editor.external"
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
"alt+left" = "layout.explorer_shrink"
"alt+right" = "layout.explorer_grow"
[results]
"w" = "data.filter"
"o" = "data.sort"
"s" = "results.sort_column"
"shift+s" = "results.sort_add_column"
"t" = "results.count"
"f" = "data.related"
"b" = "data.nav_back"
"ctrl+p" = "results.up"
"ctrl+n" = "results.down"
"delete" = "data.toggle_delete"
"f2" = "data.edit_cell"
"ctrl+s" = "data.review"
"ctrl+shift+r" = "data.discard_all"
"ctrl+r" = "data.refresh"
"e" = "transfer.export"
"v" = "results.cycle_view"
"x" = "results.record_view"
"left" = "results.left"
"right" = "results.right"
"shift+up" = "results.extend_up"
"shift+down" = "results.extend_down"
"home" = "results.first_column"
"end" = "results.last_column"
"ctrl+home" = "results.top"
"ctrl+end" = "results.bottom"
"enter" = "results.actions"
"ctrl+enter" = "results.toggle_pick"
"alt+w" = "data.copy.cell"
"?" = "help.open"
"esc" = "results.collapse"
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
                // Ctrl+Enter reaches Dexo only where the terminal tells it from Enter
                // (tmux by default, GNOME Terminal and Terminal.app do not); Ctrl+J
                // arrives the same everywhere.
                ("ctrl+j", "query.execute_statement"),
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
        assert_eq!(err.field, "[editor] Ctrl+P");
        assert_eq!(err.line, Some(4));
        assert!(err.reason.contains("`ctrl+p` on line 3"), "{err}");
        assert!(
            err.reason.contains("palette.open") && err.reason.contains("query.execute_document"),
            "{err}"
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

    /// The user's keymap.toml rebinds and unbinds over the profile; one that names a
    /// command Dexo does not have is not used, and says so with its file and line.
    #[test]
    fn a_keymap_overlay_rebinds_unbinds_and_reports_by_line() {
        let base = Keymap::default_profile();
        let overlay = "[editor]\n\"ctrl+r\" = \"query.execute_document\"\n\"ctrl+/\" = \"\"\n";
        let merged = super::merge_overlay(&base, overlay).unwrap();
        assert_eq!(
            merged.resolve(&parse_chord("ctrl+r").unwrap(), KeyContext::Editor),
            Ok(Some("query.execute_document"))
        );
        assert_eq!(
            merged.resolve(&parse_chord("ctrl+/").unwrap(), KeyContext::Editor),
            Ok(None)
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("keymap.toml"),
            "[editor]\n\"ctrl+r\" = \"query.execute_document\"\n\"ctrl+k\" = \"no.such\"\n",
        )
        .unwrap();
        let (keymap, problem) = super::load("vim", dir.path());
        assert_eq!(keymap, Keymap::vim_profile());
        let problem = problem.unwrap();
        assert!(problem.contains("keymap.toml line 3"), "{problem}");
        assert!(problem.contains("no.such"), "{problem}");
    }

    /// Each problem in an overlay names its own line -- a second section, a chord with
    /// a dot, an unknown section, a TOML syntax error -- and the file's section names.
    #[test]
    fn overlay_problems_name_their_own_line() {
        let base = Keymap::default_profile();
        let problem = |src: &str| super::merge_overlay(&base, src).unwrap_err();
        let error = problem(
            "[global]\n\"ctrl+k\" = \"palette.open\"\n\n[editor]\n\"ctrl+k\" = \"no.such\"\n",
        );
        assert_eq!(
            (error.line, error.field.as_str()),
            (Some(5), "[editor] ctrl+k")
        );
        let error = problem("[editor]\n\"ctrl+.\" = \"no.such\"\n");
        assert_eq!(error.line, Some(2));
        let error = problem("\n[editr]\n\"ctrl+k\" = \"palette.open\"\n");
        assert_eq!((error.line, error.field.as_str()), (Some(2), "[editr]"));
        let error = problem("[editor]\n\"ctrl+k\" = palette.open\n");
        assert_eq!(error.line, Some(2));
        assert!(!error.to_string().contains("toml["), "{error}");
    }

    /// An overlay chord that starts a profile chord, or a profile chord that starts an
    /// overlay chord, would never fire, and is refused.
    #[test]
    fn an_overlay_chord_that_starts_another_is_refused() {
        let emacs = Keymap::emacs_profile();
        let error =
            super::merge_overlay(&emacs, "[editor]\n\"ctrl+x\" = \"palette.open\"\n").unwrap_err();
        assert_eq!(error.line, Some(2));
        assert!(error.reason.contains("never fire"), "{error}");
        assert!(
            error.reason.contains("[global]") || error.reason.contains("[editor]"),
            "{error}"
        );
        let error =
            super::merge_overlay(&emacs, "[global]\n\"ctrl+x ctrl+c x\" = \"palette.open\"\n")
                .unwrap_err();
        assert!(
            error.reason.contains("`ctrl+x ctrl+c` (workbench.quit)"),
            "{error}"
        );
        // Unbinding the longer one makes room.
        assert!(
            super::merge_overlay(
                &emacs,
                "[global]\n\"ctrl+x ctrl+c\" = \"\"\n\"ctrl+x ctrl+n\" = \"\"\n[editor]\n\"ctrl+x ctrl+e\" = \"\"\n\"ctrl+x\" = \"palette.open\"\n"
            )
            .is_ok()
        );
    }

    /// A keymap.toml that cannot be read -- not UTF-8, or not a file -- says so.
    #[test]
    fn an_unreadable_overlay_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keymap.toml"), [0xff, 0xfe, b'[']).unwrap();
        let (keymap, problem) = super::load("default", dir.path());
        assert_eq!(keymap, Keymap::default_profile());
        assert!(problem.unwrap().contains("could not be read"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("keymap.toml")).unwrap();
        assert!(super::load("default", dir.path()).1.is_some());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(super::load("default", dir.path()).1, None);
    }

    /// Terminals send Shift+Tab as BackTab; the keymap writes it `shift+tab`, and with
    /// Ctrl `ctrl+shift+tab`, which is what Previous Document is bound to.
    #[test]
    fn back_tab_is_shift_tab() {
        let label = |modifiers| {
            super::chord_label(&super::chord_from_event(KeyEvent::new(
                KeyCode::BackTab,
                modifiers,
            )))
        };
        assert_eq!(
            label(KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            "ctrl+shift+tab"
        );
        assert_eq!(label(KeyModifiers::CONTROL), "ctrl+shift+tab");
        assert_eq!(label(KeyModifiers::SHIFT), "shift+tab");
        let chord = super::chord_from_event(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(
            Keymap::default_profile()
                .resolve(&chord, KeyContext::Editor)
                .unwrap(),
            Some("document.prev")
        );
    }

    /// `Alt+[` reaches the terminal as `ESC [`, which opens every CSI sequence: it did
    /// nothing, and ate the next key. The explorer is resized with Alt+, and Alt+. now,
    /// and Alt+= and Alt+- belong to the results pane wherever the focus is.
    #[test]
    fn the_resize_keys_name_one_pane_each_and_none_starts_an_escape_sequence() {
        let keymap = Keymap::default_profile();
        let resolve = |chord: &str, context| {
            keymap
                .resolve(&parse_chord(chord).unwrap(), context)
                .unwrap()
        };
        assert_eq!(
            resolve("alt+,", KeyContext::Editor),
            Some("layout.explorer_shrink")
        );
        assert_eq!(
            resolve("alt+.", KeyContext::Results),
            Some("layout.explorer_grow")
        );
        assert_eq!(resolve("alt+[", KeyContext::Editor), None);
        for context in [
            KeyContext::Explorer,
            KeyContext::Editor,
            KeyContext::Results,
        ] {
            assert_eq!(resolve("alt+=", context), Some("layout.results_grow"));
            assert_eq!(resolve("alt+-", context), Some("layout.results_shrink"));
        }
    }
}
