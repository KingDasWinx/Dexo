use std::time::{Duration, Instant};

use crate::model::Model;
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitTarget {
    ResultTab(usize),
    WorkbenchTab(usize),
    DocumentTab(usize),
    Explorer,
    ExplorerNode(usize),
    SidebarConnection(usize),
    Editor,
    Inspector,
    Grid,
    GridRow(usize),
    GridCell { row: usize, col: usize },
    GridHeader(usize),
    Overlay,
    ListRow(usize),
    FormField(usize),
    FooterSubmit,
    FooterCancel,
    Button(HitButton),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitButton {
    Close,
    Session,
    Keychain,
    Cancel,
    Theme,
    Keymap,
    Mouse,
    Reset,
    Pause,
    Resume,
    Confirm,
    Recover,
    Discard,
    Apply,
    ConfirmProduction,
    ToggleAdded,
    ToggleRemoved,
    ToggleChanged,
    ConfirmDiff,
    ApplyDiff,
    Export,
    Revoke,
    KeepSecrets,
    DeleteSecrets,
    ConfirmDirty,
    ToggleConnections,
    ConfirmDelete,
    New,
    Edit,
    Duplicate,
    Test,
    Delete,
    CloseSession,
    ParentDir,
    ToggleDescending,
    CycleDriver,
    ToggleAdvanced,
    GetStarted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayKind {
    Onboarding,
    Palette,
    Help,
    ResultsMenu,
    Review,
    DdlPreview,
    SchemaDiff,
    Transfer,
    Security,
    Admin,
    McpProfiles,
    Connections,
    Projects,
    ConfigTransfer,
    SecretPrompt,
    TransactionPrompt,
    DataQueryPrompt,
    ConnectionForm,
    Settings,
    Recovery,
    Diagnostics,
    McpAudit,
    FilePicker,
    Completion,
    Parameters,
    History,
    Snippets,
}

#[derive(Clone, Debug)]
pub struct LastClick {
    pub target: HitTarget,
    pub at: Instant,
}

impl PartialEq for LastClick {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitMap {
    targets: Vec<(HitTarget, Rect)>,
}

impl HitMap {
    pub fn register(&mut self, target: HitTarget, rect: Rect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.targets.push((target, rect));
    }

    pub fn clear(&mut self) {
        self.targets.clear();
    }

    pub fn at(&self, x: u16, y: u16) -> Option<HitTarget> {
        self.targets
            .iter()
            .rev()
            .find(|(_, rect)| {
                x >= rect.x
                    && y >= rect.y
                    && x < rect.x.saturating_add(rect.width)
                    && y < rect.y.saturating_add(rect.height)
            })
            .map(|(target, _)| *target)
    }

    pub fn center(&self, target: HitTarget) -> (u16, u16) {
        self.targets
            .iter()
            .find(|(candidate, _)| *candidate == target)
            .map(|(_, rect)| (rect.x + rect.width / 2, rect.y + rect.height / 2))
            .unwrap_or((0, 0))
    }
}

pub fn top_overlay(model: &Model) -> Option<OverlayKind> {
    if model.onboarding.open {
        return Some(OverlayKind::Onboarding);
    }

    [
        (model.editor.snippet_open, OverlayKind::Snippets),
        (model.editor.history_open, OverlayKind::History),
        (model.editor.parameter_prompt, OverlayKind::Parameters),
        (model.editor.completion_open, OverlayKind::Completion),
        (model.file_picker.open, OverlayKind::FilePicker),
        (model.mcp_audit.open, OverlayKind::McpAudit),
        (model.diagnostics.open, OverlayKind::Diagnostics),
        (model.recovery.open, OverlayKind::Recovery),
        (model.settings.open, OverlayKind::Settings),
        (model.connection_form.open, OverlayKind::ConnectionForm),
        (model.data.query_prompt.open, OverlayKind::DataQueryPrompt),
        (
            model.transaction_prompt.open,
            OverlayKind::TransactionPrompt,
        ),
        (model.secret_prompt.open, OverlayKind::SecretPrompt),
        (model.config_transfer.open, OverlayKind::ConfigTransfer),
        (model.projects.open, OverlayKind::Projects),
        (model.connections.open, OverlayKind::Connections),
        (model.mcp_profiles.open, OverlayKind::McpProfiles),
        (model.admin.open, OverlayKind::Admin),
        (model.security.open, OverlayKind::Security),
        (model.transfer.open, OverlayKind::Transfer),
        (model.schema_diff.open, OverlayKind::SchemaDiff),
        (
            model.schema_editor.preview.is_some(),
            OverlayKind::DdlPreview,
        ),
        (model.data.review.is_some(), OverlayKind::Review),
        (model.results_menu.open, OverlayKind::ResultsMenu),
        (model.help.open, OverlayKind::Help),
        (model.palette.open, OverlayKind::Palette),
    ]
    .into_iter()
    .find_map(|(open, kind)| open.then_some(kind))
}

pub fn overlay_blocks_workbench(model: &Model) -> bool {
    top_overlay(model).is_some()
}

pub fn popup_inner(popup: Rect) -> Rect {
    Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    )
}

pub fn line_rect(inner: Rect, row: usize) -> Rect {
    Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1)
}

pub fn register_overlay(hits: &mut HitMap, popup: Rect) {
    hits.register(HitTarget::Overlay, popup);
}

pub fn register_line(hits: &mut HitMap, inner: Rect, row: usize, target: HitTarget) {
    if (row as u16) >= inner.height {
        return;
    }
    hits.register(target, line_rect(inner, row));
}

pub fn register_label(hits: &mut HitMap, line: Rect, text: &str, needle: &str, target: HitTarget) {
    let Some(pos) = text.find(needle) else {
        return;
    };
    let x = line.x.saturating_add(pos as u16);
    if x >= line.x.saturating_add(line.width) {
        return;
    }
    let width = (needle.len() as u16).min(line.width.saturating_sub(x.saturating_sub(line.x)));
    hits.register(target, Rect::new(x, line.y, width, 1));
}

/// ponytail: crossterm 0.29 has no DoubleClick kind; 400ms same-target window.
pub fn note_click(model: &mut Model, target: HitTarget) -> bool {
    let now = Instant::now();
    let doubled = model.last_click.as_ref().is_some_and(|prev| {
        prev.target == target && now.duration_since(prev.at) <= Duration::from_millis(400)
    });
    model.last_click = Some(LastClick { target, at: now });
    doubled
}

#[cfg(test)]
mod tests {
    use super::{HitMap, HitTarget};
    use ratatui::layout::Rect;

    #[test]
    fn skips_zero_size_rects() {
        let mut map = HitMap::default();
        map.register(HitTarget::Editor, Rect::new(0, 0, 0, 4));
        map.register(HitTarget::Explorer, Rect::new(0, 0, 4, 0));
        assert_eq!(map.at(0, 0), None);
    }

    #[test]
    fn last_registered_wins() {
        let mut map = HitMap::default();
        map.register(HitTarget::Explorer, Rect::new(0, 0, 10, 5));
        map.register(HitTarget::Overlay, Rect::new(2, 1, 6, 3));
        assert_eq!(map.at(4, 2), Some(HitTarget::Overlay));
        assert_eq!(map.at(0, 0), Some(HitTarget::Explorer));
    }
}
