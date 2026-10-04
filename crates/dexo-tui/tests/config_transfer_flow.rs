//! Import / Export Config, as a person meets it: a dialog that opens fresh, asks before it
//! replaces a file, shows every clash in words with the commands it would run, and says
//! what it did.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_storage::ImportPreview;
use dexo_tui::action::{Action, Effect};
use dexo_tui::{Model, update};

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn screen(model: &Model) -> String {
    dexo_tui::render::render_to_string(model, 120, 40)
}

fn preview(clashes: usize) -> ImportPreview {
    let names: Vec<String> = (0..clashes).map(|n| format!("conn-{n:02}")).collect();
    let mut incoming = names.clone();
    incoming.extend([
        "new-1".to_string(),
        "new-2".to_string(),
        "new-3".to_string(),
    ]);
    ImportPreview {
        conflicts: names.clone(),
        incoming,
        existing: names,
        connections_needing_secret: vec!["new-1".into(), "new-2".into()],
        commands: vec![
            "new-1 runs `touch /tmp/claude-1000/qa/marker` before it connects".into(),
            "new-2 runs `cat /tmp/claude-1000/qa/pw` for its password".into(),
        ],
    }
}

fn opened() -> Model {
    let mut model = Model::default();
    model.apply_size(120, 40);
    update(&mut model, Action::OpenConfigTransfer);
    model
}

/// 15 clashes were a list cut at the 14th line, with `Rename("bad-creds-2")` Debug text, no
/// keys named and the warning about commands out of reach.
#[test]
fn every_clash_is_shown_in_words_with_the_commands_that_would_run() {
    let mut model = opened();
    update(&mut model, Action::ConfigPreviewed(preview(15)));
    let first = screen(&model);
    assert!(first.contains("> conn-00: skip it"), "{first}");
    assert!(
        !first.contains("Skip") && !first.contains("Rename("),
        "{first}"
    );
    assert!(
        first.contains("[Import]") && first.contains("[Cancel]"),
        "{first}"
    );
    assert!(
        first.contains("Space change"),
        "the keys are named:\n{first}"
    );
    // Walking down brings the last clash into view, and the commands stay reachable.
    for _ in 0..14 {
        press(&mut model, KeyCode::Down);
    }
    let last = screen(&model);
    assert!(last.contains("> conn-14: skip it"), "{last}");
    let all = {
        let mut text = String::new();
        for _ in 0..10 {
            text.push_str(&screen(&model));
            press(&mut model, KeyCode::PageDown);
        }
        text
    };
    assert!(all.contains("touch /tmp/claude-1000/qa/marker"), "{all}");
    assert!(all.contains("cat /tmp/claude-1000/qa/pw"), "{all}");
    // Space changes what happens to the picked one, in words.
    press(&mut model, KeyCode::Char(' '));
    assert!(screen(&model).contains("conn-14: overwrite the saved one"));
    press(&mut model, KeyCode::Char('r'));
    assert!(screen(&model).contains("conn-14: keep both, import it as conn-14-2"));
}

/// The file picker asks before it replaces a file; the dialog does not ask a second time.
#[test]
fn an_export_over_an_existing_file_is_asked_about_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "kept").unwrap();
    let mut model = opened();
    let effects = update(&mut model, Action::ExportConfig { path: path.clone() });
    assert!(
        matches!(effects.as_slice(), [Effect::ExportConfig { path: given }] if *given == path),
        "{effects:?}"
    );
}

/// What an earlier import left on the dialog is not there when it is opened again.
#[test]
fn the_dialog_opens_fresh() {
    let mut model = opened();
    update(&mut model, Action::ConfigPreviewed(preview(3)));
    press(&mut model, KeyCode::Char(' '));
    update(&mut model, Action::OpenConfigTransfer);
    assert!(model.config_transfer.preview.is_none());
    assert!(model.config_transfer.resolutions.is_empty());
    assert!(model.config_transfer.message.is_none());
    let view = screen(&model);
    assert!(
        view.contains("[Export]") && view.contains("[Import]"),
        "{view}"
    );
    assert!(!view.contains("conn-00"), "{view}");
}

/// Importing says what it did, and what is left to do.
#[test]
fn an_import_says_what_it_did() {
    let mut model = opened();
    update(&mut model, Action::ConfigPreviewed(preview(2)));
    press(&mut model, KeyCode::Char(' '));
    update(
        &mut model,
        Action::ConfigImported {
            needing_secret: vec!["new-1".into(), "new-2".into()],
            commands: vec!["new-1 runs `touch /tmp/x` before it connects".into()],
        },
    );
    let view = screen(&model);
    assert!(
        view.contains("Imported 4 connections: 3 new, 1 overwritten, 1 skipped."),
        "{view}"
    );
    assert!(view.contains("new-1, new-2"), "{view}");
    assert!(view.contains("touch /tmp/x"), "{view}");
}
