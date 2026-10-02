use crossterm::event::{Event, EventStream, KeyEventKind};
use dexo_app::DriverRegistry;
use dexo_storage::AppPaths;
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use crate::action::{Action, Effect};
use crate::model::Model;
use crate::runtime::WorkbenchRuntime;
use crate::runtime::storage_worker::StorageWorker;
use crate::terminal::{CrosstermTerminal, TerminalGuard, TuiError};

pub fn action_from_event(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Action::Key(key)),
        Event::Mouse(mouse) => Some(Action::Mouse(mouse)),
        Event::Resize(width, height) => Some(Action::Resize { width, height }),
        Event::Paste(text) => Some(Action::Paste(text)),
        _ => None,
    }
}

/// What the workbench opens on.
pub enum Startup {
    Workbench,
    /// `dexo <url>`: a connection that is listed and dialled but never saved.
    Temporary(Box<dexo_app::connection_url::UrlConnection>),
}

pub fn run(registry: DriverRegistry, startup: Startup) -> Result<(), TuiError> {
    crate::terminal::install_panic_hook();
    let runtime = tokio::runtime::Runtime::new()?;
    let ran = runtime.block_on(run_async(registry, startup));
    // Work still blocking a thread -- a pre-connect command opening its port, `docker`
    // listing containers -- is not waited for: its commands are stopped, and quitting
    // takes a second at most.
    dexo_app::process::stop_all();
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    ran
}

fn map_tui(error: impl std::fmt::Display) -> TuiError {
    std::io::Error::other(error.to_string()).into()
}

async fn run_async(registry: DriverRegistry, startup: Startup) -> Result<(), TuiError> {
    let paths = AppPaths::discover().map_err(map_tui)?;
    let first_run = !crate::entrance::is_complete(&paths.data_dir);
    let animate_entrance = crate::entrance::should_animate(&paths.data_dir);
    let worker = StorageWorker::start(paths.database).map_err(map_tui)?;
    let bootstrap = worker.bootstrap().await.map_err(map_tui)?;
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(32);
    crate::runtime::update_check::spawn(paths.data_dir.clone(), action_tx.clone());
    let mut runtime = WorkbenchRuntime::new(action_tx, worker, registry);
    let temporary = match startup {
        Startup::Workbench => None,
        Startup::Temporary(connection) => {
            if let Some(password) = &connection.password {
                runtime
                    .remember_secret(connection.profile.secret_ref.as_str(), password)
                    .map_err(map_tui)?;
            }
            Some((connection.profile, connection.warning))
        }
    };
    let mut guard = TerminalGuard::enter(CrosstermTerminal)?;
    // The workbench does not exist yet, so the theme is read from what was saved -- the
    // same mapping the workbench will apply, so the entrance cannot come up in another.
    // The background goes to the terminal first: the entrance resets to the terminal's
    // default after every cell, and only the default carries the theme's ground.
    let theme = crate::theme::saved_theme(&paths.data_dir);
    let capabilities = crate::capabilities::TerminalCapabilities::detect();
    guard.set_background_color(theme.background_rgb(capabilities))?;
    if animate_entrance {
        let _ = crate::entrance::play_animation(&theme);
        crate::entrance::clear_animation()?;
    }
    let animate_logo = first_run && animate_entrance && std::env::var_os("NO_COLOR").is_none();
    let logo_frames = Arc::new(crate::entrance::logo_frames(animate_logo, &theme));
    guard.enable_raw()?;
    let result = run_loop(
        bootstrap,
        temporary,
        first_run,
        logo_frames,
        &mut runtime,
        action_rx,
        &mut guard,
    )
    .await;
    runtime.dispatch(Effect::Shutdown).await;
    guard.restore();
    result
}

async fn run_loop(
    bootstrap: crate::runtime::storage_worker::BootstrapState,
    temporary: Option<(dexo_app::ConnectionProfile, Option<String>)>,
    show_onboarding: bool,
    logo_frames: Arc<Vec<crate::entrance::LogoFrame>>,
    runtime: &mut WorkbenchRuntime,
    mut action_rx: tokio::sync::mpsc::Receiver<Action>,
    guard: &mut TerminalGuard<CrosstermTerminal>,
) -> Result<(), TuiError> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut model = Model::default();
    let _ = crate::update::update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));
    // The Windows console reports Ctrl+H and Ctrl+Backspace apart, protocol or not.
    model.keys_disambiguated = guard.keyboard_enhanced() || cfg!(windows);
    model.onboarding.open = show_onboarding && temporary.is_none();
    model.onboarding.logo_frames = logo_frames;
    if let Some((profile, warning)) = temporary {
        let effects = crate::update::open_startup_connection(&mut model, profile, warning);
        if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
            return Ok(());
        }
    }
    guard.set_mouse(model.mouse)?;
    let mut events = EventStream::new();
    let mut onboarding_tick = tokio::time::interval(Duration::from_millis(66));
    onboarding_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Only runs while a toast that can age out is up, the same shape as onboarding_tick.
    // A sticky error toast never starts the clock.
    let mut toast_tick = tokio::time::interval(Duration::from_secs(1));
    let mut checkpoint = tokio::time::interval(Duration::from_secs(2));
    checkpoint.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Agent Activity is live: the requests and the calls are read again every second.
    let mut agent_tick = tokio::time::interval(Duration::from_secs(1));
    agent_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let mut hits = crate::mouse::HitMap::default();
        let mut drawn = ratatui::layout::Rect::default();
        terminal.draw(|frame| {
            drawn = frame.area();
            crate::render::render(frame, &model, &mut hits);
        })?;
        model.hits = hits;
        // crossterm only reports a resize when the size *changes*, so a terminal the user
        // never resizes left the model on `Model::default()`'s 160x50 for the whole
        // session -- every layout number computed for a screen that was not there.
        if (model.width, model.height) != (drawn.width, drawn.height) {
            let _ = crate::update::update(
                &mut model,
                Action::Resize {
                    width: drawn.width,
                    height: drawn.height,
                },
            );
            continue;
        }
        tokio::select! {
            terminal_event = events.next() => {
                let Some(event) = terminal_event else { break };
                let Some(action) = action_from_event(event?) else { continue };
                let effects = crate::update::update(&mut model, action);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
            runtime_action = action_rx.recv() => {
                let Some(action) = runtime_action else { break };
                let effects = crate::update::update(&mut model, action);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
            _ = onboarding_tick.tick(), if model.onboarding.open && model.onboarding.logo_frames.len() > 1 => {
                let _ = crate::update::update(&mut model, Action::OnboardingTick);
            }
            _ = toast_tick.tick(), if model.messages.expires() => {
                let _ = crate::update::update(&mut model, Action::ToastTick);
            }
            _ = agent_tick.tick(), if model.mcp_audit.open => {
                let effects = crate::update::update(&mut model, Action::AgentActivityTick);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
            _ = checkpoint.tick() => {
                let effects = crate::update::update(&mut model, Action::CheckpointTick);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
        }
        if let Some(request) = model.external_edit.take() {
            // The child reads the keyboard while it runs: the stream has to stop reading
            // it first, or the two take turns at the keys.
            drop(events);
            let text = edit_externally(guard, &request.text).await;
            events = EventStream::new();
            // The alternate screen comes back empty, and ratatui still thinks the last
            // frame is on it: a blank frame makes the next one draw every cell. Not
            // `Terminal::clear`, which asks the terminal where the cursor is and fails
            // the whole session when no answer comes.
            terminal.draw(|frame| frame.render_widget(ratatui::widgets::Clear, frame.area()))?;
            let effects = crate::update::update(
                &mut model,
                Action::ExternalEditFinished {
                    document: request.document,
                    text,
                },
            );
            if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                return Ok(());
            }
        }
        guard.set_mouse(model.mouse)?;
        // The theme can change under the user (mode, accent), so this is offered every
        // frame and the guard only forwards a change.
        guard.set_cursor_color(model.theme.caret_rgb(model.capabilities))?;
        // Vim mode: a block outside Insert; everyone else keeps the terminal's caret.
        guard.set_cursor_shape(
            crate::screens::vim::active(&model).then(|| crate::screens::vim::block_cursor(&model)),
        )?;
        guard.set_background_color(model.theme.background_rgb(model.capabilities))?;
    }
    Ok(())
}

/// Writes `text` to a temp file, runs `$VISUAL`, `$EDITOR` or the platform's editor on
/// it with the terminal handed over, and reads back what was saved. A non-zero exit, or
/// a file that cannot be read back, leaves the document as it was.
async fn edit_externally(
    guard: &mut TerminalGuard<CrosstermTerminal>,
    text: &str,
) -> Result<String, String> {
    let file = tempfile::Builder::new()
        .prefix("dexo-")
        .suffix(".sql")
        .tempfile()
        .map_err(|error| format!("could not create a file for the editor: {error}"))?;
    std::fs::write(file.path(), text)
        .map_err(|error| format!("could not write a file for the editor: {error}"))?;
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| if cfg!(windows) { "notepad" } else { "vi" }.to_string());
    let suspended = guard.suspend();
    // Out of raw mode, Ctrl+C at the terminal is SIGINT for Dexo too, and its default
    // would end every open session while `code --wait` sits there.
    #[cfg(unix)]
    let _interrupt = dexo_app::process::InterruptShield::new();
    // ponytail: Windows keeps Tokio's Ctrl+C handler for the session; it has no `kill -INT`
    // for that to swallow. SetConsoleCtrlHandler with a removable handler if it matters.
    #[cfg(windows)]
    let _interrupt = tokio::signal::windows::ctrl_c().ok();
    // Through the shell, so `code --wait` and other editors with arguments work; the
    // path goes as an argument, never spliced into the command.
    #[cfg(windows)]
    let status = tokio::process::Command::new("cmd")
        .arg("/C")
        // Verbatim: `arg` would escape the quotes as `\"`, which cmd does not read.
        .raw_arg(format!("{editor} \"{}\"", file.path().display()))
        .status()
        .await;
    #[cfg(not(windows))]
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(file.path())
        .status()
        .await;
    guard
        .resume(suspended)
        .map_err(|error| format!("could not take the terminal back: {error}"))?;
    match status {
        Ok(status) if status.success() => std::fs::read_to_string(file.path())
            .map_err(|error| format!("could not read what the editor saved: {error}")),
        Ok(status) => Err(format!(
            "{editor} exited with {status}; the document is unchanged"
        )),
        Err(error) => Err(format!("could not start {editor}: {error}")),
    }
}

async fn dispatch_effects(
    runtime: &mut WorkbenchRuntime,
    action_rx: &mut tokio::sync::mpsc::Receiver<Action>,
    model: &mut Model,
    mut effects: Vec<Effect>,
) -> bool {
    let mut pending: VecDeque<Effect> = effects.drain(..).collect();
    while let Some(effect) = pending.pop_front() {
        if matches!(effect, Effect::Quit | Effect::Shutdown) {
            runtime.dispatch(Effect::Shutdown).await;
            return true;
        }
        runtime.dispatch(effect).await;
        while let Ok(action) = action_rx.try_recv() {
            pending.extend(crate::update::update(model, action));
        }
    }
    false
}
