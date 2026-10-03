//! Server lists what runs, not every idle connection; it searches, sorts, cancels a query
//! as well as ending a session, and keeps Dexo from stopping its own.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::SessionInfo;
use dexo_tui::mouse::HitMap;
use dexo_tui::screens::admin::{AdminScreen, ServerTarget};
use dexo_tui::{Action, Effect, Model, update};

fn session(id: &str, user: &str, state: &str, ms: u64, query: &str) -> SessionInfo {
    SessionInfo {
        id: id.into(),
        user: Some(user.into()),
        database: Some("orders".into()),
        state: state.into(),
        duration_ms: Some(ms),
        current_query: Some(query.into()),
        application: Some("psql".into()),
        client: Some("10.0.0.7".into()),
    }
}

fn server() -> Model {
    let mut model = Model::default();
    model.screen = dexo_tui::model::Screen::Server;
    let mut ours = session(
        "50",
        "dexo",
        "active",
        100,
        "select pid from pg_stat_activity",
    );
    ours.application = Some("dexo".into());
    model.admin = AdminScreen {
        server: Some(ServerTarget {
            session: dexo_tui::runtime::SessionId(uuid::Uuid::nil()),
            generation: 1,
            connection: "pg-dev".into(),
            environment: "development".into(),
            read_only: false,
            user: Some("dexo".into()),
            database: Some("orders".into()),
        }),
        sessions: vec![
            session(
                "4121",
                "app",
                "active",
                12_400,
                "update orders set status = 'x'",
            ),
            session("4117", "app", "idle in transaction", 9_000, "begin"),
            session("4200", "report", "idle", 60_000, "select 1"),
            ours,
        ],
        ..AdminScreen::default()
    };
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

fn ids(model: &Model) -> Vec<String> {
    model
        .admin
        .visible()
        .iter()
        .map(|session| session.id.clone())
        .collect()
}

#[test]
fn idle_sessions_are_hidden_until_asked_for() {
    let mut model = server();
    assert_eq!(ids(&model), ["4121", "4117", "50"]);
    let frame = paint(&mut model);
    assert!(frame.contains("[a Idle: hidden]"), "{frame}");
    assert!(frame.contains("Sessions (3 of 4)"), "{frame}");
    press(&mut model, KeyCode::Char('a'));
    assert_eq!(ids(&model), ["4200", "4121", "4117", "50"]);
    press(&mut model, KeyCode::Esc);
    assert_eq!(ids(&model).len(), 3, "Esc hides them again");
}

#[test]
fn the_search_finds_by_user_database_and_query() {
    let mut model = server();
    press(&mut model, KeyCode::Char('/'));
    for ch in "update".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    assert_eq!(ids(&model), ["4121"]);
    model.admin.search.input.set_text("report");
    model.admin.show_idle = true;
    assert_eq!(ids(&model), ["4200"]);
}

#[test]
fn s_sorts_by_the_next_column_and_the_header_says_which() {
    let mut model = server();
    let frame = paint(&mut model);
    assert!(frame.contains("TIME ▼"), "{frame}");
    press(&mut model, KeyCode::Char('s'));
    assert_eq!(ids(&model), ["50", "4117", "4121"]);
    let frame = paint(&mut model);
    assert!(frame.contains("PID ▼"), "{frame}");
    assert!(!frame.contains("TIME ▼"), "{frame}");
}

#[test]
fn dexos_own_session_is_marked_and_not_stopped() {
    let mut model = server();
    let frame = paint(&mut model);
    assert!(frame.contains("active · you"), "{frame}");
    press(&mut model, KeyCode::End);
    assert_eq!(
        model.admin.picked().map(|session| session.id.as_str()),
        Some("50")
    );
    press(&mut model, KeyCode::Char('k'));
    assert!(model.admin.cancel.is_none());
    press(&mut model, KeyCode::Char('t'));
    assert!(model.admin.terminate.is_none());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.message.contains("Dexo's own session")),
    );
}

#[test]
fn k_cancels_the_picked_query_once_confirmed() {
    let mut model = server();
    let frame = paint(&mut model);
    for button in [
        "[k Cancel query]",
        "[t Terminate…]",
        "[y Copy query]",
        "[o Open in editor]",
    ] {
        assert!(frame.contains(button), "{button}: {frame}");
    }
    press(&mut model, KeyCode::Char('k'));
    let frame = paint(&mut model);
    assert!(
        frame.contains("Cancel the query session 4121 is running?"),
        "{frame}"
    );
    // Cancel holds the focus: Enter alone stops nothing.
    assert!(press(&mut model, KeyCode::Enter).is_empty());
    press(&mut model, KeyCode::Char('k'));
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdminCancel { target, .. } if target == "4121"
        )),
        "{effects:?}"
    );
}

#[test]
fn the_query_is_copied_or_opened_on_its_connection() {
    let mut model = server();
    let effects = press(&mut model, KeyCode::Char('y'));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::CopyToClipboard { text } if text == "update orders set status = 'x'"
    )));
    press(&mut model, KeyCode::Char('o'));
    assert_eq!(model.screen, dexo_tui::model::Screen::Workbench);
    assert_eq!(
        model.active_document().text(),
        "update orders set status = 'x'"
    );
}

#[test]
fn the_views_load_their_own_rows_and_draw_a_table() {
    use dexo_tui::screens::admin::{ServerView, ViewRows};
    let mut model = server();
    let effects = press(&mut model, KeyCode::Char('2'));
    assert_eq!(model.admin.view, ServerView::Locks);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadAdminView {
                view: ServerView::Locks,
                ..
            }
        )),
        "{effects:?}"
    );
    update(
        &mut model,
        Action::AdminViewLoaded {
            session: dexo_tui::runtime::SessionId(uuid::Uuid::nil()),
            view: ServerView::Locks,
            result: Ok((
                ViewRows::Locks(vec![dexo_driver_api::LockInfo {
                    lock_type: "relation".into(),
                    relation: Some("public.orders".into()),
                    mode: "RowExclusiveLock".into(),
                    granted: false,
                    session_id: "4121".into(),
                }]),
                None,
            )),
        },
    );
    let frame = paint(&mut model);
    for text in [
        "[2 Locks]",
        "RELATION",
        "public.orders",
        "RowExclusiveLock",
        "waiting",
    ] {
        assert!(frame.contains(text), "{text}: {frame}");
    }
}

#[test]
fn settings_are_searched_and_a_refusal_says_why() {
    use dexo_tui::screens::admin::{ServerView, ViewRows};
    let mut model = server();
    press(&mut model, KeyCode::Char('5'));
    let setting = |name: &str, value: &str| dexo_driver_api::VariableInfo {
        name: name.into(),
        value: Some(value.into()),
        scope: dexo_driver_api::VariableScope::Server,
    };
    update(
        &mut model,
        Action::AdminViewLoaded {
            session: dexo_tui::runtime::SessionId(uuid::Uuid::nil()),
            view: ServerView::Settings,
            result: Ok((
                ViewRows::Settings(vec![
                    setting("work_mem", "4MB"),
                    setting("shared_buffers", "128MB"),
                    setting("max_connections", "100"),
                ]),
                None,
            )),
        },
    );
    press(&mut model, KeyCode::Char('/'));
    for ch in "mem".chars() {
        press(&mut model, KeyCode::Char(ch));
    }
    let frame = paint(&mut model);
    assert!(frame.contains("work_mem"), "{frame}");
    assert!(!frame.contains("shared_buffers"), "{frame}");
    assert!(!frame.contains("max_connections"), "{frame}");

    press(&mut model, KeyCode::Enter);
    press(&mut model, KeyCode::Char('3'));
    update(
        &mut model,
        Action::AdminViewLoaded {
            session: dexo_tui::runtime::SessionId(uuid::Uuid::nil()),
            view: ServerView::Sizes,
            result: Ok((
                ViewRows::Sizes(Vec::new()),
                Some("permission denied for pg_class".into()),
            )),
        },
    );
    let frame = paint(&mut model);
    assert!(frame.contains("permission denied for pg_class"), "{frame}");
}

/// A view read from the server shown before is not drawn under the new one's name.
#[test]
fn another_servers_view_is_not_drawn() {
    use dexo_tui::screens::admin::{ServerView, ViewRows};
    let mut model = server();
    press(&mut model, KeyCode::Char('3'));
    update(
        &mut model,
        Action::AdminViewLoaded {
            session: dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(99)),
            view: ServerView::Sizes,
            result: Ok((ViewRows::Sizes(Vec::new()), Some("not this one".into()))),
        },
    );
    assert!(model.admin.rows.is_none());
    let frame = paint(&mut model);
    assert!(!frame.contains("not this one"), "{frame}");
}
