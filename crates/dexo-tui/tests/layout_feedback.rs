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

/// The close prompt spaced its buttons with three spaces; every other dialog uses the
/// marker slot and two.
#[test]
fn the_close_prompt_spaces_its_buttons_like_the_other_dialogs() {
    let mut model = Model::default();
    model.documents[0].sql.insert(0, "select 1").unwrap();
    update(&mut model, Action::CloseDocument);
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(frame.contains(">[Save]  [Don't save]  [Cancel]"), "{frame}");
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

/// The palette's query was clipped at the border and the typed end was never seen.
#[test]
fn a_long_palette_query_scrolls_to_its_end() {
    let mut model = Model::default();
    update(&mut model, Action::OpenPalette);
    let long = format!("{}END", "a long palette query ".repeat(8));
    update(&mut model, Action::PaletteQuery(long));
    let frame = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(frame.contains("END"), "{frame}");
}

/// Home and End did nothing in the document strip.
#[test]
fn home_and_end_walk_the_document_strip() {
    use dexo_tui::model::{EditorDocument, Focus};
    let mut model = Model {
        documents: (1..=4)
            .map(|n| EditorDocument::new_unique(format!("q{n}.sql"), None, None))
            .collect(),
        ..Model::default()
    };
    model.active_document = 1;
    model.focus = Focus::DocumentTabs;
    let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));

    update(&mut model, key(KeyCode::End));
    assert_eq!(model.active_document, 3);
    assert_eq!(model.focus, Focus::DocumentTabs);
    update(&mut model, key(KeyCode::Home));
    assert_eq!(model.active_document, 0);
}

/// A right click on a tab or in the editor did nothing, not even move the focus.
#[test]
fn a_right_click_on_the_editor_focuses_it() {
    use dexo_tui::model::Focus;
    let mut model = Model {
        focus: Focus::Explorer,
        ..Model::default()
    };
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
    model.hits = hits;
    let (column, row) = model.hits.center(dexo_tui::mouse::HitTarget::Editor);
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(model.focus, Focus::Editor);
}

/// Behind Settings and Help the status bar kept the editor's hints.
#[test]
fn a_dialog_clears_the_status_bar_hints() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::Resize {
            width: 120,
            height: 36,
        },
    );
    update(&mut model, Action::NewDocument);
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    let before = dexo_tui::render::render_to_string(&model, 120, 36);
    assert!(
        before.lines().last().unwrap_or_default().contains("run"),
        "{before}"
    );
    model.settings.open = true;
    let behind = dexo_tui::render::render_to_string(&model, 120, 36);
    assert!(
        !behind.lines().last().unwrap_or_default().contains(" run"),
        "{behind}"
    );
}
