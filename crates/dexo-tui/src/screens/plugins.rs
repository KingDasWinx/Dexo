//! The Plugins screen's state -- what is installed, what the index offers, the one
//! change under way -- and its keys.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::plugin::manifest::Kind;
use dexo_app::plugin::state::{Installed, Origin};

use crate::action::Effect;
use crate::model::Model;
use crate::mouse::HitTarget;
use crate::screen::widgets::Search;
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PluginsView {
    #[default]
    Installed,
    Available,
}

impl PluginsView {
    pub const ALL: [Self; 2] = [Self::Installed, Self::Available];

    pub fn other(self) -> Self {
        match self {
            Self::Installed => Self::Available,
            Self::Available => Self::Installed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TypeFilter {
    #[default]
    All,
    Drivers,
    Themes,
}

impl TypeFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Drivers,
            Self::Drivers => Self::Themes,
            Self::Themes => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Drivers => "drivers",
            Self::Themes => "themes",
        }
    }

    pub fn admits(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Drivers => kind == Kind::Driver,
            Self::Themes => kind == Kind::Theme,
        }
    }
}

/// One plugin as the screen lists it: installed, in the index, or both.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub description: String,
    pub authors: Vec<String>,
    pub license: String,
    pub installed: Option<Installed>,
    /// The newest version in the index this Dexo runs.
    pub newest: Option<String>,
    pub min_dexo: Option<String>,
    pub protocol: Option<u32>,
    /// Its download for this platform, in bytes.
    pub size: Option<u64>,
    /// Its drivers, or its themes' names.
    pub provides: Vec<String>,
    /// The first lines of its README.
    pub readme: Vec<String>,
}

impl PluginRow {
    /// The version an update would bring: an official plugin's, newer than installed.
    pub fn update(&self) -> Option<&str> {
        let installed = self.installed.as_ref()?;
        let newest = self.newest.as_deref()?;
        (installed.origin == Origin::Official
            && dexo_app::update::is_newer(newest, &installed.version))
        .then_some(newest)
    }

    /// Enabled, disabled, an update, available -- with a mark for each that reads
    /// without colour.
    pub fn mark(&self, unicode: bool) -> &'static str {
        let (enabled, disabled, update, available) = if unicode {
            ("●", "○", "↑", "+")
        } else {
            ("*", "o", "^", "+")
        };
        match &self.installed {
            None => available,
            Some(_) if self.update().is_some() => update,
            Some(installed) if installed.enabled => enabled,
            Some(_) => disabled,
        }
    }
}

/// What the screen shows, read off the disk and the index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginsSnapshot {
    pub rows: Vec<PluginRow>,
    /// Why there is no index, when there is none.
    pub index_error: Option<String>,
    /// The cache stood in for the index: what to say about it.
    pub offline: Option<String>,
}

/// An install or update under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Busy {
    pub id: String,
    pub verb: &'static str,
    pub done: u64,
    pub total: Option<u64>,
}

/// Installing from a file: its path typed, then -- the package read -- the trust
/// prompt, which typing the plugin's id answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FromFile {
    pub path: TextInput,
    pub staged: Option<StagedInfo>,
    pub confirm: TextInput,
    pub error: Option<String>,
}

/// A package read and waiting for trust; the runtime keeps it under `token`.
#[derive(Clone, Debug, PartialEq)]
pub struct StagedInfo {
    pub token: u64,
    pub id: String,
    pub trust: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginsState {
    pub view: PluginsView,
    pub snapshot: PluginsSnapshot,
    pub loading: bool,
    pub selected: usize,
    pub search: Search,
    pub filter: TypeFilter,
    pub busy: Option<Busy>,
    /// The plugin a removal is asked about.
    pub removing: Option<String>,
    pub from_file: Option<FromFile>,
}

impl PluginsState {
    /// The rows the view, the filter and the search let through.
    pub fn shown(&self) -> Vec<&PluginRow> {
        self.snapshot
            .rows
            .iter()
            .filter(|row| match self.view {
                PluginsView::Installed => row.installed.is_some(),
                PluginsView::Available => row.installed.is_none(),
            })
            .filter(|row| self.filter.admits(row.kind))
            .filter(|row| {
                self.search
                    .matches([row.id.as_str(), row.name.as_str(), row.description.as_str()])
            })
            .collect()
    }

    pub fn picked(&self) -> Option<&PluginRow> {
        let shown = self.shown();
        shown
            .get(self.selected.min(shown.len().saturating_sub(1)))
            .copied()
    }

    /// The installed plugins with an update: the count beside "Plugins" on the top line.
    pub fn updates(&self) -> usize {
        self.snapshot
            .rows
            .iter()
            .filter(|row| row.update().is_some())
            .count()
    }
}

/// The screen's keys. None leaves a key to the keymap, and Esc to going back.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> Option<Vec<Effect>> {
    let state = &mut model.plugins;
    if state.search.typing && state.search.key(key) {
        state.selected = 0;
        return Some(Vec::new());
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    let count = state.shown().len();
    match key.code {
        KeyCode::Tab | KeyCode::BackTab => {
            state.view = state.view.other();
            state.selected = 0;
        }
        KeyCode::Char('/') => state.search.typing = true,
        KeyCode::Char('t') => {
            state.filter = state.filter.next();
            state.selected = 0;
        }
        KeyCode::Char('r') => {
            state.loading = true;
            return Some(vec![Effect::LoadPlugins { refresh: true }]);
        }
        KeyCode::Up => state.selected = state.selected.saturating_sub(1),
        KeyCode::Down => state.selected = (state.selected + 1).min(count.saturating_sub(1)),
        KeyCode::Home => state.selected = 0,
        KeyCode::End => state.selected = count.saturating_sub(1),
        KeyCode::Esc if !state.search.input.is_empty() || state.filter != TypeFilter::All => {
            state.search = Search::default();
            state.filter = TypeFilter::All;
            state.selected = 0;
        }
        _ => return None,
    }
    Some(Vec::new())
}

/// A click: a view, or a row.
pub fn mouse(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ScreenView(index)) => {
            if let Some(view) = PluginsView::ALL.get(index) {
                model.plugins.view = *view;
                model.plugins.selected = 0;
            }
        }
        Some(HitTarget::ListRow(index)) => model.plugins.selected = index,
        _ => {}
    }
    Vec::new()
}
