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
            name: "connection form",
            open: |m| {
                update(m, Action::OpenConnectionForm);
            },
            text: |m| {
                let form = &m.connection_form;
                form.fields[form.focus].value.as_str().to_string()
            },
            masked: false,
        },
        Field {
            name: "schema form",
            open: |m| {
                m.schema_editor = dexo_tui::screens::schema_editor::SchemaEditor::table_form("");
                m.schema_editor.open = true;
            },
            text: |m| m.schema_editor.field("target").to_string(),
            masked: false,
        },
        Field {
            name: "new row form",
            open: |m| {
                m.data.insert_form.open = true;
                m.data.insert_form.fields = vec![dexo_tui::screens::schema_editor::FormField {
                    label: "name".into(),
                    value: Default::default(),
                    secret: false,
                }];
            },
            text: |m| m.data.insert_form.fields[0].value.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "new MCP grant form",
            open: |m| {
                m.mcp_profiles.open = true;
                m.mcp_profiles.grant_form =
                    Some(dexo_tui::screens::mcp_profiles::GrantForm::new("local"));
            },
            text: |m| {
                let form = m.mcp_profiles.grant_form.as_ref().unwrap();
                form.fields[form.focus].value.as_str().to_string()
            },
            masked: false,
        },
        Field {
            name: "transfer path",
            open: |m| m.transfer.open = true,
            text: |m| m.transfer.path.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "project name",
            open: |m| {
                m.projects.open = true;
                m.projects.mode = dexo_tui::screens::projects::ProjectsMode::Create;
            },
            text: |m| m.projects.name_input.as_str().to_string(),
            masked: false,
        },
        Field {
            name: "project delete confirmation",
            open: |m| {
                m.projects.open = true;
                m.projects.mode = dexo_tui::screens::projects::ProjectsMode::DeleteConfirm;
                m.projects.delete = Some(dexo_tui::screens::projects::ProjectDeletePrompt {
                    project: dexo_app::Project {
                        id: dexo_app::ProjectId(uuid::Uuid::nil()),
                        name: "acme".into(),
                        created_at: String::new(),
                    },
                    preview: Default::default(),
                    delete_connections: false,
                    typed: Default::default(),
                });
            },
            text: |m| {
                let delete = m.projects.delete.as_ref().unwrap();
                delete.typed.as_str().to_string()
            },
            masked: false,
        },
        Field {
            name: "query parameter",
            open: |m| {
                m.active_document_mut().sql = dexo_sql::SqlDocument::new("select :n");
                m.editor.parameters = vec![dexo_tui::screens::editor::ParameterValue {
                    name: "n".into(),
                    value: dexo_driver_api::DbValue::Null,
                    sensitive: false,
                }];
                m.editor.parameter_prompt = true;
            },
            text: |m| m.editor.parameter_draft.as_str().to_string(),
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

/// Where the terminal's cursor is drawn, and the column of the last character of the
/// first `shown` on screen, with its row.
fn cursor_and_last_of(model: &Model, shown: &str) -> ((u16, u16), (u16, u16)) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    let frame = terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    let rows: Vec<String> = frame
        .buffer
        .content()
        .chunks(120)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let (y, row) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains(shown))
        .unwrap_or_else(|| panic!("{shown} is not on screen:\n{}", rows.join("\n")));
    let x = row[..row.find(shown).unwrap()].chars().count() + shown.chars().count() - 1;
    let cursor = terminal.get_cursor_position().unwrap();
    ((cursor.x, cursor.y), (x as u16, y as u16))
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
        // The cursor is drawn where the input has it.
        press(&mut model, KeyCode::Left, KeyModifiers::NONE);
        let (cursor, last) = cursor_and_last_of(&model, if field.masked { "***" } else { "abc" });
        assert_eq!(cursor, last, "the cursor in the {}", field.name);
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

/// A plain `c` toggled whether the project's connections go too, so a project with a
/// `c` in its name could never be typed to confirm. Alt+C toggles it now.
#[test]
fn a_project_named_with_a_c_can_be_deleted() {
    let mut model = workbench();
    let open = fields()
        .into_iter()
        .find(|field| field.name == "project delete confirmation")
        .unwrap()
        .open;
    open(&mut model);
    type_text(&mut model, "acme");
    press(&mut model, KeyCode::Char('c'), KeyModifiers::ALT);
    let delete = model.projects.delete.as_ref().unwrap();
    assert_eq!(delete.typed.as_str(), "acme");
    assert!(delete.delete_connections);
    let effects = update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            dexo_tui::Effect::DeleteProject {
                delete_connections: true,
                ..
            }
        )),
        "{effects:?}"
    );
}
