//! A driver that is a plugin not installed is offered where it is wanted: opening a
//! saved connection on it, or testing a new one. Each test makes up its own driver, as
//! the drivers Dexo knows are shared by the whole test process.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::DriverDescriptor;
use dexo_tui::mouse::HitMap;
use dexo_tui::{Action, Effect, Model, update};

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 36)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, 120, 36)
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model);
    effects
}

#[test]
fn opening_a_connection_whose_driver_is_not_installed_offers_its_plugin() {
    DriverDescriptor::announce(DriverDescriptor::plugin_driver(
        "warehouse",
        "Warehouse",
        true,
        "warehouse",
        false,
    ));
    let mut model = Model::default();
    model.connections.load_profiles(vec![ConnectionProfile::new(
        ConnectionId(uuid::Uuid::new_v4()),
        None,
        "lake",
        "warehouse",
        "local",
        serde_json::json!({"path": "/tmp/lake.wh"}),
        SecretRef::new("ref-lake".to_string()),
    )]);
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Connections),
    );
    paint(&mut model);
    let effects = press(&mut model, KeyCode::Char('s'));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::ConnectProfile { .. })),
        "{effects:?}"
    );
    let shown = paint(&mut model);
    assert!(
        shown.contains("Warehouse is a plugin that is not installed."),
        "{shown}"
    );
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::InstallPlugin { id, upgrade: false }] if id == "warehouse"),
        "{effects:?}"
    );
    // Installed: the runtime's registry says so, and the connection opens.
    DriverDescriptor::announce(DriverDescriptor::plugin_driver(
        "warehouse",
        "Warehouse",
        true,
        "warehouse",
        true,
    ));
    let effects = update(
        &mut model,
        Action::PluginWorkDone(Ok("Installed Warehouse 1.0.0.".into())),
    );
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::ConnectProfile { profile, .. } if profile.name == "lake")
        ),
        "{effects:?}"
    );
    DriverDescriptor::withdraw("warehouse");
}

#[test]
fn testing_a_new_connection_on_a_driver_not_installed_offers_it_and_keeps_the_form() {
    DriverDescriptor::announce(DriverDescriptor::plugin_driver(
        "lakehouse",
        "Lakehouse",
        true,
        "lakehouse",
        false,
    ));
    let mut model = Model::default();
    model.apply_size(120, 36);
    update(&mut model, Action::OpenConnectionForm);
    model.connection_form.set_value("driver", "lakehouse");
    let effects = update(&mut model, Action::TestConnection);
    assert!(effects.is_empty(), "{effects:?}");
    let offer = model.plugin_offer.clone().expect("an offer");
    assert_eq!(
        (offer.plugin.as_str(), offer.driver.as_str()),
        ("lakehouse", "Lakehouse")
    );
    press(&mut model, KeyCode::Esc);
    assert!(model.plugin_offer.is_none() && model.connection_form.open);
    DriverDescriptor::withdraw("lakehouse");
}

#[test]
fn the_palette_opens_plugins_where_asked() {
    let mut model = Model::default();
    let effects = update(
        &mut model,
        Action::ShowPlugins {
            view: dexo_tui::screens::plugins::PluginsView::Available,
            refresh: true,
        },
    );
    assert_eq!(model.screen, dexo_tui::model::Screen::Plugins);
    assert_eq!(
        model.plugins.view,
        dexo_tui::screens::plugins::PluginsView::Available
    );
    assert!(
        matches!(&effects[..], [Effect::LoadPlugins { refresh: true }]),
        "{effects:?}"
    );
}

#[test]
fn a_disabled_plugins_driver_offers_to_enable_it() {
    DriverDescriptor::announce(DriverDescriptor {
        disabled: true,
        ..DriverDescriptor::plugin_driver("pond", "Pond", true, "pond", false)
    });
    let mut model = Model::default();
    model.connections.load_profiles(vec![ConnectionProfile::new(
        ConnectionId(uuid::Uuid::new_v4()),
        None,
        "pond",
        "pond",
        "local",
        serde_json::json!({"path": "/tmp/pond.db"}),
        SecretRef::new("ref-pond".to_string()),
    )]);
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Connections),
    );
    paint(&mut model);
    press(&mut model, KeyCode::Char('s'));
    let shown = paint(&mut model);
    assert!(
        shown.contains("Pond is a plugin that is disabled."),
        "{shown}"
    );
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::SetPluginEnabled { id, enabled: true }] if id == "pond"),
        "{effects:?}"
    );
    assert!(model.plugins.then_connect.is_some());
    DriverDescriptor::withdraw("pond");
}

#[test]
fn an_offer_accepted_while_a_plugin_job_runs_waits() {
    DriverDescriptor::announce(DriverDescriptor::plugin_driver(
        "brook", "Brook", true, "brook", false,
    ));
    let mut model = Model::default();
    model.apply_size(120, 36);
    model.plugins.busy = Some(dexo_tui::screens::plugins::Busy {
        id: "duckdb".into(),
        verb: "Installing",
        done: 0,
        total: None,
    });
    update(&mut model, Action::OpenConnectionForm);
    model.connection_form.set_value("driver", "brook");
    update(&mut model, Action::TestConnection);
    assert!(model.plugin_offer.is_some());
    let effects = press(&mut model, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(model.plugins.busy.as_ref().unwrap().id, "duckdb");
    DriverDescriptor::withdraw("brook");
}
