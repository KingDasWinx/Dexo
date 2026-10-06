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
    /// A saved connection to open once the install offered for it is done.
    pub then_connect: Option<dexo_app::ConnectionProfile>,
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
    // A question or a path being typed holds the keys.
    if model.plugins.removing.is_some() {
        return Some(removal_key(model, key));
    }
    if model.plugins.from_file.is_some() {
        return Some(from_file_key(model, key));
    }
    if model.plugins.busy.is_some() && key.code == KeyCode::Esc {
        return Some(vec![Effect::CancelPluginWork]);
    }
    if model.plugins.search.typing && model.plugins.search.key(key) {
        model.plugins.selected = 0;
        return Some(Vec::new());
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    match key.code {
        KeyCode::Char('i') => return Some(install(model, false)),
        KeyCode::Char('u') => return Some(install(model, true)),
        KeyCode::Char('e') => return Some(toggle(model)),
        KeyCode::Char('a') => {
            let id = model.plugins.picked()?.id.clone();
            return Some(crate::update::update(
                model,
                crate::action::Action::ApplyPluginTheme { id },
            ));
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            let id = model
                .plugins
                .picked()
                .filter(|row| row.installed.is_some())
                .map(|row| row.id.clone());
            model.plugins.removing = id;
            return Some(Vec::new());
        }
        KeyCode::Char('f') => {
            model.plugins.from_file = Some(FromFile::default());
            model.sections[crate::model::Screen::Plugins.index()] = crate::screen::Section::Detail;
            return Some(Vec::new());
        }
        _ => {}
    }
    let state = &mut model.plugins;
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

fn install(model: &mut Model, upgrade: bool) -> Vec<Effect> {
    let Some(row) = model.plugins.picked() else {
        return Vec::new();
    };
    if (upgrade && row.update().is_none()) || (!upgrade && row.installed.is_some()) {
        return Vec::new();
    }
    let (id, total) = (row.id.clone(), row.size);
    model.plugins.busy = Some(Busy {
        id: id.clone(),
        verb: if upgrade { "Updating" } else { "Installing" },
        done: 0,
        total,
    });
    vec![Effect::InstallPlugin { id, upgrade }]
}

fn toggle(model: &mut Model) -> Vec<Effect> {
    let Some(row) = model.plugins.picked() else {
        return Vec::new();
    };
    let Some(installed) = &row.installed else {
        return Vec::new();
    };
    vec![Effect::SetPluginEnabled {
        id: row.id.clone(),
        enabled: !installed.enabled,
    }]
}

fn removal_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter => match model.plugins.removing.take() {
            Some(id) => vec![Effect::RemovePlugin { id }],
            None => Vec::new(),
        },
        KeyCode::Esc => {
            model.plugins.removing = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// The path, then the plugin's id typed to trust it. Esc drops a package already read.
fn from_file_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let Some(from_file) = model.plugins.from_file.as_mut() else {
        return Vec::new();
    };
    let staged = from_file.staged.clone();
    match (key.code, staged) {
        (KeyCode::Esc, staged) => {
            model.plugins.from_file = None;
            staged
                .map(|staged| {
                    vec![Effect::DropPluginFile {
                        token: staged.token,
                    }]
                })
                .unwrap_or_default()
        }
        (KeyCode::Enter, None) => {
            let path = from_file.path.as_str().trim().to_string();
            if path.is_empty() {
                return Vec::new();
            }
            from_file.error = None;
            vec![Effect::StagePluginFile { path }]
        }
        (KeyCode::Enter, Some(staged)) => {
            if from_file.confirm.as_str().trim() != staged.id {
                from_file.error = Some(format!("Type {} to install it.", staged.id));
                return Vec::new();
            }
            model.plugins.from_file = None;
            model.plugins.busy = Some(Busy {
                id: staged.id,
                verb: "Installing",
                done: 0,
                total: None,
            });
            vec![Effect::CommitPluginFile {
                token: staged.token,
            }]
        }
        (_, None) => {
            from_file.path.handle_key(key);
            Vec::new()
        }
        (_, Some(_)) => {
            from_file.confirm.handle_key(key);
            Vec::new()
        }
    }
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

/// A plugin offered where its driver was wanted: the connection form, or a saved
/// connection being opened.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginOffer {
    pub plugin: String,
    /// The driver as people name it.
    pub driver: String,
    pub focus: crate::widgets::form::FooterFocus,
    /// The connection to open once it is installed.
    pub then_connect: Option<dexo_app::ConnectionProfile>,
}

/// The plugin to install for `driver`, and the driver's name, when it is a plugin's
/// driver that is not installed.
pub fn missing_driver(driver: &str) -> Option<(String, String)> {
    let descriptor = dexo_driver_api::DriverDescriptor::for_id(driver.trim())?;
    if descriptor.installed {
        return None;
    }
    Some((
        descriptor.plugin?.into_owned(),
        descriptor.display_name.into_owned(),
    ))
}
