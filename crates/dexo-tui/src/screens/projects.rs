use dexo_app::Project;
use dexo_storage::ProjectDeletePreview;

use crate::runtime::project_manager::ProjectSwitch;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectsMode {
    #[default]
    Browse,
    Create,
    Rename,
    DeleteConfirm,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectDeletePrompt {
    pub project: Project,
    pub preview: ProjectDeletePreview,
    pub delete_connections: bool,
    pub typed: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectsScreen {
    pub open: bool,
    pub list: Vec<Project>,
    pub selected: usize,
    pub name_input: String,
    pub mode: ProjectsMode,
    pub pending: Option<ProjectSwitch>,
    pub delete: Option<ProjectDeletePrompt>,
    pub recents: Vec<String>,
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
    }

    pub fn touch_recent(&mut self, name: &str) {
        self.recents.retain(|item| item != name);
        self.recents.insert(0, name.to_string());
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec!["Projects".into()];
        for (index, project) in self.list.iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            let active = if self
                .pending
                .as_ref()
                .map(|switch| switch.target.id == project.id)
                .unwrap_or(false)
            {
                " switching"
            } else {
                ""
            };
            lines.push(format!("{marker} {}{active}", project.name));
        }
        if !self.recents.is_empty() {
            lines.push(format!("recent: {}", self.recents.join(", ")));
        }
        match self.mode {
            ProjectsMode::Create => lines.push(format!("create: {}", self.name_input)),
            ProjectsMode::Rename => lines.push(format!("rename: {}", self.name_input)),
            ProjectsMode::Browse => {}
            ProjectsMode::DeleteConfirm => {}
        }
        if let Some(delete) = &self.delete {
            lines.push(format!(
                "delete {}? connections={} documents={} snippets={}",
                delete.project.name,
                delete.preview.connections,
                delete.preview.documents,
                delete.preview.snippets
            ));
            if !delete.preview.external_paths.is_empty() {
                lines.push(format!(
                    "external files kept: {}",
                    delete.preview.external_paths.join(", ")
                ));
            }
            lines.push(format!(
                "type name to confirm ({}) connections:{}",
                delete.typed,
                if delete.delete_connections {
                    "delete"
                } else {
                    "detach"
                }
            ));
        }
        if let Some(switch) = &self.pending {
            lines.push(format!(
                "switch to {} ({:?})",
                switch.target.name, switch.stage
            ));
        }
        lines
    }
}
