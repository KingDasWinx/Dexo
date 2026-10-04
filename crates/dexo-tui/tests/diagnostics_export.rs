use dexo_tui::{Action, Model, update};

#[test]
fn the_diagnostics_preview_is_words_not_debug_text() {
    let mut model = Model::default();
    update(&mut model, Action::OpenDiagnostics);
    let preview = model.diagnostics.preview.clone();
    assert!(!preview.contains("TerminalCapabilities"), "{preview}");
    assert!(!preview.contains("mode=dark"), "{preview}");
}

#[test]
fn the_save_says_where_the_bundle_went() {
    let mut model = Model::default();
    update(&mut model, Action::OpenDiagnostics);
    update(
        &mut model,
        Action::DiagnosticsWritten {
            path: "/tmp/dexo-diagnostics.zip".into(),
        },
    );
    let message = &model.messages.last().expect("no toast").message;
    assert!(message.contains("/tmp/dexo-diagnostics.zip"), "{message}");
}
