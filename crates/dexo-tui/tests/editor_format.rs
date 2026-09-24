use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::{Focus, Model, update};

fn editor_with(text: &str) -> Model {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    let doc = model.active_document_mut();
    doc.sql.insert(0, text).unwrap();
    model
}

fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) {
    update(model, Action::Key(KeyEvent::new(code, modifiers)));
}

const SQL: &str = "select id, name from users where id = 1";
const FORMATTED: &str = "SELECT\n  id,\n  name\nFROM\n  users\nWHERE\n  id = 1";

/// Alt+Shift+F arrives as Alt and a capital F on a terminal without the extended
/// keyboard protocol, and with Shift as well on one with it.
#[test]
fn alt_shift_f_formats_however_the_terminal_sends_it() {
    for (code, modifiers) in [
        (KeyCode::Char('F'), KeyModifiers::ALT),
        (KeyCode::Char('F'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        (KeyCode::Char('f'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        (
            KeyCode::Char('i'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    ] {
        let mut model = editor_with(SQL);
        press(&mut model, code, modifiers);
        assert_eq!(
            model.active_document().text(),
            FORMATTED,
            "{code:?}+{modifiers:?}"
        );
    }
}

/// Formatting replaced the document with a new untitled one: the file, the name, the
/// connection and the undo history went with it.
#[test]
fn the_document_stays_the_same_and_one_undo_takes_it_back() {
    let mut model = editor_with(SQL);
    {
        let doc = model.active_document_mut();
        doc.path = Some("/tmp/report.sql".into());
        doc.title = "report.sql".into();
        doc.connection_id = Some("conn-a".into());
    }
    let id = model.active_document().id.clone();
    update(&mut model, Action::FormatSql);

    let doc = model.active_document();
    assert_eq!(doc.text(), FORMATTED);
    assert_eq!(doc.id, id);
    assert_eq!(doc.title, "report.sql");
    assert_eq!(
        doc.path.as_deref(),
        Some(std::path::Path::new("/tmp/report.sql"))
    );
    assert_eq!(doc.connection_id.as_deref(), Some("conn-a"));

    press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(model.active_document().text(), SQL);
}

#[test]
fn a_selection_is_formatted_alone_and_stays_selected() {
    let text = format!("-- keep me as I am\n{SQL}");
    let mut model = editor_with(&text);
    let start = "-- keep me as I am\n".chars().count();
    {
        let doc = model.active_document_mut();
        doc.anchor = Some(start);
        doc.sql.set_cursor(text.chars().count()).unwrap();
    }
    update(&mut model, Action::FormatSql);

    let doc = model.active_document();
    assert_eq!(doc.text(), format!("-- keep me as I am\n{FORMATTED}"));
    assert_eq!(
        doc.selection(),
        Some(start..start + FORMATTED.chars().count())
    );
}
