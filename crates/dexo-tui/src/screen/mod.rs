//! The full screens next to the workbench: what each draws, which keys it takes and what
//! its status line says. The state each one shows lives in `screens`, where the dialogs
//! they grew out of kept it.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::Paragraph;

use crate::action::Effect;
use crate::model::{Model, Screen};
use crate::mouse::{HitMap, HitTarget};

/// Commands a key may run while a screen other than the workbench is up. The rest act on
/// the documents, the editor or the panes, none of which is on screen: Ctrl+W closed a
/// document nobody could see.
pub fn screen_safe(command: &str) -> bool {
    command == "workbench.quit"
        || [
            "screen.",
            "palette.",
            "help.",
            "settings.",
            "mcp.",
            "project.",
            "recovery.",
            "diagnostics.",
            "config.",
        ]
        .iter()
        .any(|prefix| command.starts_with(prefix))
}

/// One name on the header's strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StripItem {
    pub screen: Screen,
    pub label: String,
    pub current: bool,
    pub waiting: usize,
}

/// Work waiting on a screen, said beside its name.
pub fn waiting(model: &Model, screen: Screen) -> usize {
    match screen {
        Screen::Agents => model.mcp_audit.announced.len(),
        _ => 0,
    }
}

/// The strip for `room` cells: every screen when they fit, else the current one and the
/// ones with work waiting, else the current one alone. The current one is bracketed, so
/// it reads without colour too.
pub fn strip(model: &Model, room: usize) -> Vec<StripItem> {
    let all: Vec<StripItem> = Screen::ALL
        .into_iter()
        .map(|screen| {
            let waiting = waiting(model, screen);
            let name = if waiting > 0 {
                format!("{} {waiting}", screen.title())
            } else {
                screen.title().to_string()
            };
            let current = model.screen == screen;
            StripItem {
                screen,
                label: if current {
                    format!("[{name}]")
                } else {
                    format!(" {name} ")
                },
                current,
                waiting,
            }
        })
        .collect();
    let width = |items: &[StripItem]| -> usize {
        items.iter().map(|item| item.label.chars().count()).sum()
    };
    if width(&all) <= room {
        return all;
    }
    let busy: Vec<StripItem> = all
        .iter()
        .filter(|item| item.current || item.waiting > 0)
        .cloned()
        .collect();
    if width(&busy) <= room {
        return busy;
    }
    all.into_iter().filter(|item| item.current).collect()
}

/// Draws the current screen in `area`, everything between the header and the status line.
pub fn render(frame: &mut Frame, area: Rect, model: &Model, hits: &mut HitMap) {
    let _ = hits;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let muted = model
        .theme
        .style(crate::theme::Role::Muted, model.capabilities);
    let text = format!("{} is on its way.", model.screen.title());
    let middle = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(muted),
        middle,
    );
}

/// The current screen's own keys, before the keymap's. None leaves the key to the
/// keymap, and Esc to going back.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> Option<Vec<Effect>> {
    let _ = (model, key);
    None
}

/// A click on the current screen.
pub fn mouse(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    let _ = (model, hit, doubled);
    Vec::new()
}

/// What the status line says on the current screen, after its name.
pub fn hints(model: &Model) -> String {
    let _ = model;
    "Esc back".into()
}
