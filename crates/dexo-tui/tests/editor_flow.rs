use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::update;

fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Action {
    Action::Key(KeyEvent::new(code, modifiers))
}

fn ctrl(ch: char) -> Action {
    key_mod(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn model_with_sql(sql: &str) -> Model {
    let mut model = Model::default();
    model.absorb_catalog(&[dexo_driver_api::CatalogObject::new(
        dexo_driver_api::ObjectId::new("table:users"),
        dexo_driver_api::ObjectKind::Table,
        dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "users"),
        None,
    )]);
    model.set_sql(sql);
    model
}

#[test]
fn editor_highlights_formats_completes_and_prompts_for_parameters() {
    let mut model = model_with_sql("select * from users where id = :id");
    update(&mut model, Action::RefreshSqlIntelligence);
    assert!(
        model
            .editor
            .highlights
            .iter()
            .any(|span| span.kind == dexo_sql::Highlight::Keyword)
    );
    assert_eq!(
        model
            .editor
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["id"]
    );
    model.set_sql("select * from u");
    update(&mut model, Action::RefreshSqlIntelligence);
    assert!(
        model
            .editor
            .completions
            .iter()
            .any(|item| item.label == "users")
    );
}

#[test]
fn typing_opens_completion_and_tab_replaces_token() {
    let mut model = Model::default();
    model.focus = dexo_tui::model::Focus::Editor;
    send_text(&mut model, "sel");
    assert!(model.editor.completion_open);
    assert!(
        model
            .editor
            .completions
            .iter()
            .any(|item| item.label == "SELECT")
    );
    update(&mut model, key(KeyCode::Tab));
    assert_eq!(model.active_document().text(), "SELECT");
    assert!(!model.editor.completion_open);
}

#[test]
fn editor_formats_inserts_snippet_and_keeps_history_sql_only() {
    let mut model = model_with_sql("select 1");
    update(&mut model, Action::FormatSql);
    assert!(
        model
            .active_document()
            .text()
            .to_ascii_lowercase()
            .contains("select")
    );
    model.editor.snippets.push(dexo_sql::Snippet {
        name: "sel".into(),
        body: "select ${1:*} from t".into(),
    });
    model.set_sql("");
    update(&mut model, Action::InsertSnippet);
    assert_eq!(model.active_document().text(), "select * from t");
    let effects = update(
        &mut model,
        Action::ScriptFinished {
            key: dexo_tui::runtime::OperationKey::new(
                dexo_tui::runtime::OperationId::new(),
                "",
                "scratch",
                1,
            ),
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            dexo_tui::Effect::PersistHistory(request) if request.sql.contains("select") && !request.sql.contains("secret")
        ))
    );
}

fn send_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        update(model, key(KeyCode::Char(ch)));
    }
}

#[test]
fn editor_types_unicode_moves_and_undoes() {
    let mut model = Model::default();
    send_text(&mut model, "select 'ação'");
    assert_eq!(model.active_document().text(), "SELECT 'ação'");
    update(&mut model, ctrl('z'));
    assert_eq!(model.active_document().text(), "");
}

#[test]
fn editor_arrows_home_end_and_word_motion() {
    let mut model = Model::default();
    send_text(&mut model, "select from users");
    update(&mut model, key(KeyCode::Home));
    assert_eq!(model.active_document().cursor(), 0);
    update(&mut model, key(KeyCode::End));
    assert_eq!(
        model.active_document().cursor(),
        "select from users".chars().count()
    );
    update(&mut model, key_mod(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(
        model.active_document().cursor(),
        "select from ".chars().count()
    );
    update(&mut model, key(KeyCode::Left));
    assert_eq!(
        model.active_document().cursor(),
        "select from".chars().count()
    );
    update(&mut model, key(KeyCode::Right));
    update(&mut model, key_mod(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(
        model.active_document().cursor(),
        "select from users".chars().count()
    );
}

#[test]
fn editor_backspace_delete_and_shift_selection() {
    let mut model = Model::default();
    send_text(&mut model, "abcd");
    update(&mut model, key(KeyCode::Left));
    update(&mut model, key(KeyCode::Backspace));
    assert_eq!(model.active_document().text(), "abd");
    update(&mut model, key(KeyCode::Home));
    update(&mut model, key(KeyCode::Delete));
    assert_eq!(model.active_document().text(), "bd");
    update(&mut model, key_mod(KeyCode::Right, KeyModifiers::SHIFT));
    send_text(&mut model, "x");
    assert_eq!(model.active_document().text(), "xd");
}

#[test]
fn editor_select_all_indent_tab_and_redo() {
    let mut model = Model::default();
    send_text(&mut model, "select 1");
    update(&mut model, ctrl('a'));
    assert_eq!(
        model.active_document().selection(),
        Some(0.."select 1".chars().count())
    );
    update(&mut model, key(KeyCode::End));
    update(&mut model, key(KeyCode::Enter));
    assert_eq!(model.active_document().text(), "SELECT 1\n");
    send_text(&mut model, "  two");
    update(&mut model, key(KeyCode::Enter));
    assert_eq!(model.active_document().text(), "SELECT 1\n  two\n  ");
    update(&mut model, key(KeyCode::Tab));
    assert_eq!(model.active_document().text(), "SELECT 1\n  two\n      ");
    update(&mut model, ctrl('z'));
    update(&mut model, ctrl('y'));
    assert_eq!(model.active_document().text(), "SELECT 1\n  two\n      ");
}

#[test]
fn editor_scrolls_cursor_into_view() {
    let mut model = Model::default();
    for _ in 0..80 {
        update(&mut model, key(KeyCode::Enter));
    }
    let doc = model.active_document();
    let line = doc.text().matches('\n').count();
    assert!(doc.viewport_line > 0, "viewport should follow cursor");
    assert!(line >= doc.viewport_line);
}

#[tokio::test]
async fn save_is_atomic_and_external_change_requires_resolution() {
    use dexo_tui::runtime::document_io::{
        DocumentIoError, fingerprint, save_if_unchanged, save_sql_atomic,
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("query.sql");
    save_sql_atomic(&path, "select 1").await.unwrap();
    let first = fingerprint(&path).await.unwrap();
    tokio::fs::write(&path, "select 2").await.unwrap();
    let error = save_if_unchanged(&path, &first, "select 3")
        .await
        .unwrap_err();
    assert!(matches!(error, DocumentIoError::ExternalConflict { .. }));
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "select 2");
}

#[test]
fn completion_popup_accepts_selected_item() {
    let mut model = model_with_sql("select * from ");
    update(&mut model, Action::RefreshSqlIntelligence);
    assert!(model.editor.completion_open);
    assert!(!model.editor.completions.is_empty());
    model.editor.completion_selected = model
        .editor
        .completions
        .iter()
        .position(|item| item.label == "users")
        .unwrap_or(0);
    update(&mut model, Action::AcceptCompletion);
    assert!(model.active_document().text().contains("users"));
}

#[test]
fn history_overlay_enter_reruns() {
    let mut model = Model::default();
    model.editor.history = vec!["select 9".into()];
    update(&mut model, Action::HistoryLoaded(vec!["select 9".into()]));
    assert!(model.editor.history_open);
    update(&mut model, Action::HistoryPick);
    assert_eq!(model.active_document().text(), "select 9");
    assert!(!model.editor.history_open);
}

fn choose_effects(model: &mut Model, query: &str) -> Vec<dexo_tui::Effect> {
    let mut effects = update(model, Action::OpenPalette);
    for ch in query.chars() {
        effects.extend(update(model, key(KeyCode::Char(ch))));
    }
    effects.extend(update(model, key(KeyCode::Enter)));
    effects
}

fn connected_model_with_sql(sql: &str) -> Model {
    let mut model = model_with_sql(sql);
    model.active_session = Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1)));
    model
}

#[test]
fn insert_snippet_loads_storage_before_opening_picker() {
    let mut model = Model::default();
    let effects = choose_effects(&mut model, "editor.snippet");
    assert!(model.editor.snippet_pending);
    assert!(matches!(
        effects.as_slice(),
        [dexo_tui::Effect::LoadSnippets]
    ));
}

#[test]
fn submit_parameters_outside_prompt_never_executes_query() {
    let mut model = connected_model_with_sql("select 1");
    let effects = update(&mut model, Action::SubmitParameters);
    assert!(effects.is_empty());
    assert!(model.active_operation.is_none());
}

/// Save Query As names the selection, for the project and the document's connection;
/// Open Saved Query searches, opens the highlighted one in a new document of its
/// connection, renames with F2, and deletes only after a second, deliberate answer.
#[test]
fn saved_queries_save_search_open_rename_and_delete() {
    let mut model = model_with_sql("select 1;\nselect * from users where id = 7;");
    model.project_id = "project-1".into();
    let profile = |id: u128, name: &str| {
        dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::from_u128(id)),
            None,
            name,
            "postgres",
            "local",
            serde_json::json!({"host": "h", "port": 5432, "username": "u", "database": "d"}),
            dexo_app::SecretRef::new(format!("ref-{id}")),
        )
    };
    model
        .connections
        .load_profiles(vec![profile(0xa, "shop"), profile(0xb, "warehouse")]);
    let conn_a = uuid::Uuid::from_u128(0xa).to_string();
    let conn_b = uuid::Uuid::from_u128(0xb).to_string();
    model.active_document_mut().connection_id = Some(conn_a.clone());
    let start = "select 1;\n".chars().count();
    let end = "select 1;\nselect * from users where id = 7;"
        .chars()
        .count();
    model.active_document_mut().anchor = Some(start);
    model.active_document_mut().sql.set_cursor(end).unwrap();
    assert!(update(&mut model, Action::OpenSaveQuery).is_empty());
    // The suggested name goes; a typed one takes its place.
    for _ in 0..40 {
        update(&mut model, key(KeyCode::Backspace));
    }
    for ch in "User 7".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(
        matches!(
            effects.as_slice(),
            [dexo_tui::Effect::SaveQuery { project_id, connection_id, name, sql }]
                if project_id == "project-1"
                    && *connection_id == conn_a
                    && name == "User 7"
                    && sql == "select * from users where id = 7;"
        ),
        "{effects:?}"
    );
    assert!(model.save_query_prompt.is_none());

    let effects = update(&mut model, Action::OpenSavedQueries);
    assert!(matches!(
        effects.as_slice(),
        [dexo_tui::Effect::LoadSavedQueries { project_id }] if project_id == "project-1"
    ));
    let saved = |id: &str, name: &str, connection: &str, sql: &str| dexo_storage::SavedQuery {
        id: id.into(),
        project_id: "project-1".into(),
        connection_id: connection.into(),
        name: name.into(),
        sql: sql.into(),
        updated_at: String::new(),
    };
    update(
        &mut model,
        Action::SavedQueriesLoaded(Ok(vec![
            saved(
                "q1",
                "Late orders",
                &conn_a,
                "select * from orders where late",
            ),
            saved("q2", "User 7", &conn_a, "select * from users where id = 7;"),
            saved("q3", "Users elsewhere", &conn_b, "select * from users"),
        ])),
    );
    let screen = dexo_tui::render::render_to_string(&model, 110, 30);
    assert!(screen.contains("Open saved query"), "{screen}");
    assert!(screen.contains("> Late orders"), "{screen}");
    assert!(
        screen.contains("select * from orders where late"),
        "{screen}"
    );
    assert!(screen.contains("Users elsewhere · warehouse"), "{screen}");
    for ch in "users".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    assert_eq!(model.saved_queries.filtered().len(), 2);
    update(&mut model, key(KeyCode::Down));
    assert_eq!(
        model.saved_queries.current().map(|query| query.id.as_str()),
        Some("q3")
    );

    update(&mut model, key(KeyCode::F(2)));
    for ch in " (b)".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(
        matches!(
            effects.as_slice(),
            [dexo_tui::Effect::RenameSavedQuery { id, name, .. }]
                if id == "q3" && name == "Users elsewhere (b)"
        ),
        "{effects:?}"
    );
    // The field stays until the list comes back renamed.
    assert!(model.saved_queries.renaming.is_some());
    update(
        &mut model,
        Action::SavedQueriesLoaded(Ok(vec![
            saved(
                "q1",
                "Late orders",
                &conn_a,
                "select * from orders where late",
            ),
            saved("q2", "User 7", &conn_a, "select * from users where id = 7;"),
            saved("q3", "Users elsewhere (b)", &conn_b, "select * from users"),
        ])),
    );
    assert!(model.saved_queries.renaming.is_none());

    // Delete asks; Enter on the question keeps the query, [Delete] removes it.
    update(&mut model, key(KeyCode::Delete));
    assert!(update(&mut model, key(KeyCode::Enter)).is_empty());
    assert!(model.saved_queries.deleting.is_none());
    update(&mut model, key(KeyCode::Delete));
    update(&mut model, key(KeyCode::Left));
    let effects = update(&mut model, key(KeyCode::Enter));
    assert!(
        matches!(effects.as_slice(), [dexo_tui::Effect::DeleteSavedQuery { id, .. }] if id == "q3"),
        "{effects:?}"
    );

    let documents = model.documents.len();
    update(&mut model, key(KeyCode::Up));
    update(&mut model, key(KeyCode::Enter));
    assert!(!model.saved_queries.open);
    assert_eq!(model.documents.len(), documents + 1);
    let opened = model.active_document();
    assert_eq!(opened.text(), "select * from users where id = 7;");
    assert_eq!(opened.connection_id.as_deref(), Some(conn_a.as_str()));
    assert_eq!(opened.title, "User 7.sql");
}

/// A long query of wide characters keeps the find bar's cursor on the bar, after the
/// text it was typed after.
#[test]
fn the_find_bar_cursor_counts_display_columns() {
    let mut model = Model {
        focus: dexo_tui::Focus::Editor,
        ..Model::default()
    };
    model.set_sql("select 1");
    model.find.open = true;
    model.find.query.set_text("日本語".repeat(20));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    let cells: Vec<Vec<String>> = terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap()
        .buffer
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol().to_string()).collect())
        .collect();
    let rows: Vec<String> = cells.iter().map(|row| row.concat()).collect();
    let bar = rows
        .iter()
        .position(|row| row.contains("Find    "))
        .expect("the find bar is drawn");
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!(cursor.y as usize, bar);
    let row = &rows[bar];
    let label = row[..row.find("Find    ").unwrap()].chars().count() + "Find    ".len();
    // The cursor is at the end of the text shown: on the cell after the last 語.
    assert!(cursor.x as usize > label);
    assert_eq!(cells[bar][cursor.x as usize - 2], "語");
}
