use crate::action::Action;
use crate::model::Model;

mod registry;
pub use registry::{command_spec, palette_entries};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowIntent {
    SavepointCreate,
    SavepointRollback,
    SavepointRelease,
    DataSort,
    DataFilter,
    DataReview,
    SchemaPreview,
    SchemaRaw,
    SchemaDiff,
    Security,
    TransferExport,
    TransferImport,
    Backup,
    Restore,
    ConnectionConnect,
    ConnectionDuplicate,
    ConnectionTest,
    ConnectionDelete,
    ConnectionCloseSession,
    ProjectCreate,
    ProjectSwitch,
    ProjectRename,
    ProjectDelete,
    SettingsReset,
    RecoveryRestore,
    RecoveryDiscard,
    McpRevokeAll,
    InsertSnippet,
    SubmitParameters,
    ClearHistory,
    DiagnosticsExport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub requirements: &'static [Requirement],
    pub invocation: PaletteInvocation,
}

// ponytail: Action is ~344B; Box<Action> if PaletteInvocation is cloned on a hot path.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum PaletteInvocation {
    Dispatch(Action),
    OpenFlow(FlowIntent),
}

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub requirements: &'static [Requirement],
    pub disabled_reason: Option<String>,
    pub invocation: PaletteInvocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement {
    ActiveSession,
    Results,
    RowSelection,
    ExplorerNode,
    LoadedDdl,
    PendingChanges,
    Breadcrumb,
    ActiveQuery,
    Completion,
    Parameters,
    History,
    Recovery,
}

impl Requirement {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ActiveSession => "connect a session first",
            Self::Results => "no results available",
            Self::RowSelection => "select a result row or cell first",
            Self::ExplorerNode => "select an explorer object first",
            Self::LoadedDdl => "load DDL first",
            Self::PendingChanges => "no pending changes",
            Self::Breadcrumb => "no previous data location",
            Self::ActiveQuery => "no query is running",
            Self::Completion => "no completion available",
            Self::Parameters => "no query parameters",
            Self::History => "history is empty",
            Self::Recovery => "no recovery checkpoint",
        }
    }
}

pub fn invocation_by_id(model: &Model, id: &str) -> Option<PaletteInvocation> {
    palette_entries(model)
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.invocation)
}

pub fn results_menu_items() -> &'static [(&'static str, &'static str)] {
    &[
        ("copy-row-csv", "Copy row as CSV"),
        ("copy-cell", "Copy cell"),
        ("data.copy.json", "Copy as JSON"),
        ("data.copy.csv", "Copy as CSV"),
        ("data.copy.markdown", "Copy as Markdown"),
        ("data.copy.sql", "Copy as SQL"),
        ("data.inspect", "Inspect value"),
        ("data.filter", "Apply remote filter"),
        ("data.related", "Open related"),
    ]
}

/// Popup list rows for a terminal height. Matches `render_palette` (height clamp 5..=12, minus border+query).
pub fn popup_list_rows(term_height: u16) -> usize {
    term_height.clamp(5, 12).saturating_sub(3) as usize
}

/// Keep `selected` inside `[offset, offset + rows)`. Same rule as ratatui `ListState`.
pub fn scroll_to_selection(selected: usize, offset: usize, count: usize, rows: usize) -> usize {
    if count == 0 || rows == 0 {
        return 0;
    }
    let selected = selected.min(count - 1);
    let max_offset = count.saturating_sub(rows);
    if selected < offset {
        selected
    } else if selected >= offset.saturating_add(rows) {
        selected
            .saturating_add(1)
            .saturating_sub(rows)
            .min(max_offset)
    } else {
        offset.min(max_offset)
    }
}

pub fn filter_entries<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    if query.is_empty() {
        return entries.iter().collect();
    }
    let mut scored: Vec<(u8, &PaletteEntry)> = entries
        .iter()
        .filter_map(|entry| score(entry, query).map(|s| (s, entry)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(b.1.title)));
    scored.into_iter().map(|(_, entry)| entry).collect()
}

fn score(entry: &PaletteEntry, query: &str) -> Option<u8> {
    let query = query.to_ascii_lowercase();
    let haystacks = std::iter::once(entry.title)
        .chain(entry.keywords.iter().copied())
        .chain(std::iter::once(entry.id));
    haystacks.filter_map(|text| score_text(text, &query)).max()
}

fn score_text(text: &str, query: &str) -> Option<u8> {
    let text = text.to_ascii_lowercase();
    if text.starts_with(query) {
        return Some(3);
    }
    if text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        return Some(2);
    }
    if is_subsequence(&text, query) {
        return Some(1);
    }
    None
}

fn is_subsequence(text: &str, query: &str) -> bool {
    let mut chars = text.chars();
    query.chars().all(|needle| chars.any(|ch| ch == needle))
}

#[cfg(test)]
mod tests {
    use super::{filter_entries, palette_entries, popup_list_rows, scroll_to_selection};
    use crate::model::Model;
    use dexo_driver_api::TransactionState;

    #[test]
    fn palette_explains_disabled_commit() {
        let mut model = Model::fixture(TransactionState::Idle);
        model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
        let entries = palette_entries(&model);
        let commit = entries
            .iter()
            .find(|e| e.id == "transaction.commit")
            .unwrap();
        assert_eq!(
            commit.disabled_reason.as_deref(),
            Some("no active transaction")
        );
    }

    #[test]
    fn fuzzy_prefers_prefix_over_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "quit");
        assert_eq!(filtered[0].id, "workbench.quit");
    }

    #[test]
    fn fuzzy_word_start_beats_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "pal");
        assert_eq!(filtered[0].id, "palette.open");
    }

    #[test]
    fn scroll_keeps_selection_in_window() {
        assert_eq!(scroll_to_selection(0, 0, 20, 9), 0);
        assert_eq!(scroll_to_selection(8, 0, 20, 9), 0);
        assert_eq!(scroll_to_selection(9, 0, 20, 9), 1);
        assert_eq!(scroll_to_selection(8, 1, 20, 9), 1);
        assert_eq!(scroll_to_selection(0, 1, 20, 9), 0);
        assert_eq!(scroll_to_selection(19, 1, 20, 9), 11);

        let mut model = Model::default();
        model.palette.open = true;
        let entries = palette_entries(&model);
        model.palette.selected = entries.len() - 1;
        model.palette.offset = scroll_to_selection(
            model.palette.selected,
            0,
            entries.len(),
            popup_list_rows(model.height),
        );
        let view = crate::render::render_to_string(&model, 80, 24);
        let last = entries.last().unwrap().title;
        assert!(
            view.contains(last),
            "selected command `{last}` should stay visible after scroll"
        );
        assert!(
            !view.contains(entries[0].title),
            "first command should scroll off when selection is at the end"
        );
    }

    #[test]
    fn every_current_action_is_in_palette() {
        let entries = palette_entries(&Model::default());
        let ids: std::collections::BTreeSet<_> = entries.iter().map(|entry| entry.id).collect();
        assert_eq!(entries.len(), 129);
        assert_eq!(ids.len(), 129);
    }

    #[test]
    fn help_layout_and_results_menu_actions() {
        use crate::action::{Action, FocusTarget};
        use crate::layout::LayoutPreset;
        use crate::model::GridSelection;
        use crate::update::update;

        let mut model = Model::default();
        update(&mut model, Action::ToggleHelp);
        assert!(model.help.open);
        let view = crate::render::render_to_string(&model, 100, 40);
        assert!(view.contains("Keybindings"));
        assert!(view.contains("Editor"));
        update(&mut model, Action::ToggleHelp);
        assert!(!model.help.open);

        update(&mut model, Action::CycleLayout);
        assert_eq!(model.layout_preset, LayoutPreset::ResultsWide);
        assert!(!model.panes.inspector_visible);
        update(&mut model, Action::ResetLayout);
        assert_eq!(model.layout_preset, LayoutPreset::Normal);
        assert!(model.panes.inspector_visible);

        update(&mut model, Action::Focus(FocusTarget::Results));
        model.results = crate::model::ResultsState::default();
        *model.results = crate::model::GridModel::sample_rows(6);
        update(&mut model, Action::ResultsDown);
        assert_eq!(model.results.cursor_row(), Some(1));
        update(&mut model, Action::ResultsExtendDown);
        assert!(matches!(
            model.results.kind,
            GridSelection::Range {
                start: (1, _),
                end: (2, _)
            }
        ));
        update(&mut model, Action::ToggleResultsPick);
        assert!(!model.results_menu.open);
        assert!(model.results.picked_rows.contains(&2));
        update(&mut model, Action::OpenResultsMenu);
        assert!(model.results_menu.open);
        let view = crate::render::render_to_string(&model, 80, 24);
        assert!(view.contains("Row actions"));
        update(
            &mut model,
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        assert!(!model.results_menu.open);

        update(&mut model, Action::Focus(FocusTarget::Editor));
        let view = crate::render::render_to_string(&model, 100, 40);
        assert!(view.contains("FOCUS: Editor"));
        assert!(view.contains("▸ SQL") || view.contains("> SQL"));
    }
}
