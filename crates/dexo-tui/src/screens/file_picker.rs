use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct FilePicker {
    pub open: bool,
    pub cwd: PathBuf,
    pub entries: Vec<PathBuf>,
    pub selected: usize,
    pub show_hidden: bool,
    pub error: Option<String>,
    pub overwrite: bool,
}

impl Default for FilePicker {
    fn default() -> Self {
        Self {
            open: false,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            entries: Vec::new(),
            selected: 0,
            show_hidden: false,
            error: None,
            overwrite: false,
        }
    }
}

impl FilePicker {
    pub fn refresh(&mut self) {
        match read_entries(&self.cwd, self.show_hidden) {
            Ok(entries) => {
                self.entries = entries;
                self.selected = 0;
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

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.entries.get(self.selected).cloned()
    }
}

pub fn read_entries(dir: &Path, show_hidden: bool) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        entries.push(path);
    }
    entries.sort();
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
    use super::{FilePicker, drive_roots, read_entries};

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
                .any(|path| path.ends_with("visible.txt"))
        );
        assert!(!picker.entries.iter().any(|path| path.ends_with(".secret")));
        picker.toggle_hidden();
        assert!(picker.entries.iter().any(|path| path.ends_with(".secret")));
        let abs = picker.enter_path(dir.path().join("visible.txt")).unwrap();
        assert!(abs.is_absolute());
        assert!(!drive_roots().is_empty());
        let missing = read_entries(&dir.path().join("nope"), false);
        assert!(missing.is_err());
    }
}
