use std::collections::HashMap;
use std::path::PathBuf;

use dexo_storage::{ImportPreview, ImportResolution};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConfigTransferMode {
    #[default]
    Closed,
    Export,
    Import,
}

/// The step the dialog is at, read off what it holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigStage {
    /// Nothing chosen yet: Export, Import, Close.
    Start,
    /// A file was read: what importing it would do, to be confirmed.
    Preview,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigTransferScreen {
    pub open: bool,
    pub mode: ConfigTransferMode,
    pub path: PathBuf,
    pub preview: Option<ImportPreview>,
    pub resolutions: HashMap<String, ImportResolution>,
    pub needing_secret: Vec<String>,
    /// The commands the imported connections run on this machine.
    pub commands: Vec<String>,
    /// What the last export or import did, in a sentence.
    pub message: Option<String>,
    /// The conflicting connection picked in the preview.
    pub selected: usize,
    /// Lines the preview's body is scrolled down past the picked clash: Down on the last
    /// one goes on to the passwords and commands under the list.
    pub scroll: usize,
    /// How far the body could be scrolled when last drawn.
    pub max_scroll: std::cell::Cell<usize>,
    /// The focused button of the row shown.
    pub focus: usize,
}

/// One line of the preview's body, and which conflict it is when it is one.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyLine {
    pub text: String,
    pub conflict: Option<usize>,
}

impl ConfigTransferScreen {
    pub fn with_resolution(mut self, name: &str, resolution: ImportResolution) -> Self {
        self.resolutions.insert(name.to_string(), resolution);
        self
    }

    /// The dialog as opened: nothing of an earlier import or export left on it.
    pub fn reset(&mut self) {
        *self = Self {
            open: true,
            ..Self::default()
        };
    }

    pub fn stage(&self) -> ConfigStage {
        if self.preview.is_some() {
            ConfigStage::Preview
        } else {
            ConfigStage::Start
        }
    }

    /// The buttons of the row, in order.
    pub fn buttons(&self) -> &'static [&'static str] {
        match self.stage() {
            ConfigStage::Start => &["Export", "Import", "Close"],
            ConfigStage::Preview => &["Import", "Cancel"],
        }
    }

    pub fn focus_next(&mut self) {
        self.focus = (self.focus + 1) % self.buttons().len();
    }

    pub fn focus_prev(&mut self) {
        let count = self.buttons().len();
        self.focus = (self.focus + count - 1) % count;
    }

    pub fn resolution(&self, name: &str) -> ImportResolution {
        self.resolutions.get(name).cloned().unwrap_or_default()
    }

    /// The name a clashing connection is renamed to: `name-2`, or the first of `name-3`,
    /// `name-4`... that nothing saved or incoming already goes by.
    pub fn rename_target(&self, name: &str) -> String {
        let taken = |candidate: &str| {
            self.preview.as_ref().is_some_and(|preview| {
                preview.existing.iter().any(|existing| existing == candidate)
                    || preview.conflicts.iter().any(|clash| clash == candidate)
            }) || self.resolutions.values().any(|resolution| {
                matches!(resolution, ImportResolution::Rename(other) if other == candidate)
            })
        };
        (2..)
            .map(|n| format!("{name}-{n}"))
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| format!("{name}-2"))
    }

    /// Skip, then overwrite, then rename, then skip again.
    pub fn cycle_selected(&mut self) {
        let Some(name) = self
            .preview
            .as_ref()
            .and_then(|preview| preview.conflicts.get(self.selected))
            .cloned()
        else {
            return;
        };
        let next = match self.resolution(&name) {
            ImportResolution::Skip => ImportResolution::Replace,
            ImportResolution::Replace => ImportResolution::Rename(self.rename_target(&name)),
            ImportResolution::Rename(_) => ImportResolution::Skip,
        };
        self.resolutions.insert(name, next);
    }

    /// Sets the picked conflict's resolution; a rename takes a free name.
    pub fn resolve_selected(&mut self, resolution: ImportResolution) {
        let Some(name) = self
            .preview
            .as_ref()
            .and_then(|preview| preview.conflicts.get(self.selected))
            .cloned()
        else {
            return;
        };
        let resolution = match resolution {
            ImportResolution::Rename(_) => ImportResolution::Rename(self.rename_target(&name)),
            other => other,
        };
        self.resolutions.insert(name, resolution);
    }

    pub fn select(&mut self, delta: isize) {
        let count = self
            .preview
            .as_ref()
            .map_or(0, |preview| preview.conflicts.len());
        let max = self.max_scroll.get();
        if count == 0 {
            self.scroll = self.scroll.saturating_add_signed(delta).min(max);
            return;
        }
        if delta > 0 && self.selected + 1 >= count {
            self.scroll = self.scroll.saturating_add(delta as usize).min(max);
        } else if delta < 0 && self.scroll > 0 {
            self.scroll = self.scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
            self.scroll = 0;
        }
    }

    /// What was imported, in a sentence: how many, and what became of those that clashed.
    pub fn summary(&self) -> String {
        let Some(preview) = &self.preview else {
            return String::new();
        };
        let (mut replaced, mut renamed, mut skipped) = (0, 0, 0);
        for name in &preview.conflicts {
            match self.resolution(name) {
                ImportResolution::Replace => replaced += 1,
                ImportResolution::Rename(_) => renamed += 1,
                ImportResolution::Skip => skipped += 1,
            }
        }
        let added = preview.incoming.len() - preview.conflicts.len();
        let mut parts = vec![format!("{added} new")];
        for (count, what) in [
            (renamed, "renamed"),
            (replaced, "overwritten"),
            (skipped, "skipped"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {what}"));
            }
        }
        format!(
            "Imported {} connection{}: {}.",
            added + renamed + replaced,
            if added + renamed + replaced == 1 {
                ""
            } else {
                "s"
            },
            parts.join(", ")
        )
    }

    fn resolution_words(&self, name: &str) -> String {
        match self.resolution(name) {
            ImportResolution::Skip => "skip it".into(),
            ImportResolution::Replace => "overwrite the saved one".into(),
            ImportResolution::Rename(new) => format!("keep both, import it as {new}"),
        }
    }

    /// The preview's body: the clashes, then what needs a password, then the commands the
    /// file would have this machine run. Wrapped to `width`, whole: the dialog scrolls it.
    pub fn body(&self, width: usize) -> Vec<BodyLine> {
        let Some(preview) = &self.preview else {
            return Vec::new();
        };
        let wrap = |text: &str, first: &str, rest: &str| -> Vec<BodyLine> {
            let room = width.saturating_sub(first.chars().count()).max(8);
            crate::model::wrap_words(text, room)
                .into_iter()
                .enumerate()
                .map(|(index, part)| BodyLine {
                    text: format!("{}{part}", if index == 0 { first } else { rest }),
                    conflict: None,
                })
                .collect()
        };
        let mut body = Vec::new();
        if preview.conflicts.is_empty() {
            body.extend(wrap(
                "No connection in the file has the name of a saved one.",
                "",
                "",
            ));
        } else {
            body.extend(wrap(
                "These names are saved already (Space changes what happens to the picked one):",
                "",
                "",
            ));
            for (index, name) in preview.conflicts.iter().enumerate() {
                let marker = if index == self.selected { "> " } else { "  " };
                let mut lines = wrap(
                    &format!("{name}: {}", self.resolution_words(name)),
                    marker,
                    "    ",
                );
                for line in &mut lines {
                    line.conflict = Some(index);
                }
                body.extend(lines);
            }
        }
        if !preview.connections_needing_secret.is_empty() {
            body.push(BodyLine {
                text: String::new(),
                conflict: None,
            });
            body.extend(wrap(
                &format!(
                    "These need their password typed when you first connect: {}.",
                    preview.connections_needing_secret.join(", ")
                ),
                "",
                "",
            ));
        }
        // A shared file's commands run on this machine: shown whole before they are kept.
        if !preview.commands.is_empty() {
            body.push(BodyLine {
                text: String::new(),
                conflict: None,
            });
            body.extend(wrap(
                "These run on this machine when the connections connect -- read them before you import:",
                "",
                "",
            ));
            for command in &preview.commands {
                body.extend(wrap(command, "  ", "    "));
            }
        }
        body
    }

    /// The dialog's lines for a body `rows` tall and `width` wide: the header and the
    /// buttons stay, the body scrolls to keep the picked clash in view. Returns the lines
    /// and where the conflict rows start, so the mouse can be pointed at them.
    pub fn view(&self, width: usize, rows: usize) -> ConfigView {
        let mut lines = Vec::new();
        let mut conflict_rows = Vec::new();
        let button_row = |focus: usize, labels: &[&str]| {
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    format!("{}[{label}]", if index == focus { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("  ")
        };
        let push_wrapped = |lines: &mut Vec<String>, text: &str| {
            lines.extend(crate::model::wrap_words(text, width.max(8)));
        };
        match self.stage() {
            ConfigStage::Start => {
                push_wrapped(
                    &mut lines,
                    "Export saves your connections and projects, without passwords, to a TOML file. Import reads one back.",
                );
                if let Some(message) = &self.message {
                    lines.push(String::new());
                    push_wrapped(&mut lines, message);
                }
                if !self.needing_secret.is_empty() {
                    push_wrapped(
                        &mut lines,
                        &format!(
                            "They need their password typed when you first connect: {}.",
                            self.needing_secret.join(", ")
                        ),
                    );
                }
                if !self.commands.is_empty() {
                    lines.push(String::new());
                    push_wrapped(
                        &mut lines,
                        "These run on this machine when the connections connect:",
                    );
                    for command in &self.commands {
                        push_wrapped(&mut lines, &format!("  {command}"));
                    }
                }
                lines.push(String::new());
                lines.push(button_row(self.focus, self.buttons()));
                lines.push("e export  i import  Left/Right pick  Enter run  Esc close".into());
            }
            ConfigStage::Preview => {
                push_wrapped(&mut lines, &format!("Import from {}", self.path.display()));
                if let Some(message) = &self.message {
                    push_wrapped(&mut lines, message);
                }
                lines.push(String::new());
                let header = lines.len();
                let body = self.body(width);
                let room = rows.saturating_sub(header + 3).max(1);
                let focus_line = body
                    .iter()
                    .position(|line| line.conflict == Some(self.selected))
                    .unwrap_or(0);
                let follow = if self
                    .preview
                    .as_ref()
                    .is_some_and(|preview| !preview.conflicts.is_empty())
                {
                    crate::palette::scroll_to_selection(focus_line, 0, body.len(), room)
                } else {
                    0
                };
                let max = body.len().saturating_sub(room);
                self.max_scroll.set(max.saturating_sub(follow));
                let offset = (follow + self.scroll).min(max);
                for line in body.iter().skip(offset).take(room) {
                    if let Some(conflict) = line.conflict {
                        conflict_rows.push((lines.len(), conflict));
                    }
                    lines.push(line.text.clone());
                }
                if body.len() > offset + room {
                    lines.push(format!(
                        "  ... {} more below (Down)",
                        body.len() - offset - room
                    ));
                } else {
                    lines.push(String::new());
                }
                lines.push(button_row(self.focus, self.buttons()));
                lines.push("Up/Down pick  Space change  Enter import  Esc cancel".into());
            }
        }
        ConfigView {
            lines,
            conflict_rows,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigView {
    pub lines: Vec<String>,
    /// For each conflict row drawn: its line, and which conflict it is.
    pub conflict_rows: Vec<(usize, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(conflicts: &[&str], incoming: usize) -> ImportPreview {
        let mut names: Vec<String> = conflicts.iter().map(|name| name.to_string()).collect();
        names.extend((conflicts.len()..incoming).map(|n| format!("new-{n}")));
        ImportPreview {
            conflicts: conflicts.iter().map(|name| name.to_string()).collect(),
            incoming: names,
            existing: conflicts.iter().map(|name| name.to_string()).collect(),
            connections_needing_secret: vec!["my-new".into()],
            commands: vec!["my-new runs `touch /tmp/x` before it connects".into()],
        }
    }

    #[test]
    fn every_conflict_reads_as_words_and_the_warnings_are_whole() {
        let screen = ConfigTransferScreen {
            preview: Some(preview(&["a", "b", "c", "d", "e", "f", "g", "h"], 9)),
            ..ConfigTransferScreen::default()
        };
        let text = screen
            .body(60)
            .into_iter()
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("> a: skip it"), "{text}");
        assert!(!text.contains("Rename("), "no Debug text: {text}");
        assert!(text.contains("h: skip it"), "all rows: {text}");
        assert!(
            text.contains("touch /tmp/x"),
            "the command is shown: {text}"
        );
        assert!(text.contains("my-new"), "{text}");
    }

    #[test]
    fn a_rename_takes_a_name_nothing_has() {
        let mut screen = ConfigTransferScreen::default();
        let mut preview = preview(&["a", "b"], 2);
        preview.existing.push("a-2".into());
        screen.preview = Some(preview);
        screen.resolve_selected(ImportResolution::Rename(String::new()));
        assert_eq!(
            screen.resolution("a"),
            ImportResolution::Rename("a-3".into())
        );
        screen.selected = 1;
        screen.resolve_selected(ImportResolution::Rename(String::new()));
        assert_eq!(
            screen.resolution("b"),
            ImportResolution::Rename("b-2".into())
        );
    }

    #[test]
    fn the_preview_scrolls_to_the_picked_clash_and_keeps_the_buttons() {
        let mut screen = ConfigTransferScreen::default();
        let names: Vec<String> = (0..30).map(|n| format!("conn-{n:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        screen.preview = Some(preview(&refs, 30));
        screen.selected = 25;
        let view = screen.view(60, 14);
        assert!(view.lines.len() <= 14, "{}", view.lines.len());
        let text = view.lines.join("\n");
        assert!(text.contains("> conn-25"), "{text}");
        assert!(
            text.contains("[Import]") && text.contains("[Cancel]"),
            "{text}"
        );
    }

    #[test]
    fn the_summary_counts_what_became_of_each_connection() {
        let mut screen = ConfigTransferScreen {
            preview: Some(preview(&["a", "b", "c"], 5)),
            ..ConfigTransferScreen::default()
        };
        screen.resolve_selected(ImportResolution::Replace);
        screen.selected = 1;
        screen.resolve_selected(ImportResolution::Rename(String::new()));
        assert_eq!(
            screen.summary(),
            "Imported 4 connections: 2 new, 1 renamed, 1 overwritten, 1 skipped."
        );
    }
}
