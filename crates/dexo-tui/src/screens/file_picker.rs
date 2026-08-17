use std::path::{Path, PathBuf};

use crate::widgets::form::{FooterFocus, footer_line};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FilePickerMode {
    #[default]
    Open,
    Save,
    Transfer,
    Diagnostics,
}

impl FilePickerMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Open => "Open file",
            Self::Save => "Save file",
            Self::Transfer => "Choose path",
            Self::Diagnostics => "Save diagnostics",
        }
    }

    pub fn submit_label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Save | Self::Diagnostics => "Save",
            Self::Transfer => "Choose",
        }
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct FilePicker {
    pub open: bool,
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub offset: usize,
    pub show_hidden: bool,
    pub error: Option<String>,
    pub overwrite: bool,
    pub name: String,
    pub focus: FilePickerFocus,
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
            overwrite: false,
            name: String::new(),
            focus: FilePickerFocus::List,
        }
    }
}

impl FilePicker {
    pub fn open_browser(&mut self) {
        self.open = true;
        self.name.clear();
        self.focus = FilePickerFocus::List;
        self.error = None;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        match read_entries(&self.cwd, self.show_hidden) {
            Ok(entries) => {
                self.entries = entries;
                self.selected = 0;
                self.offset = 0;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(error);
            }
        }
    }

    pub fn parent(&mut self) {
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
        let entry = self.entries.get(self.selected)?.clone();
        if entry.is_dir {
            self.cwd = entry.path;
            self.refresh();
            None
        } else {
            self.name = entry.name.clone();
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
        if !name.is_empty() {
            let path = Path::new(name);
            return Some(if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.cwd.join(name)
            });
        }
        let entry = self.entries.get(self.selected)?;
        if entry.is_dir || entry.is_parent {
            None
        } else {
            Some(entry.path.clone())
        }
    }

    pub fn jump_to(&mut self, prefix: char, rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        let needle = prefix.to_ascii_lowercase();
        let start = self.selected + 1;
        let found = (0..self.entries.len()).find_map(|step| {
            let index = (start + step) % self.entries.len();
            let name = self.entries[index].name.to_ascii_lowercase();
            name.starts_with(needle).then_some(index)
        });
        if let Some(index) = found {
            self.selected = index;
            self.offset = crate::palette::scroll_to_selection(
                self.selected,
                self.offset,
                self.entries.len(),
                rows.max(1),
            );
        }
    }

    pub fn move_selection(&mut self, delta: i32, rows: usize) {
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

    pub fn footer_focus(&self) -> FooterFocus {
        match self.focus {
            FilePickerFocus::Submit => FooterFocus::Submit,
            FilePickerFocus::Cancel => FooterFocus::Cancel,
            _ => FooterFocus::Input,
        }
    }

    pub fn lines(&self, mode: FilePickerMode, rows: usize) -> Vec<String> {
        let cwd = if self.cwd.as_os_str().is_empty() {
            "Drives".into()
        } else {
            self.cwd.display().to_string()
        };
        let mut lines = vec![cwd];
        let offset = crate::palette::scroll_to_selection(
            self.selected,
            self.offset,
            self.entries.len(),
            rows.max(1),
        );
        for (index, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(offset)
            .take(rows.max(1))
        {
            let marker = if index == self.selected && self.focus == FilePickerFocus::List {
                ">"
            } else {
                " "
            };
            let kind = if entry.is_dir { "/" } else { " " };
            lines.push(format!("{marker}{kind} {}", entry.name));
        }
        let name_mark = if self.focus == FilePickerFocus::Name {
            ">"
        } else {
            " "
        };
        lines.push(format!("{name_mark} name: {}", self.name));
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line(mode.submit_label(), self.footer_focus()));
        lines
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
        picker.name = "out.sql".into();
        assert_eq!(picker.chosen_path(), Some(dir.path().join("out.sql")));
        let lines = picker.lines(FilePickerMode::Save, 12);
        assert!(lines.iter().any(|line| line.contains("/ a-dir")));
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
}
