//! Compare Schema: pick two sources -- connections that are open, saved snapshots, a
//! snapshot file -- and see what the second has that the first lacks, with the script that
//! makes the first like the second.
//!
//! It used to take both sides from the one selected connection, so it could only compare a
//! database with itself, and showed the result as a dump of internal ids.

use crossterm::event::{KeyCode, KeyEvent};
use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, classify_difference, generate_script, render_unquoted,
};
use dexo_driver_api::ChangeRisk;

use crate::action::{DiffSide, Effect};
use crate::model::Model;
use crate::runtime::SessionId;
use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Debug, PartialEq)]
pub struct DiffEntry {
    pub kind: &'static str,
    pub object: String,
    pub risk: String,
}

/// What a side of the comparison can be.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffOptionKind {
    /// A connection that is open: its catalog is read now.
    Live(SessionId),
    /// A snapshot saved with `dexo schema snapshot`.
    Snapshot,
    /// A snapshot file, whose path is typed.
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffOption {
    /// As the picker shows it.
    pub label: String,
    /// The connection or snapshot name; empty for a file.
    pub name: String,
    pub kind: DiffOptionKind,
    pub driver: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchemaDiffScreen {
    pub open: bool,
    /// Picking the two sources, rather than reading the result.
    pub source_prompt: bool,
    pub from_label: String,
    pub to_label: String,
    pub show_added: bool,
    pub show_removed: bool,
    pub show_changed: bool,
    pub entries: Vec<DiffEntry>,
    pub selected: usize,
    pub script: String,
    pub loading: bool,
    pub error: Option<String>,
    pub ordered: Vec<OrderedChange>,
    /// What can be compared, and which is picked for each side.
    pub options: Vec<DiffOption>,
    pub pick: [usize; 2],
    /// The row with the focus while picking: From, To, or the file's path.
    pub row: usize,
    pub file: TextInput,
    /// The rows (or the list of differences), or one of the two buttons.
    pub footer: FooterFocus,
    /// The connection the first side was, if it was one: the script makes it like the
    /// second, so it is the one the script is for.
    pub from_connection: Option<String>,
}

impl Default for SchemaDiffScreen {
    fn default() -> Self {
        Self {
            open: false,
            source_prompt: false,
            from_label: String::new(),
            to_label: String::new(),
            show_added: true,
            show_removed: true,
            show_changed: true,
            entries: Vec::new(),
            selected: 0,
            script: String::new(),
            loading: false,
            error: None,
            ordered: Vec::new(),
            options: Vec::new(),
            pick: [0, 0],
            row: 0,
            file: TextInput::default(),
            footer: FooterFocus::Input,
            from_connection: None,
        }
    }
}

/// A risk in words; nothing for a change that risks nothing.
pub fn describe_diff_risk(risk: &ChangeRisk) -> String {
    let mut parts = Vec::new();
    if risk.destructive {
        parts.push("drops it");
    }
    if risk.data_loss {
        parts.push("may lose data");
    }
    if !risk.reversible {
        parts.push("cannot be undone");
    }
    parts.join(", ")
}

impl SchemaDiffScreen {
    pub fn from_ordered(
        from_label: impl Into<String>,
        to_label: impl Into<String>,
        ordered: &[OrderedChange],
    ) -> Self {
        let script = generate_script(ordered, render_unquoted).forward;
        let entries = ordered
            .iter()
            .map(|item| {
                let risk = describe_diff_risk(&classify_difference(&item.difference));
                let (kind, object) = match &item.difference {
                    SchemaDifference::Added(object) => ("added", object),
                    SchemaDifference::Removed(object) => ("removed", object),
                    SchemaDifference::Changed { after, .. } => ("changed", after),
                };
                DiffEntry {
                    kind,
                    object: format!(
                        "{} {}",
                        object.kind.as_str(),
                        object.qualified_name.display_unquoted()
                    ),
                    risk,
                }
            })
            .collect();
        Self {
            open: true,
            from_label: from_label.into(),
            to_label: to_label.into(),
            entries,
            script,
            ordered: ordered.to_vec(),
            ..Self::default()
        }
    }

    pub fn fixture() -> Self {
        Self {
            open: true,
            from_label: "prod@v1".into(),
            to_label: "prod@v2".into(),
            entries: vec![
                DiffEntry {
                    kind: "added",
                    object: "table db.public.orders_new".into(),
                    risk: String::new(),
                },
                DiffEntry {
                    kind: "removed",
                    object: "table db.public.gone".into(),
                    risk: "drops it, may lose data, cannot be undone".into(),
                },
                DiffEntry {
                    kind: "changed",
                    object: "column db.public.users.age".into(),
                    risk: "may lose data, cannot be undone".into(),
                },
            ],
            script: "DROP TABLE db.public.gone;\n".into(),
            ..Self::default()
        }
    }

    /// Opens on the sources to pick from. The first side starts on the active connection,
    /// the second on the next thing that is not it.
    pub fn open_picker(&mut self, options: Vec<DiffOption>, active: Option<&str>) {
        *self = Self {
            open: true,
            source_prompt: true,
            file: std::mem::take(&mut self.file),
            options,
            ..Self::default()
        };
        let first = active
            .and_then(|name| {
                self.options.iter().position(|option| {
                    matches!(option.kind, DiffOptionKind::Live(_)) && option.name == name
                })
            })
            .unwrap_or(0);
        self.pick = [
            first,
            if self.options.len() > 1 {
                (first + 1) % self.options.len()
            } else {
                0
            },
        ];
    }

    /// The snapshots saved since the picker opened, put after the connections.
    pub fn add_snapshots(&mut self, snapshots: Vec<DiffOption>) {
        let Some(file) = self
            .options
            .iter()
            .position(|option| option.kind == DiffOptionKind::File)
        else {
            return;
        };
        for (offset, snapshot) in snapshots.into_iter().enumerate() {
            self.options.insert(file + offset, snapshot);
        }
        // The file option moved: the picks that were on it follow it.
        let new_file = self
            .options
            .iter()
            .position(|option| option.kind == DiffOptionKind::File)
            .unwrap_or(0);
        for pick in &mut self.pick {
            if *pick == file {
                *pick = new_file;
            }
        }
    }

    pub fn option(&self, side: usize) -> Option<&DiffOption> {
        self.options.get(self.pick[side])
    }

    /// Whether a typed path is part of the question.
    pub fn uses_file(&self) -> bool {
        (0..2).any(|side| {
            self.option(side)
                .is_some_and(|option| option.kind == DiffOptionKind::File)
        })
    }

    /// Rows the keys walk while picking: From, To and, with a file chosen, its path.
    pub fn rows(&self) -> usize {
        if self.uses_file() { 3 } else { 2 }
    }

    pub fn cycle(&mut self, side: usize, delta: isize) {
        let count = self.options.len();
        if count == 0 {
            return;
        }
        self.pick[side] = (self.pick[side] as isize + delta).rem_euclid(count as isize) as usize;
        self.error = None;
        self.row = self.row.min(self.rows() - 1);
    }

    /// The question to send, or why there is none yet.
    pub fn request(&self) -> Result<(DiffSide, DiffSide), String> {
        let (Some(from), Some(to)) = (self.option(0), self.option(1)) else {
            return Err("There is nothing to compare yet.".into());
        };
        if self.pick[0] == self.pick[1] {
            return Err("Pick two different sources.".into());
        }
        let side = |option: &DiffOption| match &option.kind {
            DiffOptionKind::Live(session) => Ok(DiffSide::Live {
                session: *session,
                driver: option.driver.clone(),
                name: option.name.clone(),
            }),
            DiffOptionKind::Snapshot => Ok(DiffSide::Snapshot {
                name: option.name.clone(),
            }),
            DiffOptionKind::File => {
                let path = self.file.as_str().trim();
                if path.is_empty() {
                    Err("Type the path of the snapshot file.".to_string())
                } else {
                    Ok(DiffSide::File { path: path.into() })
                }
            }
        };
        Ok((side(from)?, side(to)?))
    }

    pub fn filtered(&self) -> Vec<&DiffEntry> {
        self.entries
            .iter()
            .filter(|entry| match entry.kind {
                "added" => self.show_added,
                "removed" => self.show_removed,
                "changed" => self.show_changed,
                _ => true,
            })
            .collect()
    }

    pub fn toggle_added(&mut self) {
        self.show_added = !self.show_added;
        self.clamp_selection();
    }

    pub fn toggle_removed(&mut self) {
        self.show_removed = !self.show_removed;
        self.clamp_selection();
    }

    pub fn toggle_changed(&mut self) {
        self.show_changed = !self.show_changed;
        self.clamp_selection();
    }

    pub fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.filtered().len().saturating_sub(1));
    }

    /// The button the footer's first stop is: it compares while picking and, with a
    /// result, opens the script in a document.
    pub fn submit_label(&self) -> &'static str {
        if self.source_prompt {
            "Compare"
        } else {
            "Open script"
        }
    }

    fn count(&self, kind: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .count()
    }

    /// The body above the buttons, the line of the selected difference, and the line the
    /// differences start on. Lines are `width` columns wide at most.
    pub fn body(&self, width: usize) -> (Vec<String>, Option<usize>, usize) {
        use crate::model::truncate_cell;
        let mut lines = Vec::new();
        if self.source_prompt {
            lines.push("Compare two schemas; the script makes From like To.".into());
            lines.push(String::new());
            let on_rows = self.footer == FooterFocus::Input;
            for (side, name) in ["From", "To  "].into_iter().enumerate() {
                let marker = if on_rows && self.row == side {
                    ">"
                } else {
                    " "
                };
                let label = self
                    .option(side)
                    .map_or("(nothing to pick)".to_string(), |option| {
                        option.label.clone()
                    });
                lines.push(truncate_cell(
                    &format!("{marker} {name}: \u{2039} {label} \u{203a}"),
                    width,
                ));
            }
            if self.uses_file() {
                let focused = on_rows && self.row == 2;
                lines.push(self.file.inline_line_within(
                    &format!("{} File: ", if focused { ">" } else { " " }),
                    focused,
                    width,
                ));
            }
            lines.push(String::new());
            match &self.error {
                Some(error) if !self.loading => lines.push(error.clone()),
                _ if self.loading => lines.push("Reading both schemas...".into()),
                _ if self.options.len() < 2 => lines.push(
                    "Connect a second connection or save a snapshot (dexo schema snapshot) to compare."
                        .into(),
                ),
                _ => {}
            }
            return (lines, None, 0);
        }
        lines.push(truncate_cell(
            &format!(
                "{} -> {}: {}",
                self.from_label,
                self.to_label,
                match self.entries.len() {
                    0 => "no differences".to_string(),
                    1 => "1 difference".to_string(),
                    n => format!("{n} differences"),
                }
            ),
            width,
        ));
        lines.push(truncate_cell(
            &format!(
                "Show: [{}] added {}  [{}] removed {}  [{}] changed {}   (a / r / c)",
                if self.show_added { "x" } else { " " },
                self.count("added"),
                if self.show_removed { "x" } else { " " },
                self.count("removed"),
                if self.show_changed { "x" } else { " " },
                self.count("changed"),
            ),
            width,
        ));
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(String::new());
        let entries_from = lines.len();
        let shown = self.filtered();
        let on_list = self.footer == FooterFocus::Input;
        for (index, entry) in shown.iter().enumerate() {
            let marker = if on_list && index == self.selected {
                ">"
            } else {
                " "
            };
            let risk = if entry.risk.is_empty() {
                String::new()
            } else {
                format!("  ({})", entry.risk)
            };
            lines.push(truncate_cell(
                &format!("{marker} {:<8} {}{risk}", entry.kind, entry.object),
                width,
            ));
        }
        if self.entries.is_empty() {
            lines.push("The two schemas are the same.".into());
        } else if shown.is_empty() {
            lines.push("Every difference is filtered out; a, r and c bring them back.".into());
        }
        let selected_line =
            (!shown.is_empty()).then(|| entries_from + self.selected.min(shown.len() - 1));
        if !self.script.is_empty() {
            lines.push(String::new());
            lines.push("Migration script".into());
            lines.extend(self.script.lines().map(|line| format!("  {line}")));
        }
        (lines, selected_line, entries_from)
    }

    /// The whole dialog as text: the body, then the buttons.
    pub fn lines(&self) -> Vec<String> {
        let (mut lines, _, _) = self.body(200);
        lines.push(crate::widgets::form::footer_line(
            self.submit_label(),
            if self.footer == FooterFocus::Input {
                FooterFocus::Submit
            } else {
                self.footer
            },
        ));
        lines
    }
}

/// The keys of the dialog: the rows (or the list), Left and Right on a row, the filters,
/// Enter, Esc.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let on_rows = model.schema_diff.footer == FooterFocus::Input;
    if model.schema_diff.source_prompt {
        let rows = model.schema_diff.rows();
        let row = model.schema_diff.row;
        if on_rows {
            match key.code {
                KeyCode::Tab | KeyCode::Down if row + 1 < rows => {
                    model.schema_diff.row += 1;
                    return Vec::new();
                }
                KeyCode::BackTab | KeyCode::Up if row > 0 => {
                    model.schema_diff.row -= 1;
                    return Vec::new();
                }
                KeyCode::Left | KeyCode::Right if row < 2 => {
                    let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                    model.schema_diff.cycle(row, delta);
                    return Vec::new();
                }
                _ if row == 2 && model.schema_diff.file.handle_key(key) => {
                    model.schema_diff.error = None;
                    return Vec::new();
                }
                _ => {}
            }
        }
        let before = model.schema_diff.footer;
        return match footer_key(&mut model.schema_diff.footer, &key) {
            FooterKey::Cancel => {
                model.schema_diff.open = false;
                Vec::new()
            }
            FooterKey::Submit => request(model),
            FooterKey::Moved => {
                // Walking back up from the buttons lands on the row it came in from.
                if model.schema_diff.footer == FooterFocus::Input {
                    model.schema_diff.row = if before == FooterFocus::Cancel {
                        0
                    } else {
                        rows - 1
                    };
                }
                Vec::new()
            }
            FooterKey::Pass => Vec::new(),
        };
    }
    if on_rows {
        match key.code {
            KeyCode::Up => {
                model.schema_diff.selected = model.schema_diff.selected.saturating_sub(1);
                return Vec::new();
            }
            KeyCode::Down => {
                let last = model.schema_diff.filtered().len().saturating_sub(1);
                model.schema_diff.selected = (model.schema_diff.selected + 1).min(last);
                return Vec::new();
            }
            KeyCode::Char('a') => {
                model.schema_diff.toggle_added();
                return Vec::new();
            }
            KeyCode::Char('r') => {
                model.schema_diff.toggle_removed();
                return Vec::new();
            }
            KeyCode::Char('c') => {
                model.schema_diff.toggle_changed();
                return Vec::new();
            }
            _ => {}
        }
    }
    match footer_key(&mut model.schema_diff.footer, &key) {
        FooterKey::Cancel => {
            model.schema_diff.open = false;
            Vec::new()
        }
        FooterKey::Submit => open_script(model),
        FooterKey::Moved | FooterKey::Pass => Vec::new(),
    }
}

/// Reads both sides and compares them.
pub fn request(model: &mut Model) -> Vec<Effect> {
    let (left, right) = match model.schema_diff.request() {
        Ok(sides) => sides,
        Err(message) => {
            model.schema_diff.error = Some(message);
            return Vec::new();
        }
    };
    // A live side is rendered with its own driver's DDL; two saved sides with none.
    let render_session = [&left, &right].into_iter().find_map(|side| match side {
        DiffSide::Live { session, .. } => Some(*session),
        _ => None,
    });
    model.schema_diff.loading = true;
    model.schema_diff.error = None;
    model.schema_diff.from_connection = match &left {
        DiffSide::Live { name, .. } => Some(name.clone()),
        _ => None,
    };
    vec![Effect::LoadSchemaDiff {
        left,
        right,
        render_session,
        generation: model.session_generation,
    }]
}

/// The script in a document of its own, to read, change and run through the connection's
/// own checks. Nothing is applied from here.
pub fn open_script(model: &mut Model) -> Vec<Effect> {
    if model.schema_diff.script.is_empty() {
        model
            .messages
            .info("There is no script: the two schemas are the same.".into());
        return Vec::new();
    }
    model.schema_diff.open = false;
    let text = format!(
        "-- Makes {} like {}.\n{}",
        model.schema_diff.from_label, model.schema_diff.to_label, model.schema_diff.script
    );
    let connection = model.schema_diff.from_connection.clone();
    crate::update::open_text_document(model, "migration.sql", &text, connection.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<DiffOption> {
        let live = |name: &str, id: u128| DiffOption {
            label: format!("{name}  (connected)"),
            name: name.into(),
            kind: DiffOptionKind::Live(SessionId(uuid::Uuid::from_u128(id))),
            driver: "postgres".into(),
        };
        vec![
            live("pg-dev", 1),
            live("pg-b", 2),
            DiffOption {
                label: "snapshot snap-before".into(),
                name: "snap-before".into(),
                kind: DiffOptionKind::Snapshot,
                driver: "postgres".into(),
            },
            DiffOption {
                label: "a snapshot file...".into(),
                name: String::new(),
                kind: DiffOptionKind::File,
                driver: String::new(),
            },
        ]
    }

    #[test]
    fn filters_hide_removed_and_the_dialog_says_what_it_shows() {
        let mut screen = SchemaDiffScreen::fixture();
        screen.toggle_removed();
        let visible: Vec<_> = screen
            .filtered()
            .iter()
            .map(|entry| entry.object.as_str())
            .collect();
        assert!(!visible.iter().any(|object| object.contains("gone")));
        assert!(visible.iter().any(|object| object.contains("orders_new")));
        let dump = screen.lines().join("\n");
        assert!(dump.contains("prod@v1 -> prod@v2: 3 differences"), "{dump}");
        assert!(dump.contains("[ ] removed 1"), "{dump}");
        assert!(dump.contains("Migration script"), "{dump}");
        assert!(dump.contains("[Open script]"), "{dump}");
        for raw in ["destructive=", "apply=", "confirm=", "Live(", "sources "] {
            assert!(!dump.contains(raw), "{raw}: {dump}");
        }
    }

    #[test]
    fn filters_clamp_the_selection_to_remaining_entries() {
        let mut screen = SchemaDiffScreen::fixture();
        screen.selected = 2;
        screen.toggle_changed();
        assert_eq!(screen.selected, 1);
    }

    /// Two different sources, not the connection against itself.
    #[test]
    fn the_picker_starts_on_the_active_connection_against_another_source() {
        let mut screen = SchemaDiffScreen::default();
        screen.open_picker(options(), Some("pg-b"));
        assert_eq!(screen.option(0).unwrap().name, "pg-b");
        assert_ne!(screen.option(1).unwrap().name, "pg-b");
        let shown = screen.lines().join("\n");
        assert!(
            shown.contains("From: \u{2039} pg-b  (connected)"),
            "{shown}"
        );
        assert!(shown.contains("[Compare]"), "{shown}");
        let (from, to) = screen.request().expect("two different sources");
        assert!(matches!(from, DiffSide::Live { ref name, .. } if name == "pg-b"));
        assert!(matches!(
            to,
            DiffSide::Live { .. } | DiffSide::Snapshot { .. }
        ));

        screen.pick = [1, 1];
        assert_eq!(screen.request().unwrap_err(), "Pick two different sources.");
    }

    #[test]
    fn a_file_side_asks_for_its_path() {
        let mut screen = SchemaDiffScreen::default();
        screen.open_picker(options(), Some("pg-dev"));
        screen.pick = [0, 3];
        assert!(screen.uses_file() && screen.rows() == 3);
        assert_eq!(
            screen.request().unwrap_err(),
            "Type the path of the snapshot file."
        );
        screen.file.set_text("/tmp/snap.json");
        assert!(matches!(
            screen.request().unwrap().1,
            DiffSide::File { ref path } if path.to_str() == Some("/tmp/snap.json")
        ));
    }

    #[test]
    fn saved_snapshots_slip_in_before_the_file_option() {
        let mut screen = SchemaDiffScreen::default();
        screen.open_picker(options()[..1].to_vec(), Some("pg-dev"));
        screen.options.push(options().remove(3));
        screen.pick = [0, 1];
        screen.add_snapshots(vec![DiffOption {
            label: "snapshot nightly".into(),
            name: "nightly".into(),
            kind: DiffOptionKind::Snapshot,
            driver: "postgres".into(),
        }]);
        assert_eq!(screen.options.len(), 3);
        assert_eq!(screen.options[1].name, "nightly");
        assert_eq!(
            screen.option(1).unwrap().kind,
            DiffOptionKind::File,
            "the pick followed"
        );
    }
}
