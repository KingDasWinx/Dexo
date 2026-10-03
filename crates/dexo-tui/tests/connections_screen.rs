//! Connections is a list to find a connection in: a search, filters by state and by
//! environment, and groups that fold. It was every connection in one run, the pick
//! walked to with the arrows.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::mouse::{HitMap, HitTarget};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::{Action, Effect, Model, update};

fn profile(name: &str, driver: &str, environment: &str, group: Option<&str>) -> ConnectionProfile {
    let mut profile = ConnectionProfile::new(
        ConnectionId(uuid::Uuid::new_v4()),
        None,
        name,
        driver,
        environment,
        serde_json::json!({"host":"127.0.0.1","port":5432,"username":"dexo","database":"qa0"}),
        SecretRef::new(format!("ref-{name}")),
    );
    profile.group_path = group.map(str::to_string);
    profile
}

/// my-dev, pg-dev (connected), pg-prod, and orders and shop in the group `team`.
fn four_connections() -> Model {
    let mut model = Model::default();
    model.connections.load_profiles(vec![
        profile("pg-dev", "postgres", "development", None),
        profile("pg-prod", "postgres", "production", None),
        profile("my-dev", "mysql", "development", None),
        profile("shop", "sqlite", "local", Some("team")),
        profile("orders", "postgres", "staging", Some("team")),
    ]);
    model.connections.upsert_session(SessionRow {
        id: SessionId(uuid::Uuid::from_u128(1)),
        connection: "pg-dev".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "development".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    update(
        &mut model,
        Action::GoToScreen(dexo_tui::model::Screen::Connections),
    );
    paint(&mut model);
    model
}

fn paint(model: &mut Model) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    model.hits = hits;
    dexo_tui::render::render_to_string(model, 140, 40)
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    let effects = update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    paint(model);
    effects
}

fn click(model: &mut Model, target: HitTarget) -> Vec<Effect> {
    let (column, row) = model.hits.center(target);
    assert_ne!((column, row), (0, 0), "{target:?} is not on screen");
    let effects = update(
        model,
        Action::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
    paint(model);
    effects
}

fn picked(model: &Model) -> Option<String> {
    model
        .connections
        .picked()
        .map(|profile| profile.name.clone())
}

fn shown(model: &Model) -> Vec<String> {
    model
        .connections
        .items()
        .into_iter()
        .filter_map(|item| match item {
            dexo_tui::screens::connections::Item::Row(index) => model
                .connections
                .profiles
                .get(index)
                .map(|row| row.profile.name.clone()),
            dexo_tui::screens::connections::Item::Group { .. } => None,
        })
        .collect()
}

#[test]
fn search_narrows_the_list_and_what_is_typed_is_text() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('/'));
    // `x` deletes and `d` duplicates outside the search.
    for ch in "xd".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert!(model.connections.delete_target.is_none());
    for _ in 0..2 {
        press(&mut model, KeyCode::Backspace);
    }
    for ch in "prod".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert_eq!(shown(&model), ["pg-prod"]);
    assert_eq!(picked(&model).as_deref(), Some("pg-prod"));
    let frame = paint(&mut model);
    assert!(frame.contains("/ prod"), "{frame}");
    assert!(!frame.contains("my-dev"), "{frame}");
}

#[test]
fn search_matches_host_database_group_and_driver_in_smart_case() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('/'));
    for ch in "mysql".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert_eq!(shown(&model), ["my-dev"]);
    model.connections.search.input.set_text("team");
    assert_eq!(shown(&model), ["orders", "shop"]);
    model.connections.search.input.set_text("Team");
    assert!(shown(&model).is_empty(), "a capital matches case");
}

#[test]
fn esc_ends_the_search_then_clears_it_then_the_filters_then_goes_back() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('o'));
    press(&mut model, KeyCode::Char('/'));
    press(&mut model, KeyCode::Char('p'));
    press(&mut model, KeyCode::Enter);
    assert!(!model.connections.search.typing);
    assert_eq!(shown(&model), ["pg-dev"]);
    press(&mut model, KeyCode::Esc);
    assert!(model.connections.search.input.is_empty());
    assert!(
        model.connections.connected_only,
        "the filters go after the search"
    );
    press(&mut model, KeyCode::Esc);
    assert!(!model.connections.connected_only);
    assert_eq!(model.screen, dexo_tui::model::Screen::Connections);
    press(&mut model, KeyCode::Esc);
    assert_ne!(model.screen, dexo_tui::model::Screen::Connections);
}

#[test]
fn connected_only_and_nothing_matches_offers_to_clear() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(shown(&model), ["pg-dev"]);
    let frame = paint(&mut model);
    assert!(frame.contains("[o Connected only]"), "{frame}");
    model.connections.search.input.set_text("zzz");
    let frame = paint(&mut model);
    assert!(frame.contains("Nothing matches the filters."), "{frame}");
    assert!(frame.contains("[Esc Clear filters]"), "{frame}");
    assert!(model.connections.picked().is_none());
    // The button clears the search and the filters at once.
    click(&mut model, HitTarget::Press(KeyCode::Esc, false));
    assert!(model.connections.search.input.is_empty());
    assert!(!model.connections.connected_only);
    assert_eq!(model.screen, dexo_tui::model::Screen::Connections);
}

#[test]
fn v_cycles_the_environment_and_its_chip_says_which() {
    let mut model = four_connections();
    let mut labels = Vec::new();
    for _ in 0..5 {
        let frame = paint(&mut model);
        let label = [
            "Env: all",
            "Env: prod",
            "Env: staging",
            "Env: dev",
            "Env: local",
        ]
        .into_iter()
        .find(|label| frame.contains(label))
        .unwrap_or("none");
        labels.push((label, shown(&model)));
        press(&mut model, KeyCode::Char('v'));
    }
    assert_eq!(labels[0].0, "Env: all");
    assert_eq!(labels[1], ("Env: prod", vec!["pg-prod".to_string()]));
    assert_eq!(labels[2], ("Env: staging", vec!["orders".to_string()]));
    assert_eq!(
        labels[3],
        ("Env: dev", vec!["my-dev".to_string(), "pg-dev".to_string()])
    );
    assert_eq!(labels[4], ("Env: local", vec!["shop".to_string()]));
    let frame = paint(&mut model);
    assert!(frame.contains("Env: all"), "{frame}");
}

#[test]
fn a_group_folds_with_left_and_unfolds_with_right_or_a_click() {
    let mut model = four_connections();
    let frame = paint(&mut model);
    assert!(frame.contains("▾ team (2)"), "{frame}");
    // Ungrouped first, then the group's.
    assert_eq!(
        shown(&model),
        ["my-dev", "pg-dev", "pg-prod", "orders", "shop"]
    );
    press(&mut model, KeyCode::End);
    assert_eq!(picked(&model).as_deref(), Some("shop"));
    // Left on a row of a group folds it, and the pick goes to its heading.
    press(&mut model, KeyCode::Left);
    assert_eq!(shown(&model), ["my-dev", "pg-dev", "pg-prod"]);
    assert!(model.connections.picked().is_none());
    let frame = paint(&mut model);
    assert!(frame.contains("▸ team (2)"), "{frame}");
    press(&mut model, KeyCode::Right);
    assert_eq!(shown(&model).len(), 5);
    click(&mut model, HitTarget::ListGroup(0));
    assert_eq!(shown(&model).len(), 3, "a click on the heading folds it");
    // Up from the heading reaches the rows above it.
    press(&mut model, KeyCode::Up);
    assert_eq!(picked(&model).as_deref(), Some("pg-prod"));
}

#[test]
fn a_search_shows_the_rows_of_a_folded_group() {
    let mut model = four_connections();
    model.connections.folded.insert("team".into());
    assert_eq!(shown(&model).len(), 3);
    model.connections.search.input.set_text("shop");
    assert_eq!(shown(&model), ["shop"]);
}

#[test]
fn the_list_is_a_table_with_status_driver_env_and_address() {
    let mut model = four_connections();
    let frame = paint(&mut model);
    assert!(frame.contains("NAME"), "{frame}");
    assert!(frame.contains("ADDRESS"), "{frame}");
    assert!(frame.contains("○ pg-prod"), "{frame}");
    assert!(frame.contains("● pg-dev"), "{frame}");
    assert!(frame.contains("PostgreSQL"), "{frame}");
    assert!(frame.contains("prod "), "{frame}");
    assert!(frame.contains("127.0.0.1:5432/qa0"), "{frame}");
    assert!(
        !frame.contains("offline"),
        "a word on every row marks nothing: {frame}"
    );
}

#[test]
fn the_detail_is_fields_not_prose() {
    let mut model = four_connections();
    let frame = paint(&mut model);
    for label in ["Driver", "Address", "Database", "User", "Password"] {
        assert!(frame.contains(&format!(" {label} ")), "{label}: {frame}");
    }
    assert!(!frame.contains("database qa0 · user dexo"), "{frame}");
    // A connected one has its session's section.
    while picked(&model).as_deref() != Some("pg-dev") {
        press(&mut model, KeyCode::Down);
    }
    let frame = paint(&mut model);
    assert!(frame.contains("── Session"), "{frame}");
    assert!(frame.contains("pg-dev ● connected"), "{frame}");
}

#[test]
fn narrow_the_address_goes_first_then_the_driver() {
    let mut model = four_connections();
    let frame = |model: &mut Model, width: u16| {
        model.apply_size(width, 30);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 30)).unwrap();
        let mut hits = HitMap::default();
        terminal
            .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
            .unwrap();
        dexo_tui::render::render_to_string(model, width, 30)
    };
    let narrow = frame(&mut model, 100);
    assert!(!narrow.contains("ADDRESS"), "{narrow}");
    assert!(narrow.contains("DRIVER"), "{narrow}");
    let narrower = frame(&mut model, 60);
    assert!(narrower.contains("NAME"), "{narrower}");
    assert!(narrower.contains("○ pg-prod"), "{narrower}");
}

fn form_value(model: &Model, label: &str) -> String {
    model
        .connection_form
        .fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.as_str().to_string())
        .unwrap_or_default()
}

#[test]
fn a_url_fills_the_form() {
    let mut model = four_connections();
    let frame = paint(&mut model);
    assert!(frame.contains("[u From URL]"), "{frame}");
    press(&mut model, KeyCode::Char('u'));
    assert!(model.connection_form.open);
    for ch in "postgres://ana:s3cret@db.local:5433/shop".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    press(&mut model, KeyCode::Enter);
    assert_eq!(form_value(&model, "driver"), "postgres");
    assert_eq!(form_value(&model, "host"), "db.local");
    assert_eq!(form_value(&model, "port"), "5433");
    assert_eq!(form_value(&model, "database"), "shop");
    assert_eq!(form_value(&model, "username"), "ana");
    assert_eq!(form_value(&model, "password"), "s3cret");
    assert_eq!(form_value(&model, "name"), "ana@db.local/shop");
    // The URL, password and all, is not kept once read.
    assert_eq!(form_value(&model, "url"), "");
    assert!(model.connection_form.errors.is_empty());
}

#[test]
fn a_pasted_url_fills_the_form_at_once() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    update(
        &mut model,
        Action::Paste("mysql://root:pw@127.0.0.1:3307/app".into()),
    );
    assert_eq!(form_value(&model, "driver"), "mysql");
    assert_eq!(form_value(&model, "port"), "3307");
    assert_eq!(form_value(&model, "url"), "");
    let frame = paint(&mut model);
    assert!(!frame.contains("root:pw"), "{frame}");
}

#[test]
fn a_bad_url_stays_with_its_reason() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    for ch in "nonsense".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    press(&mut model, KeyCode::Enter);
    assert!(model.connection_form.open);
    assert_eq!(
        form_value(&model, "url"),
        "nonsense",
        "the focus stays on it"
    );
    let frame = paint(&mut model);
    assert!(frame.contains("not a connection URL"), "{frame}");
}

#[test]
fn new_starts_on_the_name_with_the_url_above() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('n'));
    let focused = &model.connection_form.fields[model.connection_form.focus];
    assert_eq!(focused.label, "name");
    press(&mut model, KeyCode::Up);
    let focused = &model.connection_form.fields[model.connection_form.focus];
    assert_eq!(focused.label, "url");
}

fn pick(model: &mut Model, name: &str) {
    model.connections.pick_end(false);
    while picked(model).as_deref() != Some(name) {
        press(model, KeyCode::Down);
    }
}

fn profile_id(model: &Model, name: &str) -> String {
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| row.profile.id.0.to_string())
        .unwrap()
}

#[test]
fn new_sql_opens_a_document_on_the_connection_dialling_it_if_need_be() {
    let mut model = four_connections();
    pick(&mut model, "pg-prod");
    let effects = press(&mut model, KeyCode::Char('s'));
    assert_eq!(model.screen, dexo_tui::model::Screen::Workbench);
    assert_eq!(
        model.active_document().connection_id,
        Some(profile_id(&model, "pg-prod"))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ConnectProfile { profile, .. } if profile.name == "pg-prod")),
        "{effects:?}"
    );
}

#[test]
fn copy_url_leaves_the_password_out() {
    let mut model = four_connections();
    pick(&mut model, "pg-dev");
    let effects = press(&mut model, KeyCode::Char('y'));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::CopyToClipboard { text } if text == "postgres://dexo@127.0.0.1:5432/qa0"
        )),
        "{effects:?}"
    );
}

#[test]
fn browse_goes_to_the_explorer_on_the_connection() {
    let mut model = four_connections();
    pick(&mut model, "pg-prod");
    press(&mut model, KeyCode::Char('b'));
    assert_eq!(model.screen, dexo_tui::model::Screen::Workbench);
    assert_eq!(model.explorer.selected_connection_name(), Some("pg-prod"));
}

#[test]
fn a_test_shows_in_the_detail_as_it_runs_and_when_it_ends() {
    let mut model = four_connections();
    pick(&mut model, "pg-prod");
    let effects = press(&mut model, KeyCode::Char('t'));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::TestSavedProfile { .. }))
    );
    let frame = paint(&mut model);
    assert!(frame.contains("── Last test"), "{frame}");
    assert!(frame.contains("testing"), "{frame}");
    update(
        &mut model,
        Action::ConnectionTested {
            name: "pg-prod".into(),
            ok: false,
            message: "connection refused".into(),
        },
    );
    let frame = paint(&mut model);
    assert!(frame.contains("✗ connection refused"), "{frame}");
    // Another connection picked, the line goes.
    press(&mut model, KeyCode::Up);
    let frame = paint(&mut model);
    assert!(!frame.contains("── Last test"), "{frame}");
}

#[test]
fn the_buttons_follow_the_connections_state() {
    let mut model = four_connections();
    pick(&mut model, "pg-prod");
    let frame = paint(&mut model);
    for button in ["[⏎ Connect]", "[s New SQL]", "[b Browse]", "[y Copy URL]"] {
        assert!(frame.contains(button), "{button}: {frame}");
    }
    pick(&mut model, "pg-dev");
    let frame = paint(&mut model);
    assert!(frame.contains("[⏎ Use]"), "{frame}");
    model.active_session = Some(SessionId(uuid::Uuid::from_u128(1)));
    let frame = paint(&mut model);
    assert!(frame.contains("[⏎ Open SQL]"), "{frame}");
    assert!(!frame.contains("[s New SQL]"), "{frame}");
}

/// Disconnected with "Connected only" on, the picked row leaves the list and the pick
/// goes to one still on it: the detail went blank, its buttons gone.
#[test]
fn the_pick_stays_on_the_list_when_its_row_leaves_it() {
    let mut model = four_connections();
    model.connections.upsert_session(SessionRow {
        id: SessionId(uuid::Uuid::from_u128(2)),
        connection: "my-dev".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: "development".into(),
        read_only: false,
        driver: "mysql".into(),
    });
    press(&mut model, KeyCode::Char('o'));
    while picked(&model).as_deref() != Some("pg-dev") {
        press(&mut model, KeyCode::Down);
    }
    model
        .connections
        .remove_session(SessionId(uuid::Uuid::from_u128(1)));
    assert_eq!(picked(&model).as_deref(), Some("my-dev"));
}

/// The last connection deleted, the pick goes to the one before it, not into Docker's.
#[test]
fn deleting_the_last_connection_picks_the_one_before() {
    let mut model = four_connections();
    press(&mut model, KeyCode::End);
    let last = model.connections.selected_profile;
    let name = model.connections.profiles[last].profile.name.clone();
    update(&mut model, Action::ProfileDeleted { name });
    assert!(model.connections.picked().is_some());
    assert_eq!(model.connections.selected_profile, last - 1);
}

/// A URL typed by hand shows no password, and is read however the field is left.
#[test]
fn a_typed_urls_password_never_shows_and_up_reads_it() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    for ch in "postgres://ana:s3cret".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let frame = paint(&mut model);
    assert!(!frame.contains("s3cret"), "{frame}");
    for ch in "@db.local/shop".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let frame = paint(&mut model);
    assert!(!frame.contains("s3cret"), "{frame}");
    press(&mut model, KeyCode::Up);
    assert_eq!(form_value(&model, "host"), "db.local");
    assert_eq!(form_value(&model, "url"), "");
}

/// Submitted with a URL still in its field, the URL is read first.
#[test]
fn submit_reads_a_url_left_in_its_field() {
    let mut model = four_connections();
    press(&mut model, KeyCode::Char('u'));
    for ch in "nonsense".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let effects = update(&mut model, Action::SaveConnection);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(
        model
            .connection_form
            .errors
            .iter()
            .any(|error| error.contains("not a connection URL")),
        "{:?}",
        model.connection_form.errors
    );
}
