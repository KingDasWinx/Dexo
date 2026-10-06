//! The theme in use goes with its plugin, and the person is told. In a file of its own: it
//! points DEXO_DATA_HOME at a scratch folder for its whole process.
use dexo_tui::model::Severity;
use dexo_tui::{Action, Model, update};

#[test]
fn disabling_the_pack_of_the_theme_in_use_says_so_last() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: the one test in this process, before anything reads the environment.
    unsafe { std::env::set_var("DEXO_DATA_HOME", dir.path()) };
    let mut model = Model::default();
    model.settings.theme = "plugin:extra-themes/everforest".into();
    update(
        &mut model,
        Action::PluginWorkDone(Ok("Disabled extra-themes.".into())),
    );
    assert_eq!(model.settings.theme, "dexo");
    let toast = model.messages.toast.clone().expect("a toast");
    assert_eq!(toast.severity, Severity::Warn, "{toast:?}");
    assert!(toast.message.contains("Dexo's own is used"), "{toast:?}");
}
