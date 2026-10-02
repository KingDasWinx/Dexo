use crate::action::Action;
use crate::model::Model;

mod registry;
pub(crate) use registry::shortcut_for;
pub use registry::{command_spec, command_specs, palette_entries};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowIntent {
    SavepointCreate,
    SavepointRollback,
    SavepointRelease,
    DataReview,
    SchemaPreview,
    SchemaRaw,
    SchemaDiff,
    Security,
    TransferExport,
    TransferImport,
    Backup,
    Restore,
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
    ConnectionDelete,
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
    /// The key the active keymap gives the command, as the palette shows it.
    pub shortcut: Option<String>,
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
    SelectedConnection,
    LoadedDdl,
    PendingChanges,
    ActiveQuery,
    Parameters,
    History,
}

impl Requirement {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ActiveSession => "connect a session first",
            Self::Results => "no results available",
            Self::RowSelection => "select a result row or cell first",
            Self::ExplorerNode => "select an explorer object first",
            Self::SelectedConnection => "select a connection first",
            Self::LoadedDdl => "load DDL first",
            Self::PendingChanges => "no pending changes",
            Self::ActiveQuery => "no query is running",
            Self::Parameters => "no query parameters",
            Self::History => "history is empty",
        }
    }
}

/// Resolves any registered command, hidden ones included -- keys and the results menu
/// address commands by id and must keep reaching what the palette no longer lists.
pub fn invocation_by_id(_model: &Model, id: &str) -> Option<PaletteInvocation> {
    command_spec(id).map(|spec| spec.invocation)
}

/// What a sidebar node offers. The tree has more kinds than this; what matters to a
/// menu is which set of commands applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeMenuKind {
    Connection,
    Relation,
    Object,
}

/// Commands the context menu lists for a node, in display order. Ids only: the title,
/// the shortcut and the reason a row is disabled all come from the same registry the
/// palette reads, so the menu cannot drift from it the way `ExplorerAction` did.
pub fn node_menu_items(kind: NodeMenuKind) -> &'static [&'static str] {
    match kind {
        NodeMenuKind::Connection => &[
            "explorer.expand",
            "document.new",
            "explorer.copy_name",
            "connection.test",
            "explorer.refresh",
            "editor.history",
            "schema.security",
            "admin.sessions",
            "backup.dump",
            "backup.restore",
            "explorer.favorites_only",
            "explorer.system_objects",
            "connection.edit",
            "connection.duplicate",
            "connection.move_group",
            "connection.close_session",
            "connection.delete",
        ],
        NodeMenuKind::Relation => &[
            "explorer.data",
            "explorer.inspect",
            "explorer.ddl",
            "explorer.copy_ddl",
            "explorer.dependencies",
            "explorer.copy_name",
            "explorer.copy_simple",
            "explorer.favorite",
            "explorer.refresh",
        ],
        NodeMenuKind::Object => &[
            "explorer.inspect",
            "explorer.copy_name",
            "explorer.copy_simple",
            "explorer.favorite",
            "explorer.refresh",
        ],
    }
}

/// The menu's rows for `kind`, carrying the same disabled reasons the palette shows.
pub fn node_menu_entries(model: &Model, kind: NodeMenuKind) -> Vec<PaletteEntry> {
    let entries = registry::all_entries(model);
    node_menu_items(kind)
        .iter()
        .filter_map(|id| entries.iter().find(|entry| entry.id == *id).cloned())
        .collect()
}

pub fn results_menu_items() -> &'static [(&'static str, &'static str)] {
    &[
        ("data.edit_cell", "Edit cell"),
        ("data.copy.cell", "Copy cell"),
        ("data.copy.text", "Copy as Text"),
        ("data.copy.json", "Copy as JSON"),
        ("data.copy.csv", "Copy as CSV"),
        ("data.copy.markdown", "Copy as Markdown"),
        ("data.copy.sql", "Copy as SQL"),
        ("data.inspect", "Inspect value"),
        ("data.filter", "Filter rows (WHERE)"),
        ("data.sort", "Sort rows (ORDER BY)"),
        ("results.sort_column", "Sort by this column"),
        ("results.sort_add_column", "Add column to sort"),
        ("results.count", "Count rows"),
        ("data.related", "Related rows…"),
        ("data.nav_back", "Back from related rows"),
        ("data.refresh", "Refresh table data"),
    ]
}

/// Rows the popup spends on itself: two borders, the query line, and the footer that
/// carries why the selected command cannot run.
const POPUP_CHROME: u16 = 4;

/// Height the popup takes for `count` matches. It shrinks to its content -- sizing it
/// to the terminal left a column of blank rows under every short result list.
pub fn popup_height(term_height: u16, count: usize) -> u16 {
    let wanted = u16::try_from(count)
        .unwrap_or(u16::MAX)
        .saturating_add(POPUP_CHROME);
    wanted
        .clamp(POPUP_CHROME + 1, POPUP_MAX_HEIGHT)
        .min(term_height.max(POPUP_CHROME + 1))
}

/// Command rows the popup can draw for `count` matches.
pub fn popup_list_rows(term_height: u16, count: usize) -> usize {
    popup_height(term_height, count)
        .saturating_sub(POPUP_CHROME)
        .max(1) as usize
}

/// The context menu has no query line, so it spends one row less than the palette on
/// itself, and it is allowed to be taller: its list is fixed, and a menu that hides
/// Delete behind a scroll the user cannot see is worse than a tall menu.
const MENU_CHROME: u16 = 3;
const MENU_MAX_HEIGHT: u16 = 24;

pub fn menu_height(term_height: u16, count: usize) -> u16 {
    let wanted = u16::try_from(count)
        .unwrap_or(u16::MAX)
        .saturating_add(MENU_CHROME);
    wanted
        .clamp(MENU_CHROME + 1, MENU_MAX_HEIGHT)
        .min(term_height.max(MENU_CHROME + 1))
}

pub fn menu_list_rows(term_height: u16, count: usize) -> usize {
    menu_height(term_height, count)
        .saturating_sub(MENU_CHROME)
        .max(1) as usize
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

/// Palette categories, in display order. The key is the command id prefix; several
/// one-command prefixes fold into a shared label so the list does not turn into a
/// column of headings for a column of commands.
/// Kept flat so `render_palette` and `popup_list_rows` cannot drift apart.
pub const POPUP_MAX_HEIGHT: u16 = 16;
pub const POPUP_MAX_WIDTH: u16 = 76;

const CATEGORIES: &[(&str, &str)] = &[
    ("query", "Query"),
    ("document", "Document"),
    ("editor", "Editor"),
    ("workbench", "Workbench"),
    ("palette", "Workbench"),
    ("help", "Workbench"),
    ("config", "Workbench"),
    ("admin", "Workbench"),
    ("diagnostics", "Workbench"),
    ("connection", "Connection"),
    ("explorer", "Explorer"),
    ("transaction", "Transaction"),
    ("data", "Data"),
    ("results", "Results"),
    ("schema", "Schema"),
    ("explain", "Explain"),
    ("transfer", "Transfer"),
    ("backup", "Backup"),
    ("project", "Project"),
    ("mcp", "MCP"),
    ("recovery", "Recovery"),
    ("settings", "Settings"),
    ("layout", "Layout"),
    ("focus", "Focus"),
    ("tab", "Tab"),
];

fn category_index(id: &str) -> usize {
    let prefix = id.split('.').next().unwrap_or(id);
    CATEGORIES
        .iter()
        .position(|(key, _)| *key == prefix)
        .unwrap_or(CATEGORIES.len())
}

/// Display name for a command's category, e.g. `data.copy.csv` -> "Data".
pub fn category_label(id: &str) -> &str {
    CATEGORIES
        .get(category_index(id))
        .map(|(_, label)| *label)
        .unwrap_or_else(|| id.split('.').next().unwrap_or(id))
}

/// Commands that cannot run sort after the ones that can. Ranking them by score alone
/// put "Commit Transaction (connect a session first)" under the cursor for a query like
/// `com`, so the default Enter did nothing.
fn unusable(entry: &PaletteEntry) -> bool {
    entry.disabled_reason.is_some()
}

pub fn filter_entries<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    let query = query.trim();
    if query.is_empty() {
        let mut browse: Vec<&PaletteEntry> = entries.iter().collect();
        browse.sort_by_key(|entry| (unusable(entry), category_index(entry.id)));
        return browse;
    }
    let mut scored: Vec<(u32, usize, &PaletteEntry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| score(entry, query).map(|s| (s, index, entry)))
        .collect();
    // Letters scattered across a title are a last resort: once something matches by its
    // words or a typo, they are noise. `stat` listed `Reset layout` beside the real hit.
    if scored.iter().any(|(score, ..)| *score >= 400) {
        scored.retain(|(score, ..)| *score >= 400);
    }
    // Equal matches keep the registry's order inside a category, the categories their
    // own: `save` lists Save Document before Save Query As, not by the alphabet.
    // A command that cannot run sorts after the ones that can -- but only among those
    // the title names: a disabled `Execute Statement` is still the answer to `exec`, and
    // a keyword match that happens to be usable is not.
    let by_title = |score: u32| score < 700;
    scored.sort_by(|a, b| {
        by_title(a.0)
            .cmp(&by_title(b.0))
            .then_with(|| unusable(a.2).cmp(&unusable(b.2)))
            .then_with(|| b.0.cmp(&a.0))
            .then_with(|| category_index(a.2.id).cmp(&category_index(b.2.id)))
            .then_with(|| a.1.cmp(&b.1))
    });
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

/// What `query` is worth against a command, best first; `None` is no match.
///
/// The title is what the user reads, so it outranks every other way in: a command whose
/// title starts with the query, then one that has the words in it, then its keywords,
/// then a typo. A scattered match inside the keywords or the id put `Copy Object Name`
/// above `Execute Statement` for `exec`.
fn score(entry: &PaletteEntry, query: &str) -> Option<u32> {
    let query = query.to_lowercase();
    let title = entry.title.to_lowercase();
    // The id is how the docs and the keymap name a command, so typed whole it is a hit.
    if entry.id == query || title == query {
        return Some(1000);
    }
    if title.starts_with(&query) {
        return Some(950);
    }
    if query.contains('.') && entry.id.starts_with(&query) {
        return Some(900);
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let title_words = words_of(&title);
    if words.iter().all(|word| {
        title_words
            .iter()
            .any(|candidate| candidate.starts_with(word))
    }) {
        return Some(850);
    }
    if title.contains(&query) {
        return Some(750);
    }
    // A slip of the keys in the title: one letter missing, extra, wrong or swapped in a
    // word of four or more letters, the others matching as above.
    if words.iter().all(|word| {
        title_words
            .iter()
            .any(|candidate| candidate.starts_with(word) || is_typo(candidate, word))
    }) {
        return Some(720);
    }
    let keyword_words: Vec<String> = entry
        .keywords
        .iter()
        .flat_map(|keyword| words_of(&keyword.to_lowercase()))
        .collect();
    if words.iter().all(|word| {
        title_words
            .iter()
            .chain(&keyword_words)
            .any(|candidate| candidate.starts_with(word))
    }) {
        return Some(650);
    }
    // The same slip, in the title or the keywords.
    if words.iter().all(|word| {
        title_words
            .iter()
            .chain(&keyword_words)
            .any(|candidate| candidate.starts_with(word) || is_typo(candidate, word))
    }) {
        return Some(500);
    }
    if entry.id.starts_with(&query) {
        return Some(400);
    }
    // Letters in order across the title: `qit` for Quit. Nothing but the title, or the
    // keywords and the id would match anything.
    if words.len() == 1 && query.chars().count() >= 2 && is_subsequence(&title, &query) {
        return Some(100);
    }
    None
}

fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// `typed` is `word` with one letter dropped, added, changed or swapped with the next,
/// or the start of `word` with one such slip. Short words are exact: `sav` is not a
/// typo of anything.
fn is_typo(word: &str, typed: &str) -> bool {
    let typed: Vec<char> = typed.chars().collect();
    if typed.len() < 4 {
        return false;
    }
    let word: Vec<char> = word.chars().collect();
    // The typed text is a whole word or the start of one, so compare it with the word
    // cut to the same length, give or take the letter that slipped.
    (typed.len().saturating_sub(1)..=typed.len() + 1)
        .any(|length| word.len() >= length && one_slip(&word[..length], &typed))
}

/// Whether `a` and `b` differ by one edit: a letter added, removed, changed, or two
/// neighbours swapped.
fn one_slip(a: &[char], b: &[char]) -> bool {
    if a == b {
        return false;
    }
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => {
            let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
            diff.len() == 1
                || (diff.len() == 2
                    && diff[1] == diff[0] + 1
                    && a[diff[0]] == b[diff[1]]
                    && a[diff[1]] == b[diff[0]])
        }
        std::cmp::Ordering::Less => skips_one(b, a),
        std::cmp::Ordering::Greater => skips_one(a, b),
    }
}

/// `long` is `short` with one more letter somewhere.
fn skips_one(long: &[char], short: &[char]) -> bool {
    long.len() == short.len() + 1
        && (0..long.len()).any(|skip| {
            long.iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, c)| c)
                .eq(short.iter())
        })
}

/// Whether any of `texts` holds every word of `query`, each at the start of a word
/// (case-insensitive). The F1 help filters with it: a scattered match listed `Object
/// Actions` for `exec`. An empty `query` always matches.
pub(crate) fn matches_any(texts: &[&str], query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    texts.iter().any(|text| {
        let text = text.to_lowercase();
        let words = words_of(&text);
        text.contains(&query)
            || query
                .split_whitespace()
                .all(|word| words.iter().any(|candidate| candidate.starts_with(word)))
    })
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

    /// The key shown is the one that works everywhere -- Help is F1, not the explorer's
    /// `?` -- and a bare letter reads as typed, apart from Shift and the letter.
    #[test]
    fn the_palette_shows_the_key_that_works_and_letters_as_typed() {
        let model = Model::default();
        let entries = crate::palette::registry::all_entries(&model);
        let key = |id: &str| {
            entries
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.shortcut.clone())
        };
        assert_eq!(key("help.open").as_deref(), Some("F1"));
        assert_eq!(key("results.sort_column").as_deref(), Some("s"));
        assert_eq!(key("results.sort_add_column").as_deref(), Some("Shift+S"));
    }

    /// A key keymap.toml gives a command no built-in keymap binds shows in the palette.
    #[test]
    fn an_overlay_only_binding_shows_its_key() {
        let mut model = Model::default();
        model.keymap =
            crate::keymap::merge_overlay(&model.keymap, "[global]\n\"f9\" = \"connection.test\"\n")
                .unwrap();
        let entries = palette_entries(&model);
        let test = entries.iter().find(|e| e.id == "connection.test").unwrap();
        assert_eq!(test.shortcut.as_deref(), Some("F9"));
    }

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
        // "Submit Parameters" starts a word with the query; "Compare Schema" only
        // holds its letters scattered. Once something matches by its words, the
        // scattered ones are not listed at all.
        let filtered = filter_entries(&entries, "para");
        let rank = |id: &str| filtered.iter().position(|entry| entry.id == id);
        assert_eq!(rank("editor.parameters"), Some(0), "{filtered:?}");
        assert_eq!(rank("schema.diff"), None, "{filtered:?}");
    }

    /// What users type and what they should get first, the ten the QA run tried.
    #[test]
    fn the_obvious_command_is_first() {
        let entries = palette_entries(&Model::default());
        let first = |query: &str| {
            filter_entries(&entries, query)
                .first()
                .map(|entry| entry.id)
                .unwrap_or("none")
        };
        for (query, id) in [
            ("exec", "query.execute_statement"),
            ("exec st", "query.execute_statement"),
            ("stat", "query.execute_statement"),
            ("save", "document.save"),
            ("svae", "document.save"),
            ("clsoe doc", "document.close"),
            ("reanme", "document.rename"),
            ("qit", "workbench.quit"),
            ("excute", "query.execute_statement"),
            ("statment", "query.execute_statement"),
            ("project.create", "project.create"),
        ] {
            assert_eq!(first(query), id, "`{query}`");
        }
    }

    /// Letters scattered over keywords and ids made `exec` find `Copy Object Name`.
    #[test]
    fn scattered_letters_in_the_id_find_nothing() {
        let entries = palette_entries(&Model::default());
        let ids: Vec<_> = filter_entries(&entries, "exec")
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert!(!ids.contains(&"explorer.copy_name"), "{ids:?}");
        assert!(ids.contains(&"explain.analyze"), "{ids:?}");
    }

    /// The empty palette used to open on `Data  Back from Related Rows` with no table
    /// open, and listed a group twice: what can run comes first, group by group, and the
    /// rest follows in the same order.
    #[test]
    fn the_empty_palette_leads_with_what_can_run_and_names_each_group_once() {
        let entries = palette_entries(&Model::default());
        let browse = filter_entries(&entries, "");
        let first = browse[0];
        assert!(first.disabled_reason.is_none(), "{first:?}");
        for (usable, name) in [(true, "can run"), (false, "cannot run")] {
            let mut labels: Vec<&str> = Vec::new();
            for entry in browse
                .iter()
                .filter(|entry| entry.disabled_reason.is_none() == usable)
            {
                let label = super::category_label(entry.id);
                if labels.last() != Some(&label) {
                    assert!(
                        !labels.contains(&label),
                        "{label} twice among those that {name}"
                    );
                    labels.push(label);
                }
            }
        }
        let rank = |id: &str| browse.iter().position(|entry| entry.id == id).unwrap();
        assert!(rank("workbench.quit") < rank("results.record_view"));
        assert!(rank("help.open") < rank("explain.open"));
    }

    #[test]
    fn the_settings_and_layout_commands_are_in_the_palette() {
        let entries = palette_entries(&Model::default());
        for id in [
            "settings.theme",
            "settings.mode",
            "settings.accent",
            "settings.keymap",
            "settings.mouse",
            "settings.animation",
            "settings.unicode",
            "settings.reset",
            "layout.hide_explorer",
            "layout.hide_results",
            "layout.results_grow",
            "layout.results_shrink",
            "layout.explorer_grow",
            "layout.explorer_shrink",
            "focus.explorer",
            "focus.editor",
            "focus.results",
            "focus.tabs",
            "document.next",
            "document.prev",
        ] {
            assert!(entries.iter().any(|entry| entry.id == id), "{id}");
        }
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
            popup_list_rows(model.height, entries.len()),
        );
        let view = crate::render::render_to_string(&model, 80, 24);
        let ordered = filter_entries(&entries, "");
        let last = ordered.last().unwrap().title;
        assert!(
            view.contains(last),
            "selected command `{last}` should stay visible after scroll"
        );
        assert!(
            !view.contains(ordered[0].title),
            "first command should scroll off when selection is at the end"
        );
    }

    #[test]
    fn palette_exposes_only_curated_commands() {
        let entries = palette_entries(&Model::default());
        let ids: std::collections::BTreeSet<_> = entries.iter().map(|entry| entry.id).collect();
        assert_eq!(entries.len(), 145);
        assert_eq!(ids.len(), 145);
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
        update(&mut model, Action::ResetLayout);
        assert_eq!(model.layout_preset, LayoutPreset::Normal);

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
        assert!(view.contains("Row 3"));
        assert!(view.contains("Record"));
        assert!(view.contains("Actions"));
        assert!(view.contains("n: 2"));
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
        assert!(view.contains("▸ SQL") || view.contains("> SQL"));
    }

    /// A name with accents is words like any other, in any case.
    #[test]
    fn accented_names_match_by_word_and_case() {
        assert!(super::matches_any(&["Relatório Mensal"], "mensal"));
        assert!(super::matches_any(&["Relatório Mensal"], "RELATÓ"));
        assert!(super::matches_any(&["vendas_por_região"], "região"));
    }
}
