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

impl DiffEntry {
    /// The kind of object it is: `table`, `index`, the first word of `object`.
    pub fn object_kind(&self) -> &str {
        self.object.split_once(' ').map_or("", |(kind, _)| kind)
    }

    /// The object without its kind.
    pub fn name(&self) -> &str {
        self.object
            .split_once(' ')
            .map_or(self.object.as_str(), |(_, name)| name)
    }
}

/// A kind of object as a heading: `Tables`, `Indexes`.
pub fn kind_heading(kind: &str) -> String {
    let mut heading: String = kind
        .chars()
        .enumerate()
        .map(|(at, ch)| if at == 0 { ch.to_ascii_uppercase() } else { ch })
        .collect();
    heading.push_str(if heading.ends_with('x') || heading.ends_with('s') {
        "es"
    } else {
        "s"
    });
    heading.replace('_', " ")
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
    /// The detail shows the whole script, not only the picked difference's part.
    pub whole_script: bool,
    /// Lines the detail is scrolled down.
    pub scroll: u16,
    /// A result is there, under the sources being picked again.
    pub compared: bool,
}

impl Default for SchemaDiffScreen {
    fn default() -> Self {
        Self {
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
            whole_script: false,
            scroll: 0,
            compared: false,
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
            from_label: from_label.into(),
            to_label: to_label.into(),
            entries,
            script,
            ordered: ordered.to_vec(),
            compared: true,
            ..Self::default()
        }
    }

    pub fn fixture() -> Self {
        Self {
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
    /// the second on the next connection or snapshot after it -- a file, whose path has
    /// to be typed, only when there is nothing else.
    pub fn open_picker(&mut self, options: Vec<DiffOption>, active: Option<&str>) {
        *self = Self {
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
        let count = self.options.len();
        let second = (1..count)
            .map(|step| (first + step) % count)
            .find(|at| self.options[*at].kind != DiffOptionKind::File)
            .or((count > 1).then(|| (first + 1) % count))
            .unwrap_or(0);
        self.pick = [first, second];
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

    /// A side as the toolbar names it.
    pub fn side_name(&self, side: usize) -> String {
        match self.option(side) {
            Some(option) if option.kind == DiffOptionKind::File => {
                let path = self.file.trim();
                if path.is_empty() {
                    "a file".into()
                } else {
                    format!("file {path}")
                }
            }
            Some(option) => option.name.clone(),
            None if side == 0 && !self.from_label.is_empty() => self.from_label.clone(),
            None if side == 1 && !self.to_label.is_empty() => self.to_label.clone(),
            None => "-".into(),
        }
    }

    /// From and To exchanged.
    pub fn swap(&mut self) {
        self.pick.swap(0, 1);
        std::mem::swap(&mut self.from_label, &mut self.to_label);
        self.error = None;
    }

    /// The result of comparing, over the sources it came from: they stay to be looked at,
    /// swapped, or compared again.
    pub fn with_sources(mut self, sources: Self) -> Self {
        self.options = sources.options;
        self.pick = sources.pick;
        self.file = sources.file;
        self.from_connection = sources.from_connection;
        self
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

    /// The differences the filters leave, as listed: by the kind of object, the kinds in
    /// the order the script takes them.
    pub fn shown_indices(&self) -> Vec<usize> {
        let mut kinds: Vec<&str> = Vec::new();
        for entry in &self.entries {
            if !kinds.contains(&entry.object_kind()) {
                kinds.push(entry.object_kind());
            }
        }
        let mut shown: Vec<usize> = (0..self.entries.len())
            .filter(|index| match self.entries[*index].kind {
                "added" => self.show_added,
                "removed" => self.show_removed,
                "changed" => self.show_changed,
                _ => true,
            })
            .collect();
        shown.sort_by_key(|index| {
            kinds
                .iter()
                .position(|kind| *kind == self.entries[*index].object_kind())
        });
        shown
    }

    pub fn filtered(&self) -> Vec<&DiffEntry> {
        self.shown_indices()
            .into_iter()
            .map(|index| &self.entries[index])
            .collect()
    }

    /// Whether a kind of difference is hidden.
    pub fn filtering(&self) -> bool {
        !(self.show_added && self.show_removed && self.show_changed)
    }

    /// The statement for the picked difference, or the whole script; empty when there is
    /// none to show.
    pub fn shown_sql(&self) -> String {
        if self.whole_script {
            return self.script.clone();
        }
        self.shown_indices()
            .get(self.selected)
            .and_then(|index| self.ordered.get(*index))
            .map(|change| generate_script(std::slice::from_ref(change), render_unquoted).forward)
            .unwrap_or_default()
    }

    /// The picked difference.
    pub fn picked(&self) -> Option<&DiffEntry> {
        self.shown_indices()
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
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
        self.scroll = 0;
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
}

/// The keys of the dialog: the rows (or the list), Left and Right on a row, the filters,
/// Enter, Esc.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> Option<Vec<Effect>> {
    let on_rows = model.schema_diff.footer == FooterFocus::Input;
    if model.schema_diff.source_prompt {
        let rows = model.schema_diff.rows();
        let row = model.schema_diff.row;
        // `s` exchanges the sides and `e` compares them, from anywhere but the file's
        // path, which they are typed in.
        let typing = on_rows && row == 2;
        if key.code == KeyCode::Char('s') && !typing {
            model.schema_diff.swap();
            return Some(Vec::new());
        }
        if key.code == KeyCode::Char('e') && !typing {
            return Some(request(model));
        }
        if on_rows {
            match key.code {
                KeyCode::Tab | KeyCode::Down if row + 1 < rows => {
                    model.schema_diff.row += 1;
                    return Some(Vec::new());
                }
                KeyCode::BackTab | KeyCode::Up if row > 0 => {
                    model.schema_diff.row -= 1;
                    return Some(Vec::new());
                }
                KeyCode::Left | KeyCode::Right if row < 2 => {
                    let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                    model.schema_diff.cycle(row, delta);
                    return Some(Vec::new());
                }
                _ if row == 2 && model.schema_diff.file.handle_key(key) => {
                    model.schema_diff.error = None;
                    return Some(Vec::new());
                }
                _ => {}
            }
        }
        let before = model.schema_diff.footer;
        return Some(match footer_key(&mut model.schema_diff.footer, &key) {
            // Cancel goes back to the result there is, else leaves the screen.
            FooterKey::Cancel if model.schema_diff.compared => {
                model.schema_diff.source_prompt = false;
                model.schema_diff.footer = FooterFocus::Input;
                Vec::new()
            }
            FooterKey::Cancel => return None,
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
        });
    }
    let diff = &mut model.schema_diff;
    match key.code {
        // The kinds hidden come back first; then the screen is left.
        KeyCode::Esc if diff.filtering() => {
            diff.show_added = true;
            diff.show_removed = true;
            diff.show_changed = true;
            diff.clamp_selection();
        }
        KeyCode::Up => {
            diff.selected = diff.selected.saturating_sub(1);
            diff.scroll = 0;
        }
        KeyCode::Down => {
            let last = diff.filtered().len().saturating_sub(1);
            diff.selected = (diff.selected + 1).min(last);
            diff.scroll = 0;
        }
        KeyCode::Char('a') => diff.toggle_added(),
        KeyCode::Char('r') => diff.toggle_removed(),
        KeyCode::Char('c') => diff.toggle_changed(),
        KeyCode::Char('w') => {
            diff.whole_script = !diff.whole_script;
            diff.scroll = 0;
        }
        KeyCode::PageDown | KeyCode::PageUp => {
            let page = i32::from(model.hits.page(
                crate::mouse::ScrollArea::SchemaDiff,
                (model.height / 3).max(1),
            ));
            let delta = if key.code == KeyCode::PageDown {
                page
            } else {
                -page
            };
            model.schema_diff.scroll = model.hits.scroll(
                crate::mouse::ScrollArea::SchemaDiff,
                model.schema_diff.scroll,
                delta,
            );
        }
        KeyCode::Enter => return Some(open_script(model)),
        KeyCode::Char('y') => {
            let sql = diff.shown_sql();
            if sql.trim().is_empty() {
                return Some(Vec::new());
            }
            model.messages.info("Copied the script.".into());
            return Some(vec![Effect::CopyToClipboard { text: sql }]);
        }
        // Compared again, the sides exchanged.
        KeyCode::Char('s') => {
            diff.swap();
            return Some(compare_again(model));
        }
        KeyCode::Char('e') => return Some(compare_again(model)),
        // Other sources: the pickers have the keys.
        KeyCode::Char('p') => {
            diff.source_prompt = true;
            diff.row = 0;
            diff.footer = FooterFocus::Input;
            diff.error = None;
        }
        _ => return None,
    }
    Some(Vec::new())
}

/// The sources compared again, as they are picked now; with none kept -- a comparison
/// made before they were -- the pickers open.
fn compare_again(model: &mut Model) -> Vec<Effect> {
    if model.schema_diff.options.is_empty() {
        return crate::update::new_schema_comparison(model);
    }
    request(model)
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
    let text = format!(
        "-- Makes {} like {}.\n{}",
        model.schema_diff.from_label, model.schema_diff.to_label, model.schema_diff.script
    );
    let connection = model.schema_diff.from_connection.clone();
    // The document is on the workbench, where it is read.
    let mut effects = crate::update::update(
        model,
        crate::action::Action::GoToScreen(crate::model::Screen::Workbench),
    );
    effects.extend(crate::update::open_text_document(
        model,
        "migration.sql",
        &text,
        connection.as_deref(),
    ));
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Compare screen with `screen` on it, as drawn.
    fn shown(screen: SchemaDiffScreen) -> String {
        let mut model = Model {
            screen: crate::model::Screen::Compare,
            ..Model::default()
        };
        model.schema_diff = screen;
        crate::render::render_to_string(&model, 140, 30)
    }

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
        let dump = shown(screen);
        assert!(dump.contains("From ‹ prod@v1 ›"), "{dump}");
        assert!(dump.contains("To ‹ prod@v2 ›"), "{dump}");
        assert!(dump.contains("−1 removed (hidden)"), "{dump}");
        assert!(dump.contains("[⏎ Open script]"), "{dump}");
        for raw in ["destructive=", "apply=", "confirm=", "Live(", "sources ["] {
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
        let dump = shown(screen.clone());
        assert!(dump.contains("From ‹ pg-b ›"), "{dump}");
        assert!(dump.contains("[Compare]"), "{dump}");
        let (from, to) = screen.request().expect("two different sources");
        assert!(matches!(from, DiffSide::Live { ref name, .. } if name == "pg-b"));
        assert!(matches!(
            to,
            DiffSide::Live { .. } | DiffSide::Snapshot { .. }
        ));

        screen.pick = [1, 1];
        assert_eq!(screen.request().unwrap_err(), "Pick two different sources.");
    }

    /// The connection in use last in the list: the second side wrapped around to the
    /// file, whose path then had to be typed, past the other connection.
    #[test]
    fn the_second_side_is_another_connection_before_a_file() {
        let mut screen = SchemaDiffScreen::default();
        let mut sources = options();
        sources.retain(|option| option.kind != DiffOptionKind::Snapshot);
        screen.open_picker(sources, Some("pg-b"));
        assert_eq!(screen.option(0).unwrap().name, "pg-b");
        assert_eq!(screen.option(1).unwrap().name, "pg-dev");
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
