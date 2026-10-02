use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::{Action, Model, update};

#[test]
fn f10_says_which_layout_it_is_on() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE)),
    );
    let last = model.messages.last().expect("no feedback").message.clone();
    assert!(last.starts_with("Layout 2 of 4"), "{last}");
}

/// Only the right border cell of the divider started a drag; the explorer's own did not.
#[test]
fn both_border_cells_of_a_divider_start_a_drag() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 36)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    let edge = dexo_tui::mouse::PaneEdge::Explorer;
    let x = model.panes.explorer_width;
    assert_eq!(
        hits.at(x - 1, 5),
        Some(dexo_tui::mouse::HitTarget::PaneDivider(edge))
    );
    assert_eq!(
        hits.at(x, 5),
        Some(dexo_tui::mouse::HitTarget::PaneDivider(edge))
    );
}

/// F1 spelled keys in lower case and listed the pane-size keys under Editor.
#[test]
fn help_spells_keys_like_the_palette_and_groups_layout_keys() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 60,
        },
    );
    update(&mut model, Action::ToggleHelp);
    model.help.query = dexo_tui::widgets::text_input::TextInput::new("execute document");
    let frame = dexo_tui::render::render_to_string(&model, 120, 60);
    assert!(frame.contains("Ctrl+Shift+F10"), "{frame}");
    assert!(!frame.contains("ctrl+shift+f10"), "{frame}");
    model.help.query = dexo_tui::widgets::text_input::TextInput::new("pane");
    let frame = dexo_tui::render::render_to_string(&model, 120, 60);
    assert!(frame.contains("[Layout]"), "{frame}");
    assert!(!frame.contains("[Editor]"), "{frame}");
}

#[test]
fn a_click_in_the_help_search_does_not_close_help() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    update(&mut model, Action::ToggleHelp);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 36)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let (column, row) = model.hits.center(dexo_tui::mouse::HitTarget::Overlay);
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(model.help.open);
}
