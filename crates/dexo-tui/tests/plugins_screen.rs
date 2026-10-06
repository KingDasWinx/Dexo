//! The Plugins screen: what is installed and what the index offers, the pick in full
//! with what can be done to it, the index's state said over the list.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::plugin::manifest::Kind;
use dexo_app::plugin::state::{Installed, Origin};
use dexo_tui::model::Screen;
use dexo_tui::mouse::HitMap;
use dexo_tui::screens::plugins::{Busy, FromFile, PluginRow, PluginsSnapshot, StagedInfo};
use dexo_tui::{Action, Effect, Model, update};

fn paint(model: &mut Model, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, width, height)
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model, 120, 30);
    effects
}

fn installed(version: &str, enabled: bool, origin: Origin) -> Installed {
    Installed {
        version: version.into(),
        kind: Kind::Driver,
        enabled,
        origin,
        sha256: None,
        files: Default::default(),
        installed_at: 0,
    }
}

fn row(
    id: &str,
    name: &str,
    kind: Kind,
    installed: Option<Installed>,
    newest: Option<&str>,
) -> PluginRow {
    PluginRow {
        id: id.into(),
        name: name.into(),
        kind,
        description: format!("The {name} plugin"),
        authors: vec!["KingDasWinx".into()],
        license: "MIT".into(),
        installed: installed.map(|mut entry| {
            entry.kind = kind;
            entry
        }),
        newest: newest.map(Into::into),
        min_dexo: Some("1.5.0".into()),
        protocol: (kind == Kind::Driver).then_some(1),
        size: Some(27_839_411),
        provides: vec![id.into()],
        readme: vec![],
    }
}

fn rows() -> Vec<PluginRow> {
    vec![
        row(
            "duckdb",
            "DuckDB",
            Kind::Driver,
            Some(installed("1.0.0", true, Origin::Official)),
            Some("1.1.0"),
        ),
        row(
            "mine",
            "Mine",
            Kind::Theme,
            Some(installed("0.1.0", false, Origin::Local)),
            None,
        ),
        row(
            "extra-themes",
            "Extra themes",
            Kind::Theme,
            None,
            Some("1.0.0"),
        ),
    ]
}

fn screen() -> Model {
    let mut model = Model::default();
    model.capabilities.unicode = true;
    let effects = update(&mut model, Action::GoToScreen(Screen::Plugins));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadPlugins { refresh: false })),
        "{effects:?}"
    );
    update(
        &mut model,
        Action::PluginsLoaded(PluginsSnapshot {
            rows: rows(),
            index_error: None,
            offline: None,
        }),
    );
    paint(&mut model, 120, 30);
    model
}

#[test]
fn installed_plugins_list_with_their_state_and_actions() {
    let mut model = screen();
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("[Plugins 1]"),
        "the update count beside its name:\n{shown}"
    );
    assert!(
        shown.contains("↑ DuckDB") && shown.contains("○ Mine"),
        "{shown}"
    );
    assert!(shown.contains("local"), "{shown}");
    assert!(
        !shown.contains("Extra themes"),
        "available plugins wait in their view:\n{shown}"
    );
    for button in [
        "[u Update to 1.1.0]",
        "[e Disable]",
        "[x Remove…]",
        "[r Refresh]",
        "[f Install from file…]",
    ] {
        assert!(shown.contains(button), "{button}:\n{shown}");
    }
    assert!(
        shown.contains("official, signed") && shown.contains("26.5 MB"),
        "{shown}"
    );
    insta::assert_snapshot!(paint(&mut model, 80, 24));
}

#[test]
fn the_available_view_and_the_type_filter() {
    let mut model = screen();
    press(&mut model, KeyCode::Tab);
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("+ Extra themes") && shown.contains("[i Install]"),
        "{shown}"
    );
    insta::assert_snapshot!(paint(&mut model, 80, 24));
    press(&mut model, KeyCode::Char('t'));
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("Type: drivers") && shown.contains("No plugin matches"),
        "{shown}"
    );
    press(&mut model, KeyCode::Esc);
    assert!(paint(&mut model, 120, 30).contains("+ Extra themes"));
}

#[test]
fn under_80_columns_the_list_comes_alone() {
    let mut model = screen();
    let shown = paint(&mut model, 70, 24);
    assert!(
        shown.contains("DuckDB") && !shown.contains("[u Update"),
        "{shown}"
    );
    insta::assert_snapshot!(shown);
}

#[test]
fn the_index_state_is_said_over_the_list() {
    let mut model = screen();
    update(
        &mut model,
        Action::PluginsLoaded(PluginsSnapshot {
            rows: rows(),
            index_error: Some("the index signature does not match: nothing was installed".into()),
            offline: None,
        }),
    );
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("the index signature does not match: nothing was installed"),
        "{shown}"
    );
    insta::assert_snapshot!("signature_mismatch", shown);
    update(
        &mut model,
        Action::PluginsLoaded(PluginsSnapshot {
            rows: rows(),
            index_error: None,
            offline: Some("offline: showing the index from 2026-10-05".into()),
        }),
    );
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("offline: showing the index from 2026-10-05"),
        "{shown}"
    );
    insta::assert_snapshot!("offline", shown);
}

#[test]
fn a_download_shows_a_bar_with_bytes_and_percent() {
    let mut model = screen();
    model.plugins.busy = Some(Busy {
        id: "duckdb".into(),
        verb: "Updating",
        done: 13_919_706,
        total: Some(27_839_411),
    });
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("50%") && shown.contains("13.2 MB of 26.5 MB"),
        "{shown}"
    );
    assert!(shown.contains("Esc cancels"), "{shown}");
    insta::assert_snapshot!(shown);
}

#[test]
fn the_trust_prompt_asks_for_the_plugin_id() {
    let mut model = screen();
    model.plugins.from_file = Some(FromFile {
        staged: Some(StagedInfo {
            token: 1,
            id: "lake".into(),
            trust: "Lake 1.0.0 (driver, id lake) by someone is not from Dexo's signed index: nothing vouches for it. It brings the driver lake: this program will receive the passwords of the connections that use it.".into(),
        }),
        ..Default::default()
    });
    let shown = paint(&mut model, 120, 30);
    assert!(
        shown.contains("receive the passwords of the connections that use it"),
        "{shown}"
    );
    assert!(shown.contains("Type lake to install it"), "{shown}");
    insta::assert_snapshot!(shown);
}

#[test]
fn the_snapshot_lists_installed_plugins_without_the_index() {
    use dexo_app::plugin::{index::IndexSource, install};
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pack = dexo_test_support::plugins::theme_folder(work.path(), "pack", "1.0.0");
    install::commit(
        data.path(),
        install::stage(data.path(), &pack).unwrap(),
        Origin::Local,
        None,
    )
    .unwrap();
    let offline = IndexSource {
        url: dexo_test_support::plugins::HttpServer::dead_url(),
        keys: vec![],
    };
    let snapshot = dexo_tui::runtime::plugin_manager::snapshot(data.path(), &offline, false);
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].provides, ["ocean"]);
    assert!(snapshot.index_error.is_some());
}

#[test]
fn update_disable_and_remove_send_their_effects() {
    let mut model = screen();
    let effects = press(&mut model, KeyCode::Char('u'));
    assert!(
        matches!(&effects[..], [Effect::InstallPlugin { id, upgrade: true }] if id == "duckdb"),
        "{effects:?}"
    );
    assert!(model.plugins.busy.is_some());
    // While it downloads the other actions wait, and Esc cancels it.
    press(&mut model, KeyCode::Char('x'));
    assert!(model.plugins.removing.is_none());
    assert!(matches!(
        &press(&mut model, KeyCode::Esc)[..],
        [Effect::CancelPluginWork]
    ));
    update(
        &mut model,
        Action::PluginProgress {
            done: 50,
            total: Some(100),
        },
    );
    assert_eq!(model.plugins.busy.as_ref().unwrap().done, 50);
    update(
        &mut model,
        Action::PluginWorkDone(Ok("Updated DuckDB to 1.1.0.".into())),
    );
    assert!(model.plugins.busy.is_none());
    let effects = press(&mut model, KeyCode::Char('e'));
    assert!(
        matches!(&effects[..], [Effect::SetPluginEnabled { id, enabled: false }] if id == "duckdb"),
        "{effects:?}"
    );
    press(&mut model, KeyCode::Char('x'));
    assert_eq!(model.plugins.removing.as_deref(), Some("duckdb"));
    assert!(paint(&mut model, 120, 30).contains("Remove DuckDB and its files?"));
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::RemovePlugin { id }] if id == "duckdb"),
        "{effects:?}"
    );
}

#[test]
fn install_from_a_file_asks_for_the_id_first() {
    let mut model = screen();
    press(&mut model, KeyCode::Char('f'));
    for ch in "/tmp/lake.tar.gz".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::StagePluginFile { path }] if path == "/tmp/lake.tar.gz"),
        "{effects:?}"
    );
    update(
        &mut model,
        Action::PluginFileStaged(Ok(StagedInfo {
            token: 7,
            id: "lake".into(),
            trust: "Lake is not from Dexo's signed index.".into(),
        })),
    );
    for ch in "lak".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert!(
        press(&mut model, KeyCode::Enter).is_empty(),
        "the id is not typed yet"
    );
    assert!(paint(&mut model, 120, 30).contains("Type lake to install it."));
    press(&mut model, KeyCode::Char('e'));
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::CommitPluginFile { token: 7 }]),
        "{effects:?}"
    );
    assert!(model.plugins.from_file.is_none() && model.plugins.busy.is_some());
}

#[test]
fn esc_on_the_trust_prompt_drops_the_package() {
    let mut model = screen();
    press(&mut model, KeyCode::Char('f'));
    update(
        &mut model,
        Action::PluginFileStaged(Ok(StagedInfo {
            token: 3,
            id: "lake".into(),
            trust: "Lake is not from Dexo's signed index.".into(),
        })),
    );
    let effects = press(&mut model, KeyCode::Esc);
    assert!(
        matches!(&effects[..], [Effect::DropPluginFile { token: 3 }]),
        "{effects:?}"
    );
    assert!(model.plugins.from_file.is_none());
}

#[test]
fn a_small_download_is_said_in_kilobytes() {
    let mut model = screen();
    model.plugins.snapshot.rows[0].size = Some(5_900);
    let shown = paint(&mut model, 120, 30);
    assert!(shown.contains("download   5 KB"), "{shown}");
}

#[test]
fn the_removal_question_wraps_in_a_narrow_pane() {
    let mut model = screen();
    press(&mut model, KeyCode::Char('x'));
    let shown = paint(&mut model, 70, 24);
    assert!(
        shown.contains("say it is not ") && shown.contains("│installed."),
        "{shown}"
    );
}
