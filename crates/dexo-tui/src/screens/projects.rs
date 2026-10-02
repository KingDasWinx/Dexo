use dexo_app::Project;
use dexo_storage::ProjectDeletePreview;

use crate::model::CloseChoice;
use crate::runtime::project_manager::{ProjectSwitch, ProjectSwitchStage};
use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectsMode {
    #[default]
    Browse,
    Create,
    Rename,
    DeleteConfirm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectIntent {
    Switch,
    Rename,
    Delete,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectDeletePrompt {
    pub project: Project,
    pub preview: ProjectDeletePreview,
    pub delete_connections: bool,
    pub typed: TextInput,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectsScreen {
    pub open: bool,
    pub list: Vec<Project>,
    pub selected: usize,
    pub name_input: TextInput,
    pub mode: ProjectsMode,
    pub pending: Option<ProjectSwitch>,
    pub delete: Option<ProjectDeletePrompt>,
    pub recents: Vec<String>,
    pub intent: Option<ProjectIntent>,
    pub error: Option<String>,
    pub footer: FooterFocus,
    /// Which answer the unsaved-changes question has focused.
    pub dirty_choice: Option<CloseChoice>,
    /// The name being typed was asked for from the list, so Esc goes back to it; asked
    /// for by the palette, Esc closes the dialog.
    pub from_list: bool,
    /// How many sessions the switch under way closes, to say so when it is done.
    pub closing_sessions: usize,
}

/// `n` of a thing, in the plural unless there is exactly one.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl ProjectsScreen {
    pub fn selected(&self) -> Option<&Project> {
        self.list.get(self.selected)
    }

    pub fn by_name(&self, name: &str) -> Option<Project> {
        self.list
            .iter()
            .find(|project| project.name == name)
            .cloned()
    }

    pub fn load(&mut self, list: Vec<Project>) {
        self.list = list;
        if self.selected >= self.list.len() {
            self.selected = 0;
        }
        // A project that is gone or renamed is no longer a recent one.
        let names: Vec<String> = self
            .list
            .iter()
            .map(|project| project.name.clone())
            .collect();
        self.recents.retain(|name| names.contains(name));
    }

    pub fn touch_recent(&mut self, name: &str) {
        self.recents.retain(|item| item != name);
        self.recents.insert(0, name.to_string());
    }

    /// Whether the question about unsaved documents is the one on screen.
    pub fn asking_about_unsaved(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|switch| switch.stage == ProjectSwitchStage::ConfirmDirty)
    }

    /// The dialog's title: it is five dialogs, and says which one it is.
    pub fn title(&self) -> &'static str {
        if self.asking_about_unsaved() {
            return "Unsaved changes";
        }
        match (self.delete.is_some(), self.mode, self.intent) {
            (true, ..) => "Delete project",
            (_, ProjectsMode::Create, _) => "New project",
            (_, ProjectsMode::Rename, _) => "Rename project",
            (_, _, Some(ProjectIntent::Switch)) => "Switch project",
            (_, _, Some(ProjectIntent::Rename)) => "Rename project",
            (_, _, Some(ProjectIntent::Delete)) => "Delete project",
            _ => "Projects",
        }
    }

    /// The dialog's lines. `active` is the project that is open, `unsaved` the titles of
    /// the open documents that have changes no file holds.
    pub fn lines(&self, active: &str, unsaved: &[String], room: usize) -> Vec<String> {
        if self.asking_about_unsaved() {
            return self.unsaved_lines(active, unsaved);
        }
        if let Some(delete) = &self.delete {
            return self.delete_lines(delete);
        }
        match self.mode {
            ProjectsMode::Create | ProjectsMode::Rename => self.name_lines(),
            ProjectsMode::Browse | ProjectsMode::DeleteConfirm => self.list_lines(active, room),
        }
    }

    /// Where the list starts, so the selected project stays on screen.
    pub fn list_offset(&self, room: usize) -> usize {
        crate::palette::scroll_to_selection(self.selected, 0, self.list.len(), room.max(1))
    }

    fn list_lines(&self, active: &str, room: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let offset = self.list_offset(room);
        for (index, project) in self.list.iter().enumerate().skip(offset).take(room.max(1)) {
            let marker = if index == self.selected { ">" } else { " " };
            let open = if project.name == active {
                "  (open)"
            } else {
                ""
            };
            lines.push(format!("{marker} {}{open}", project.name));
        }
        lines.push(String::new());
        if let Some(switch) = &self.pending {
            lines.push(format!("Switching to {}...", switch.target.name));
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(
            match self.intent {
                Some(ProjectIntent::Switch) => "Enter switch  n new  r rename  x delete  Esc close",
                Some(ProjectIntent::Rename) => "Enter rename  n new  x delete  Esc close",
                Some(ProjectIntent::Delete) => "Enter delete  n new  r rename  Esc close",
                None => "Enter switch  n new  r rename  x delete  Esc close",
            }
            .into(),
        );
        lines
    }

    /// The name being typed, then the buttons: the input comes first in the Tab order and
    /// carries the focus marker.
    fn name_lines(&self) -> Vec<String> {
        let renaming = self.mode == ProjectsMode::Rename;
        let marker = if self.footer == FooterFocus::Input {
            ">"
        } else {
            " "
        };
        let mut lines = Vec::new();
        if renaming && let Some(project) = self.selected() {
            lines.push(format!("Renaming {}", project.name));
        }
        lines.push(format!("{marker} name: {}", self.name_input.as_str()));
        lines.push(
            self.error
                .clone()
                .unwrap_or_else(|| "Enter save  Tab next  Esc cancel".into()),
        );
        lines.push(footer_line("Submit", self.footer));
        lines
    }

    fn delete_lines(&self, delete: &ProjectDeletePrompt) -> Vec<String> {
        let preview = &delete.preview;
        let marker = if self.footer == FooterFocus::Input {
            ">"
        } else {
            " "
        };
        let mut lines = vec![
            format!("Delete the project {}?", delete.project.name),
            format!(
                "It has {}, {} and {}.",
                count(preview.documents, "document", "documents"),
                count(preview.snippets, "snippet", "snippets"),
                count(preview.saved_queries, "saved query", "saved queries"),
            ),
        ];
        if !preview.external_paths.is_empty() {
            lines.push(format!(
                "Files outside Dexo stay where they are: {}",
                preview.external_paths.join(", ")
            ));
        }
        lines.push(format!(
            "[{}] also delete its {}  Alt+C",
            if delete.delete_connections { "x" } else { " " },
            count(preview.connections, "connection", "connections"),
        ));
        if !delete.delete_connections && preview.connections > 0 {
            lines.push("    Left unchecked, they stay, outside any project.".into());
        }
        lines.push("Type the project's name to confirm.".into());
        lines.push(format!("{marker} name: {}", delete.typed.as_str()));
        lines.push(footer_line("Delete", self.footer));
        lines
    }

    fn unsaved_lines(&self, active: &str, unsaved: &[String]) -> Vec<String> {
        let target = self
            .pending
            .as_ref()
            .map(|switch| switch.target.name.as_str())
            .unwrap_or_default();
        let choice = self.dirty_choice.unwrap_or(CloseChoice::Save);
        let button = |this: CloseChoice, label: &str| {
            format!("{}{label}", if choice == this { ">" } else { " " })
        };
        let mut lines = vec![
            format!(
                "{} in {active} {} changes that are not saved.",
                count(unsaved.len(), "document", "documents"),
                if unsaved.len() == 1 { "has" } else { "have" }
            ),
            format!("  {}", unsaved.join(", ")),
            String::new(),
            format!("Switching to {target}:"),
            "  Save writes the ones that have a file; the rest stay here as drafts.".into(),
            "  Don't save closes them and drops their changes.".into(),
            String::new(),
        ];
        lines.push(
            [
                button(CloseChoice::Save, "[Save]"),
                button(CloseChoice::Discard, "[Don't save]"),
                button(CloseChoice::Cancel, "[Cancel]"),
            ]
            .join("  "),
        );
        lines
    }
}

#[cfg(test)]
mod tests {
    use dexo_app::{Project, ProjectId};

    use super::*;

    fn project(name: &str) -> Project {
        Project {
            id: ProjectId(uuid::Uuid::new_v4()),
            name: name.into(),
            created_at: String::new(),
        }
    }

    #[test]
    fn the_open_project_is_marked_and_the_keys_are_named() {
        let mut screen = ProjectsScreen::default();
        screen.load(vec![project("Default"), project("qa")]);
        screen.selected = 1;
        let lines = screen.lines("Default", &[], 10);
        assert_eq!(lines[0], "  Default  (open)");
        assert_eq!(lines[1], "> qa");
        assert!(lines.last().unwrap().contains("Enter switch"));
    }

    #[test]
    fn a_name_comes_before_its_buttons_and_carries_the_focus_marker() {
        let mut screen = ProjectsScreen::default();
        screen.load(vec![project("Default")]);
        screen.mode = ProjectsMode::Create;
        screen.name_input.set_text("new");
        let lines = screen.lines("Default", &[], 10);
        assert_eq!(lines[0], "> name: new");
        assert!(lines.last().unwrap().contains("[Submit]"));
        screen.footer = FooterFocus::Submit;
        assert_eq!(screen.lines("Default", &[], 10)[0], "  name: new");
        assert_eq!(screen.title(), "New project");
    }

    #[test]
    fn deleting_asks_in_plain_words() {
        let mut screen = ProjectsScreen::default();
        screen.load(vec![project("a"), project("b")]);
        screen.delete = Some(ProjectDeletePrompt {
            project: project("b"),
            preview: ProjectDeletePreview {
                connections: 1,
                documents: 1,
                snippets: 0,
                saved_queries: 1,
                external_paths: Vec::new(),
            },
            delete_connections: false,
            typed: Default::default(),
        });
        let text = screen.lines("a", &[], 10).join("\n");
        assert!(text.contains("Delete the project b?"), "{text}");
        assert!(
            text.contains("1 document, 0 snippets and 1 saved query"),
            "{text}"
        );
        assert!(text.contains("[ ] also delete its 1 connection"), "{text}");
        assert!(!text.contains('='), "no key=value dump: {text}");
        assert!(text.contains("[Delete]  "), "{text}");
    }
}
