use std::path::{Path, PathBuf};

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FilePickerMode {
    #[default]
    Open,
    Save,
    Transfer,
    Diagnostics,
    ConfigExport,
    ConfigImport,
}

impl FilePickerMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Open => "Open file",
            Self::Save => "Save file",
            Self::Transfer => "Choose path",
            Self::Diagnostics => "Save diagnostics",
            Self::ConfigExport => "Export config to",
            Self::ConfigImport => "Import config from",
        }
    }

    pub fn submit_label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Save | Self::Diagnostics | Self::ConfigExport => "Save",
            Self::Transfer => "Choose",
            Self::ConfigImport => "Open",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FilePickerSection {
    #[default]
    Browser,
    Recent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilePickerLineKind {
    Cwd,
    RecentHeader,
    RecentItem(usize),
    BrowseHeader,
    BrowserEntry(usize),
    Padding,
    Name,
    Error,
    Footer,
    Hint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilePickerLayout {
    pub lines: Vec<String>,
    pub kinds: Vec<FilePickerLineKind>,
    pub browser_offset: usize,
    pub browser_rows: usize,
}

/// Open's field: what is typed finds files in the folder.
const FIND_LABEL: &str = "Find: ";
/// The other pickers' field: the file's name.
const NAME_LABEL: &str = "Name: ";

/// Recent files the Open picker lists; more pushed the name field and the buttons out
/// of the dialog.
const MAX_RECENT: usize = 5;
/// Rows the picker spends on itself: the folder line, the field, the buttons. Its keys
/// are on the status line.
const CHROME_ROWS: usize = 3;
/// Fewest file rows worth showing: below it the recent files give way.
const MIN_LIST_ROWS: usize = 4;

/// Height of the picker's popup on a terminal `terminal_height` rows tall: dialogs start
/// a sixth of the way down, and the status line, which says its keys, stays out from
/// under it.
pub fn popup_height(terminal_height: u16) -> u16 {
    let below_top = terminal_height.saturating_sub(terminal_height / 6 + 1);
    terminal_height.min(below_top.clamp(6, 22))
}

/// Rows inside the popup's border.
pub fn inner_rows(terminal_height: u16) -> usize {
    usize::from(popup_height(terminal_height).saturating_sub(2))
}

/// `text` cut from the left to fit `width` columns, so the end of a path -- the folder
/// the user is in -- is what stays.
fn keep_end(text: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if text.width() <= width {
        return text.to_string();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut used = 1;
    for ch in text.chars().rev() {
        let wide = ch.width().unwrap_or(0);
        if used + wide > width {
            break;
        }
        used += wide;
        kept.push(ch);
    }
    kept.push('…');
    kept.iter().rev().collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FilePickerFocus {
    #[default]
    List,
    Name,
    Submit,
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_parent: bool,
}

/// A write that would replace a file that is already there, waiting for the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct OverwriteConfirm {
    pub path: PathBuf,
    pub focus: FooterFocus,
}

impl OverwriteConfirm {
    pub fn lines(&self) -> Vec<String> {
        let name = self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        vec![
            format!("{name} already exists."),
            "Writing here replaces what is in it.".to_string(),
            String::new(),
            footer_line("Replace", self.focus),
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilePicker {
    pub open: bool,
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub offset: usize,
    pub show_hidden: bool,
    pub error: Option<String>,
    /// Set while the picker asks whether to replace the file it was about to write.
    pub confirm: Option<OverwriteConfirm>,
    pub name: TextInput,
    pub focus: FilePickerFocus,
    pub section: FilePickerSection,
    pub recent_selected: usize,
    pub recent_paths: Vec<PathBuf>,
    /// Open's way: folders and SQL files only, and what is typed finds among them -- or
    /// is a path to open. The other pickers type a file's name.
    pub finding: bool,
    /// The folder as read, before finding narrows it to `entries`.
    pub all: Vec<FileEntry>,
    /// The SQL files below the folder, read once finding starts there.
    pub below: Option<Vec<FileEntry>>,
}

/// Folders finding does not look into: a build's or a package manager's.
const SKIPPED: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "__pycache__",
];

/// The SQL files under `root`, by their path from it, as deep as `MAX_DEPTH` and as many
/// as `MAX_FOUND`: a home folder is not walked whole on a keystroke. ponytail: the limits
/// are fixed; a setting if a project nests its SQL deeper.
fn sql_below(root: &Path, show_hidden: bool) -> Vec<FileEntry> {
    const MAX_DEPTH: usize = 6;
    const MAX_FOUND: usize = 2000;
    let mut found = Vec::new();
    let mut folders = vec![(root.to_path_buf(), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(read) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if depth + 1 < MAX_DEPTH && !SKIPPED.contains(&name.as_str()) {
                    folders.push((path, depth + 1));
                }
            } else if is_sql(&name) && depth > 0 {
                let shown = path
                    .strip_prefix(root)
                    .map(|rest| rest.display().to_string())
                    .unwrap_or(name);
                found.push(FileEntry {
                    path,
                    name: shown,
                    is_dir: false,
                    is_parent: false,
                });
                if found.len() >= MAX_FOUND {
                    return found;
                }
            }
        }
    }
    found.sort_by_key(|entry| entry.name.to_lowercase());
    found
}

/// The files Open lists: SQL by its usual names.
fn is_sql(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".sql", ".psql", ".pgsql", ".mysql", ".ddl", ".dml"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Whether what was typed is a path to go to rather than words to find.
pub fn looks_like_path(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || text.starts_with('~')
}

/// A path as people read it: under the home folder, from `~`.
pub fn shown_path(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

impl Default for FilePicker {
    fn default() -> Self {
        Self {
            open: false,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            entries: Vec::new(),
            selected: 0,
            offset: 0,
            show_hidden: false,
            error: None,
            confirm: None,
            name: TextInput::default(),
            focus: FilePickerFocus::List,
            section: FilePickerSection::Browser,
            recent_selected: 0,
            recent_paths: Vec::new(),
            finding: false,
            all: Vec::new(),
            below: None,
        }
    }
}

impl FilePicker {
    pub fn open_browser(&mut self) {
        self.open_browser_with_recents(&[]);
    }

    pub fn open_browser_with_recents(&mut self, recent: &[PathBuf]) {
        self.open = true;
        self.confirm = None;
        self.name.clear();
        self.focus = FilePickerFocus::List;
        self.error = None;
        self.recent_paths = recent.iter().take(MAX_RECENT).cloned().collect();
        if self.recent_paths.is_empty() {
            self.section = FilePickerSection::Browser;
            self.recent_selected = 0;
        } else {
            self.section = FilePickerSection::Recent;
            self.recent_selected = 0;
        }
        self.refresh();
    }

    /// Drops the recent files that would leave the list fewer than a few rows in a popup
    /// `inner_rows` tall; on a short terminal the picker is the folder alone.
    pub fn fit_recents(&mut self, inner_rows: usize) {
        let room = inner_rows.saturating_sub(CHROME_ROWS + MIN_LIST_ROWS + 2);
        self.recent_paths.truncate(room);
        if self.recent_paths.is_empty() {
            self.section = FilePickerSection::Browser;
            self.recent_selected = 0;
        }
    }

    pub fn refresh(&mut self) {
        match read_entries(&self.cwd, self.show_hidden) {
            Ok(entries) => {
                self.all = entries;
                self.below = None;
                self.error = None;
                self.apply_find();
            }
            Err(error) => {
                self.all.clear();
                self.entries.clear();
                self.error = Some(error);
            }
        }
    }

    /// What finding looks for: the typed words, unless they are a path.
    fn needle(&self) -> Option<String> {
        let text = self.name.trim();
        (self.finding && !text.is_empty() && !looks_like_path(text)).then(|| text.to_lowercase())
    }

    /// The folder narrowed to what Open lists and what is typed, the first match picked:
    /// its folders and SQL files that have the words in their names, and the SQL files in
    /// the folders below that have them in their paths.
    pub fn apply_find(&mut self) {
        let needle = self.needle();
        if needle.is_some() && self.below.is_none() {
            self.below = Some(sql_below(&self.cwd, self.show_hidden));
        }
        let has = |entry: &FileEntry| {
            needle
                .as_deref()
                .is_none_or(|needle| entry.name.to_lowercase().contains(needle))
        };
        let below = needle
            .is_some()
            .then(|| self.below.iter().flatten().filter(|entry| has(entry)))
            .into_iter()
            .flatten();
        self.entries = self
            .all
            .iter()
            .filter(|entry| !self.finding || entry.is_dir || is_sql(&entry.name))
            .filter(|entry| entry.is_parent || has(entry))
            .chain(below)
            .cloned()
            .collect();
        self.selected = if needle.is_some() {
            self.entries
                .iter()
                .position(|entry| !entry.is_parent)
                .unwrap_or(0)
        } else {
            0
        };
        self.offset = 0;
        if needle.is_some() {
            self.section = FilePickerSection::Browser;
        }
    }

    pub fn parent(&mut self) {
        self.section = FilePickerSection::Browser;
        if self.finding {
            self.name.clear();
        }
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.refresh();
        } else if !drive_roots().is_empty() {
            self.cwd = PathBuf::new();
            self.refresh();
        }
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }

    pub fn enter_path(&mut self, path: PathBuf) -> Result<PathBuf, String> {
        let path = if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        };
        if path.is_dir() {
            self.cwd = path;
            self.refresh();
            return Err("directory selected".into());
        }
        Ok(path)
    }

    pub fn activate_selected(&mut self) -> Option<PathBuf> {
        if self.section == FilePickerSection::Recent {
            return self.recent_paths.get(self.recent_selected).cloned();
        }
        let entry = self.entries.get(self.selected)?.clone();
        if entry.is_dir {
            self.cwd = entry.path;
            if self.finding {
                self.name.clear();
            }
            self.refresh();
            None
        } else {
            // What Open typed was finding, not the file's name.
            if !self.finding {
                self.name.set_text(entry.name.clone());
            }
            Some(entry.path)
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.entries
            .get(self.selected)
            .map(|entry| entry.path.clone())
    }

    pub fn chosen_path(&self) -> Option<PathBuf> {
        let name = self.name.trim();
        // Finding, the words pick from the list; a path is the file.
        if self.finding && !name.is_empty() && !looks_like_path(name) {
            let entry = self.entries.get(self.selected)?;
            return (!entry.is_dir).then(|| entry.path.clone());
        }
        if self.finding && name.starts_with('~') {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            let rest = name.trim_start_matches('~').trim_start_matches(['/', '\\']);
            return Some(PathBuf::from(home).join(rest));
        }
        if !name.is_empty() {
            let path = Path::new(name);
            return Some(if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.cwd.join(name)
            });
        }
        if self.section == FilePickerSection::Recent {
            return self.recent_paths.get(self.recent_selected).cloned();
        }
        let entry = self.entries.get(self.selected)?;
        if entry.is_dir || entry.is_parent {
            None
        } else {
            Some(entry.path.clone())
        }
    }

    pub fn move_selection(&mut self, delta: i32, rows: usize) {
        if self.section == FilePickerSection::Recent {
            self.move_recent(delta);
            return;
        }
        if self.entries.is_empty() {
            return;
        }
        let next = (self.selected as i32 + delta).clamp(0, self.entries.len() as i32 - 1) as usize;
        self.selected = next;
        self.offset = crate::palette::scroll_to_selection(
            self.selected,
            self.offset,
            self.entries.len(),
            rows.max(1),
        );
    }

    fn move_recent(&mut self, delta: i32) {
        if self.recent_paths.is_empty() {
            return;
        }
        if delta > 0 && self.recent_selected + 1 >= self.recent_paths.len() {
            self.section = FilePickerSection::Browser;
            self.selected = 0;
            self.offset = 0;
            return;
        }
        if delta < 0 && self.recent_selected == 0 {
            return;
        }
        let next = (self.recent_selected as i32 + delta)
            .clamp(0, self.recent_paths.len() as i32 - 1) as usize;
        self.recent_selected = next;
    }

    fn move_browser_up(&mut self, rows: usize) {
        if self.selected == 0 && !self.recent_paths.is_empty() {
            self.section = FilePickerSection::Recent;
            self.recent_selected = self.recent_paths.len().saturating_sub(1);
            return;
        }
        self.move_selection(-1, rows);
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            FilePickerFocus::List => FilePickerFocus::Name,
            FilePickerFocus::Name => FilePickerFocus::Submit,
            FilePickerFocus::Submit => FilePickerFocus::Cancel,
            FilePickerFocus::Cancel => FilePickerFocus::List,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            FilePickerFocus::List => FilePickerFocus::Cancel,
            FilePickerFocus::Name => FilePickerFocus::List,
            FilePickerFocus::Submit => FilePickerFocus::Name,
            FilePickerFocus::Cancel => FilePickerFocus::Submit,
        };
    }

    pub fn move_down(&mut self, rows: usize) {
        match self.focus {
            FilePickerFocus::List if self.section == FilePickerSection::Recent => {
                self.move_recent(1);
            }
            FilePickerFocus::List
                if self.section == FilePickerSection::Browser
                    && self.selected + 1 >= self.entries.len() =>
            {
                self.focus = FilePickerFocus::Name;
            }
            FilePickerFocus::List => self.move_selection(1, rows),
            FilePickerFocus::Name => self.focus = FilePickerFocus::Submit,
            FilePickerFocus::Submit => self.focus = FilePickerFocus::Cancel,
            FilePickerFocus::Cancel => {}
        }
    }

    pub fn move_up(&mut self, rows: usize) {
        match self.focus {
            FilePickerFocus::List if self.section == FilePickerSection::Recent => {
                self.move_recent(-1);
            }
            FilePickerFocus::List
                if self.section == FilePickerSection::Browser && self.selected == 0 =>
            {
                self.move_browser_up(rows);
            }
            FilePickerFocus::List => self.move_selection(-1, rows),
            FilePickerFocus::Name => {
                if self.recent_paths.is_empty() {
                    self.focus = FilePickerFocus::List;
                } else {
                    self.section = FilePickerSection::Recent;
                    self.recent_selected = self.recent_paths.len().saturating_sub(1);
                    self.focus = FilePickerFocus::List;
                }
            }
            FilePickerFocus::Submit => self.focus = FilePickerFocus::Name,
            FilePickerFocus::Cancel => self.focus = FilePickerFocus::Submit,
        }
    }

    pub fn footer_left(&mut self) {
        if self.focus == FilePickerFocus::Cancel {
            self.focus = FilePickerFocus::Submit;
        }
    }

    pub fn footer_right(&mut self) {
        if self.focus == FilePickerFocus::Submit {
            self.focus = FilePickerFocus::Cancel;
        }
    }

    pub fn footer_focus(&self) -> FooterFocus {
        match self.focus {
            FilePickerFocus::Submit => FooterFocus::Submit,
            FilePickerFocus::Cancel => FooterFocus::Cancel,
            _ => FooterFocus::Input,
        }
    }

    /// File rows the popup has room for, `inner_rows` being what is inside its border.
    /// The key handling scrolls by this too, so the two cannot disagree.
    pub fn browser_rows(&self, mode: FilePickerMode, inner_rows: usize) -> usize {
        let recent = if mode == FilePickerMode::Open && !self.recent_paths.is_empty() {
            self.recent_paths.len() + 2
        } else {
            0
        };
        let error = usize::from(self.error.is_some());
        inner_rows
            .saturating_sub(CHROME_ROWS + recent + error)
            .max(1)
    }

    /// The popup's lines for `inner_rows` x `inner_width` cells: where it is, what is
    /// typed, the recent files, the folder, the buttons. Everything but the folder has a
    /// fixed size, so the buttons stay put; its keys are on the status line.
    pub fn layout(
        &self,
        mode: FilePickerMode,
        inner_rows: usize,
        inner_width: usize,
    ) -> FilePickerLayout {
        let finding = self.needle().is_some();
        let show_recent = mode == FilePickerMode::Open && !self.recent_paths.is_empty() && !finding;
        let mut lines = Vec::new();
        let mut kinds = Vec::new();

        // The end of the path is the folder the user is in: a long one is cut from the
        // left, not the right.
        let cwd = if self.cwd.as_os_str().is_empty() {
            "Drives".into()
        } else {
            format!(
                "In {}",
                keep_end(&shown_path(&self.cwd), inner_width.saturating_sub(3))
            )
        };
        lines.push(cwd);
        kinds.push(FilePickerLineKind::Cwd);
        // Always typed into, so it has no mark of its own: the list's is the one.
        if self.finding {
            lines.push(self.name.inline_line_within(
                FIND_LABEL,
                !matches!(
                    self.focus,
                    FilePickerFocus::Submit | FilePickerFocus::Cancel
                ),
                inner_width,
            ));
            kinds.push(FilePickerLineKind::Name);
        }

        if show_recent {
            lines.push("Recent".into());
            kinds.push(FilePickerLineKind::RecentHeader);
            for (index, path) in self.recent_paths.iter().enumerate() {
                let marker = if self.section == FilePickerSection::Recent
                    && index == self.recent_selected
                    && self.focus == FilePickerFocus::List
                {
                    ">"
                } else {
                    " "
                };
                let label = recent_label(path);
                lines.push(crate::model::truncate_cell(
                    &format!("{marker} {label}"),
                    inner_width,
                ));
                kinds.push(FilePickerLineKind::RecentItem(index));
            }
            lines.push("This folder".into());
            kinds.push(FilePickerLineKind::BrowseHeader);
        }

        let browser_rows = self.browser_rows(mode, inner_rows);
        let browser_offset = crate::palette::scroll_to_selection(
            self.selected,
            self.offset,
            self.entries.len(),
            browser_rows,
        );
        for (index, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(browser_offset)
            .take(browser_rows)
        {
            let marker = if self.section == FilePickerSection::Browser
                && index == self.selected
                && self.focus == FilePickerFocus::List
            {
                ">"
            } else {
                " "
            };
            // A folder ends in its separator, as a shell lists it.
            let name = if entry.is_dir && !entry.name.ends_with(['/', '\\']) {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            lines.push(format!("{marker} {name}"));
            kinds.push(FilePickerLineKind::BrowserEntry(index));
        }
        let shown = self
            .entries
            .len()
            .saturating_sub(browser_offset)
            .min(browser_rows);
        let mut padding = browser_rows.saturating_sub(shown);
        // Nothing found says so where the files would be.
        if finding && !self.entries.iter().any(|entry| !entry.is_parent) && padding > 0 {
            lines.push(format!(
                "  No SQL file here or below has \"{}\" in its name.",
                self.name.trim()
            ));
            kinds.push(FilePickerLineKind::Padding);
            padding -= 1;
        }
        // A short folder leaves the rows it does not use empty, so the buttons stay where
        // they are whatever the folder holds.
        for _ in 0..padding {
            lines.push(String::new());
            kinds.push(FilePickerLineKind::Padding);
        }

        if !self.finding {
            lines.push(self.name.labeled_line_within(
                NAME_LABEL,
                self.focus == FilePickerFocus::Name,
                inner_width,
            ));
            kinds.push(FilePickerLineKind::Name);
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
            kinds.push(FilePickerLineKind::Error);
        }
        lines.push(footer_line(mode.submit_label(), self.footer_focus()));
        kinds.push(FilePickerLineKind::Footer);

        FilePickerLayout {
            lines,
            kinds,
            browser_offset,
            browser_rows,
        }
    }

    /// The field's label, as drawn before what is typed.
    pub fn field_label(&self) -> &'static str {
        if self.finding { FIND_LABEL } else { NAME_LABEL }
    }

    pub fn lines(&self, mode: FilePickerMode, inner_rows: usize) -> Vec<String> {
        self.layout(mode, inner_rows, 70).lines
    }

    /// Moves the file list by a page, whichever part of the dialog has the focus.
    pub fn page(&mut self, pages: i32, rows: usize) {
        self.move_selection(pages * rows.max(1) as i32, rows);
    }

    /// First or last entry of the list.
    pub fn jump_to_end(&mut self, last: bool, rows: usize) {
        if self.section == FilePickerSection::Recent {
            if last {
                self.section = FilePickerSection::Browser;
            } else {
                self.recent_selected = 0;
                return;
            }
        }
        let target = if last {
            self.entries.len().saturating_sub(1)
        } else {
            0
        };
        self.select_browser(target, rows);
    }

    pub fn select_recent(&mut self, index: usize) {
        if index < self.recent_paths.len() {
            self.section = FilePickerSection::Recent;
            self.recent_selected = index;
            self.focus = FilePickerFocus::List;
        }
    }

    pub fn select_browser(&mut self, index: usize, rows: usize) {
        if index < self.entries.len() {
            self.section = FilePickerSection::Browser;
            self.selected = index;
            self.offset = crate::palette::scroll_to_selection(
                self.selected,
                self.offset,
                self.entries.len(),
                rows.max(1),
            );
            self.focus = FilePickerFocus::List;
        }
    }
}

/// A recent file: its name, then the folder it is in.
fn recent_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => {
            format!("{name}  {}", shown_path(folder))
        }
        _ => name,
    }
}

pub fn read_entries(dir: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    if dir.as_os_str().is_empty() {
        return Ok(drive_roots()
            .into_iter()
            .map(|path| FileEntry {
                name: path.display().to_string(),
                is_dir: true,
                is_parent: false,
                path,
            })
            .collect());
    }
    let mut entries = Vec::new();
    if let Some(parent) = dir.parent() {
        entries.push(FileEntry {
            path: parent.to_path_buf(),
            name: "..".into(),
            is_dir: true,
            is_parent: true,
        });
    } else {
        for path in drive_roots() {
            if path != dir {
                entries.push(FileEntry {
                    name: path.display().to_string(),
                    is_dir: true,
                    is_parent: false,
                    path,
                });
            }
        }
    }
    for entry in std::fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let is_dir = path.is_dir();
        entries.push(FileEntry {
            path,
            name,
            is_dir,
            is_parent: false,
        });
    }
    entries.sort_by(|left, right| match (left.is_parent, right.is_parent) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => match (left.is_dir, right.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left
                .name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase()),
        },
    });
    Ok(entries)
}

#[cfg(windows)]
pub fn drive_roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let path = PathBuf::from(format!("{}:\\", letter as char));
            path.exists().then_some(path)
        })
        .collect()
}

#[cfg(not(windows))]
pub fn drive_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

#[cfg(test)]
mod tests {
    use super::{FilePicker, FilePickerFocus, FilePickerMode, drive_roots, read_entries};
    use crate::widgets::text_input::TextInput;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn parent_hidden_absolute_and_inaccessible() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), b"ok").unwrap();
        std::fs::write(dir.path().join(".secret"), b"no").unwrap();
        let child = dir.path().join("sub");
        std::fs::create_dir(&child).unwrap();
        let mut picker = FilePicker {
            cwd: child.clone(),
            ..FilePicker::default()
        };
        picker.parent();
        assert_eq!(picker.cwd, dir.path());
        picker.refresh();
        assert!(
            picker
                .entries
                .iter()
                .any(|entry| entry.path.ends_with("visible.txt"))
        );
        assert!(
            !picker
                .entries
                .iter()
                .any(|entry| entry.path.ends_with(".secret"))
        );
        picker.toggle_hidden();
        assert!(
            picker
                .entries
                .iter()
                .any(|entry| entry.path.ends_with(".secret"))
        );
        let abs = picker.enter_path(dir.path().join("visible.txt")).unwrap();
        assert!(abs.is_absolute());
        assert!(!drive_roots().is_empty());
        let missing = read_entries(&dir.path().join("nope"), false);
        assert!(missing.is_err());
    }

    #[test]
    fn arrows_scroll_past_the_visible_window() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("f{i:02}.txt")), b"x").unwrap();
        }
        let mut picker = FilePicker {
            cwd: dir.path().to_path_buf(),
            ..FilePicker::default()
        };
        picker.refresh();
        assert!(picker.entries.len() >= 20);
        for _ in 0..15 {
            picker.move_selection(1, 8);
        }
        assert!(picker.selected >= 15);
        assert!(picker.offset > 0);
        assert!(picker.selected >= picker.offset);
        assert!(picker.selected < picker.offset + 8);
    }

    #[test]
    fn browser_lists_parent_dirs_first_short_names_and_actions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("z-file.txt"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("a-dir")).unwrap();
        let mut picker = FilePicker {
            cwd: dir.path().to_path_buf(),
            ..FilePicker::default()
        };
        picker.refresh();
        assert_eq!(picker.entries[0].name, "..");
        assert!(picker.entries[0].is_parent);
        let names: Vec<&str> = picker
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        let dir_at = names.iter().position(|name| *name == "a-dir").unwrap();
        let file_at = names.iter().position(|name| *name == "z-file.txt").unwrap();
        assert!(dir_at < file_at);
        assert!(picker.entries.iter().all(|entry| {
            entry.is_parent || (!entry.name.contains('/') && !entry.name.contains('\\'))
        }));
        picker.name.set_text("out.sql");
        assert_eq!(picker.chosen_path(), Some(dir.path().join("out.sql")));
        let lines = picker.lines(FilePickerMode::Save, 12);
        assert!(lines.iter().any(|line| line.contains(" a-dir/")));
        assert!(lines.iter().any(|line| line.contains(" z-file.txt")));
        assert!(lines.iter().any(|line| line.contains("[Save]")));
        assert!(lines.iter().any(|line| line.contains("[Cancel]")));
        picker.focus = FilePickerFocus::Cancel;
        assert!(
            picker
                .lines(FilePickerMode::Save, 12)
                .iter()
                .any(|line| line.contains(">[Cancel]"))
        );
    }

    #[test]
    fn arrow_and_tab_navigation_reaches_save_and_cancel() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.sql"), b"x").unwrap();
        let mut picker = FilePicker {
            cwd: dir.path().to_path_buf(),
            ..FilePicker::default()
        };
        picker.refresh();
        assert_eq!(picker.focus, FilePickerFocus::List);
        picker.selected = picker.entries.len().saturating_sub(1);

        picker.move_down(8);
        assert_eq!(picker.focus, FilePickerFocus::Name);

        picker.move_down(8);
        assert_eq!(picker.focus, FilePickerFocus::Submit);
        assert!(
            picker
                .lines(FilePickerMode::Save, 8)
                .iter()
                .any(|line| line.contains(">[Save]"))
        );

        picker.move_down(8);
        assert_eq!(picker.focus, FilePickerFocus::Cancel);

        picker.footer_left();
        assert_eq!(picker.focus, FilePickerFocus::Submit);

        picker.focus_prev();
        assert_eq!(picker.focus, FilePickerFocus::Name);

        picker.focus_next();
        assert_eq!(picker.focus, FilePickerFocus::Submit);
    }

    #[test]
    fn open_mode_lists_recent_files_before_browser() {
        let dir = tempfile::tempdir().unwrap();
        let recent = vec![
            dir.path().join("recent-a.sql"),
            dir.path().join("recent-b.sql"),
        ];
        let mut picker = FilePicker {
            cwd: dir.path().to_path_buf(),
            ..FilePicker::default()
        };
        picker.open_browser_with_recents(&recent);
        let lines = picker.lines(FilePickerMode::Open, 8);
        assert!(lines.iter().any(|line| line == "Recent"));
        assert!(lines.iter().any(|line| line.contains("recent-a.sql")));
        assert!(lines.iter().any(|line| line == "This folder"));
        assert_eq!(picker.section, super::FilePickerSection::Recent);
        assert_eq!(picker.chosen_path(), Some(recent[0].clone()));
    }

    #[test]
    fn recent_navigation_moves_into_browser() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.sql"), b"x").unwrap();
        let recent = vec![dir.path().join("recent.sql")];
        let mut picker = FilePicker {
            cwd: dir.path().to_path_buf(),
            ..FilePicker::default()
        };
        picker.open_browser_with_recents(&recent);
        picker.move_down(8);
        assert_eq!(picker.section, super::FilePickerSection::Browser);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn name_field_supports_word_navigation() {
        let mut picker = FilePicker {
            focus: FilePickerFocus::Name,
            name: TextInput::new("query-1.sql"),
            ..FilePicker::default()
        };
        picker
            .name
            .handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        picker
            .name
            .handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(picker.name.cursor(), "query-1.".chars().count());
    }

    /// Ctrl+A and Ctrl+W never reached the name: only the arrows, Backspace, Delete
    /// and plain letters did.
    #[test]
    fn the_name_takes_ctrl_a_and_ctrl_w() {
        let mut model = crate::model::Model {
            file_picker: FilePicker {
                open: true,
                focus: FilePickerFocus::Name,
                name: TextInput::new("monthly sales.sql"),
                ..FilePicker::default()
            },
            ..crate::model::Model::default()
        };
        let ctrl = |ch| {
            crate::action::Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
        };
        crate::update::update(&mut model, ctrl('w'));
        assert_eq!(model.file_picker.name.as_str(), "monthly sales.");
        crate::update::update(&mut model, ctrl('a'));
        assert!(model.file_picker.name.is_selected());
        crate::update::update(
            &mut model,
            crate::action::Action::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        assert_eq!(model.file_picker.name.as_str(), "x");
    }
}
