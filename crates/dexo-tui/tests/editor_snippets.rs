//! Insert Snippet lists something: nothing in the program makes a snippet, so with only
//! the person's own it said "no snippets available" and never anything else.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_tui::{Action, Focus, Model, update};

#[test]
fn insert_snippet_offers_the_built_in_snippets_and_inserts_one() {
    let mut model = Model {
        focus: Focus::Editor,
        ..Model::default()
    };
    update(&mut model, Action::SnippetsLoaded(Vec::new()));
    assert!(model.editor.snippet_open, "an empty list opened nothing");
    let screen = dexo_tui::render::render_to_string(&model, 100, 30);
    assert!(
        screen.contains("Snippets") && screen.contains("select"),
        "{screen}"
    );

    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(!model.editor.snippet_open);
    assert!(model.active_document().text().starts_with("SELECT"));
}

#[test]
fn a_snippet_of_the_persons_own_wins_on_its_name() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::SnippetsLoaded(vec![dexo_sql::Snippet {
            name: "select".into(),
            body: "select 1".into(),
        }]),
    );
    let selects: Vec<&str> = model
        .editor
        .snippets
        .iter()
        .filter(|snippet| snippet.name == "select")
        .map(|snippet| snippet.body.as_str())
        .collect();
    assert_eq!(selects, ["select 1"]);
}
