use std::time::{Duration, Instant};

use crate::model::Model;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneEdge {
    Explorer,
    Results,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitTarget {
    /// A name on the header's strip of screens.
    ScreenTab(crate::model::Screen),
    /// One of a screen's views, on the row under the header.
    ScreenView(usize),
    /// A screen's detail pane: the wheel over it reads on, elsewhere it moves the pick.
    ScreenDetail,
    ResultTab(usize),
    ResultsView(usize),
    DocumentTab(usize),
    DocumentTabClose(usize),
    DocumentTabNew,
    DocumentTabScrollPrev,
    DocumentTabScrollNext,
    Explorer,
    ExplorerNode(usize),
    /// The arrow before a node's name: a click on it opens or closes the node.
    ExplorerTwistie(usize),
    SidebarConnection(usize),
    Editor,
    PaneDivider(PaneEdge),
    Grid,
    Console,
    GridRow(usize),
    GridCell {
        row: usize,
        col: usize,
    },
    GridHeader(usize),
    ClauseBar(crate::screens::data::ClauseBar),
    RecentSqlFile(usize),
    Overlay,
    ListRow(usize),
    /// One of the values a settings row lists side by side: `index` into that row's.
    SettingsChoice {
        row: usize,
        index: usize,
    },
    FormField(usize),
    /// A click on the `<` (step -1) or `>` (step 1) of a field picked from a list.
    FormChoice {
        index: usize,
        step: i8,
    },
    FooterSubmit,
    FooterCancel,
    Button(HitButton),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitButton {
    Close,
    Keychain,
    Cancel,
    Theme,
    Keymap,
    Mouse,
    Reset,
    Confirm,
    Recover,
    Discard,
    Apply,
    ToggleAdded,
    ToggleRemoved,
    ToggleChanged,
    Export,
    Revoke,
    ConfirmDirty,
    ToggleConnections,
    ConfirmDelete,
    New,
    Edit,
    Actions,
    Duplicate,
    Test,
    Delete,
    CloseSession,
    Connect,
    Docker,
    ParentDir,
    ToggleDescending,
    ToggleAdvanced,
    GetStarted,
    /// The edit-cell dialog's NULL and Editor buttons.
    SetNull,
    OpenEditor,
    /// The `[Browse]` button of a dialog's file field.
    Browse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayKind {
    Onboarding,
    Palette,
    Help,
    ResultsMenu,
    NodeMenu,
    ClosePrompt,
    RunPrompt,
    ProductionPrompt,
    ExplainPrompt,
    QuitPrompt,
    DeleteConnection,
    Review,
    DdlPreview,
    Transfer,
    Security,
    ValueViewer,
    ObjectOverlay,
    SchemaForm,
    InsertRow,
    CellEdit,
    Projects,
    ConfigTransfer,
    SecretPrompt,
    TransactionPrompt,
    DocumentNamePrompt,
    ConnectionForm,
    Settings,
    Recovery,
    Diagnostics,
    FilePicker,
    Completion,
    Parameters,
    ClearHistory,
    Snippets,
    Related,
    SaveQuery,
    TryIndex,
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

/// Text views that scroll by line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollArea {
    Help,
    Inspector,
    Explain,
    McpAudit,
    Value,
    Review,
    Messages,
    DdlPreview,
    McpProfiles,
    Sessions,
    SchemaDiff,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitMap {
    targets: Vec<(HitTarget, Rect)>,
    scroll_limits: Vec<(ScrollArea, u16)>,
    pages: Vec<(ScrollArea, u16)>,
}

impl HitMap {
    /// The furthest a view can scroll, as last drawn. Only the render knows how many lines
    /// a view has and how tall it came out; without the bound, every key past the end kept
    /// counting, and scrolling back took as many presses before anything moved.
    pub fn set_scroll_limit(&mut self, area: ScrollArea, max: usize) {
        let max = u16::try_from(max).unwrap_or(u16::MAX);
        self.scroll_limits.retain(|(known, _)| *known != area);
        self.scroll_limits.push((area, max));
    }

    /// How many lines a view showed, as last drawn: a page of it.
    pub fn set_page(&mut self, area: ScrollArea, rows: u16) {
        self.pages.retain(|(known, _)| *known != area);
        self.pages.push((area, rows.max(1)));
    }

    /// A page of `area`, or `fallback` before it is drawn. A page taller than the view
    /// skipped lines nobody saw.
    pub fn page(&self, area: ScrollArea, fallback: u16) -> u16 {
        self.pages
            .iter()
            .find(|(known, _)| *known == area)
            .map_or(fallback, |(_, rows)| *rows)
    }

    /// Moves `scroll` by `delta` lines, within the bound the last frame recorded. A view
    /// not drawn yet has no bound, and only the lower one applies.
    pub fn scroll(&self, area: ScrollArea, scroll: u16, delta: i32) -> u16 {
        let moved = (i32::from(scroll) + delta).max(0);
        let limit = self
            .scroll_limits
            .iter()
            .find(|(known, _)| *known == area)
            .map_or(i32::from(u16::MAX), |(_, max)| i32::from(*max));
        moved.min(limit) as u16
    }

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
        // A question about losing work sits above everything else.
        (model.close_prompt.is_some(), OverlayKind::ClosePrompt),
        (model.run_prompt.is_some(), OverlayKind::RunPrompt),
        (
            model.production_prompt.is_some(),
            OverlayKind::ProductionPrompt,
        ),
        (model.explain_prompt.is_some(), OverlayKind::ExplainPrompt),
        (model.quit_prompt.is_some(), OverlayKind::QuitPrompt),
        (
            model.connections.delete_target.is_some(),
            OverlayKind::DeleteConnection,
        ),
        (model.editor.snippet_open, OverlayKind::Snippets),
        (model.data.related_picker.is_some(), OverlayKind::Related),
        (model.save_query_prompt.is_some(), OverlayKind::SaveQuery),
        (model.try_index.is_some(), OverlayKind::TryIndex),
        (
            model.editor.history_confirm_clear,
            OverlayKind::ClearHistory,
        ),
        (model.editor.parameter_prompt, OverlayKind::Parameters),
        // The preview of a change is above whatever asked for it: the Security panel
        // stayed on top of it, and took its clicks.
        (
            model.schema_editor.preview.is_some(),
            OverlayKind::DdlPreview,
        ),
        (model.schema_editor.open, OverlayKind::SchemaForm),
        (model.inspector.open, OverlayKind::ObjectOverlay),
        (model.data.viewer.is_some(), OverlayKind::ValueViewer),
        (model.editor.completion_open, OverlayKind::Completion),
        (model.file_picker.open, OverlayKind::FilePicker),
        (model.diagnostics.open, OverlayKind::Diagnostics),
        (model.recovery.open, OverlayKind::Recovery),
        (model.settings.open, OverlayKind::Settings),
        (model.connection_form.open, OverlayKind::ConnectionForm),
        (model.data.insert_form.open, OverlayKind::InsertRow),
        (model.data.cell_edit.is_some(), OverlayKind::CellEdit),
        (
            model.transaction_prompt.open,
            OverlayKind::TransactionPrompt,
        ),
        (
            model.document_name_prompt.open,
            OverlayKind::DocumentNamePrompt,
        ),
        (model.secret_prompt.open, OverlayKind::SecretPrompt),
        (model.config_transfer.open, OverlayKind::ConfigTransfer),
        (model.projects.open, OverlayKind::Projects),
        (model.security.open, OverlayKind::Security),
        (model.transfer.open, OverlayKind::Transfer),
        (model.data.review.is_some(), OverlayKind::Review),
        (model.results_menu.open, OverlayKind::ResultsMenu),
        (model.node_menu.open, OverlayKind::NodeMenu),
        (model.help.open, OverlayKind::Help),
        (model.palette.open, OverlayKind::Palette),
    ]
    .into_iter()
    .find_map(|(open, kind)| open.then_some(kind))
}

/// The completion popup is not modal: clicks and the wheel away from it still reach the
/// editor, so the workbench keeps its hit areas while it is open.
pub fn overlay_blocks_workbench(model: &Model) -> bool {
    !matches!(top_overlay(model), None | Some(OverlayKind::Completion))
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
    let x = line
        .x
        .saturating_add(text[..pos].width().try_into().unwrap_or(u16::MAX));
    if x >= line.x.saturating_add(line.width) {
        return;
    }
    let width = needle
        .width()
        .try_into()
        .unwrap_or(u16::MAX)
        .min(line.width.saturating_sub(x.saturating_sub(line.x)));
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
