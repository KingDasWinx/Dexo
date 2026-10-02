//! Every single-line input edits alike: Ctrl+A selects its text and shows it, typing
//! replaces the selection, Ctrl+W deletes a word, and a letter typed with Ctrl is a
//! shortcut, never text. Several fields only appended and deleted from the end, and
//! one typed the `a` of Ctrl+A.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::update::update;

struct Field {
    name: &'static str,
    open: fn(&mut Model),
    text: fn(&Model) -> String,
    /// Drawn as one mark per character.
    masked: bool,
}

fn fields() -> Vec<Field> {
    vec![
        Field {
            name: "command palette",
            open: |m| {
                update(m, Action::OpenPalette);
            },
            text: |m| m.palette.query.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "keybindings search",
            open: |m| {
                update(m, Action::ToggleHelp);
            },
            text: |m| m.help.query.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "savepoint prompt",
            open: |m| m.transaction_prompt.open = true,
            text: |m| m.transaction_prompt.name.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "secret prompt",
            open: |m| m.secret_prompt.open = true,
            text: |m| m.secret_prompt.buffer.expose().to_string(),
            masked: true,
        },
    ]
}

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) {
    update(model, Action::Key(KeyEvent::new(code, modifiers)));
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch), KeyModifiers::NONE);
    }
}

/// The cells a frame draws in reverse video, in order.
fn reversed(model: &Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    let frame = terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    frame
        .buffer
        .content()
        .iter()
        .filter(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED))
        .map(|cell| cell.symbol())
        .collect()
}

fn workbench() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 40);
    model
}

#[test]
fn ctrl_a_selects_shows_and_is_replaced_by_typing() {
    for field in fields() {
        let mut model = workbench();
        (field.open)(&mut model);
        type_text(&mut model, "abc");
        assert_eq!(
            (field.text)(&model),
            "abc",
            "typing into the {}",
            field.name
        );
        press(&mut model, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            (field.text)(&model),
            "abc",
            "Ctrl+A typed into the {}",
            field.name
        );
        assert!(
            reversed(&model).contains(if field.masked { "***" } else { "abc" }),
            "the {} does not show its selection",
            field.name
        );
        type_text(&mut model, "z");
        assert_eq!(
            (field.text)(&model),
            "z",
            "typing over the {}'s selection",
            field.name
        );
    }
}

#[test]
fn the_word_keys_edit_and_ctrl_letters_are_not_text() {
    for field in fields() {
        let mut model = workbench();
        (field.open)(&mut model);
        type_text(&mut model, "ab cd");
        press(&mut model, KeyCode::Left, KeyModifiers::CONTROL);
        type_text(&mut model, "x");
        assert_eq!(
            (field.text)(&model),
            "ab xcd",
            "Ctrl+Left in the {}",
            field.name
        );
        press(&mut model, KeyCode::Right, KeyModifiers::CONTROL);
        press(&mut model, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!((field.text)(&model), "ab ", "Ctrl+W in the {}", field.name);
        press(&mut model, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(
            (field.text)(&model),
            "ab ",
            "Ctrl+K typed into the {}",
            field.name
        );
    }
}
