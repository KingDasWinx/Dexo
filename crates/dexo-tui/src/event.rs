use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_app::DriverRegistry;
use dexo_storage::AppPaths;
use futures_util::{FutureExt, StreamExt};
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

fn is_plain_escape(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Esc
            && key.modifiers.is_empty()
            && key.kind == KeyEventKind::Press
    )
}

/// The key a CSI sequence names, for the parameters between `[` and its final character:
/// `1;5` and `D` are Ctrl+Left, `15` and `~` is F5.
fn csi_key(params: &str, last: char) -> Option<KeyEvent> {
    let mut numbers = params.split(';').map(|part| part.parse::<u8>().ok());
    let first = numbers.next().flatten();
    // xterm's modifier parameter is 1 plus shift 1, alt 2 and ctrl 4.
    let mut modifiers = KeyModifiers::NONE;
    if let Some(code) = numbers.next().flatten().filter(|code| *code > 1) {
        let bits = code - 1;
        for (bit, modifier) in [
            (1, KeyModifiers::SHIFT),
            (2, KeyModifiers::ALT),
            (4, KeyModifiers::CONTROL),
        ] {
            if bits & bit != 0 {
                modifiers |= modifier;
            }
        }
    }
    let code = match (last, first) {
        ('A', _) => KeyCode::Up,
        ('B', _) => KeyCode::Down,
        ('C', _) => KeyCode::Right,
        ('D', _) => KeyCode::Left,
        ('H', _) | ('~', Some(1 | 7)) => KeyCode::Home,
        ('F', _) | ('~', Some(4 | 8)) => KeyCode::End,
        ('~', Some(2)) => KeyCode::Insert,
        ('~', Some(3)) => KeyCode::Delete,
        ('~', Some(5)) => KeyCode::PageUp,
        ('~', Some(6)) => KeyCode::PageDown,
        ('~', Some(15)) => KeyCode::F(5),
        ('~', Some(17)) => KeyCode::F(6),
        ('~', Some(18)) => KeyCode::F(7),
        ('~', Some(19)) => KeyCode::F(8),
        ('~', Some(20)) => KeyCode::F(9),
        ('~', Some(21)) => KeyCode::F(10),
        ('~', Some(23)) => KeyCode::F(11),
        ('~', Some(24)) => KeyCode::F(12),
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

/// An Esc and a key that reached the terminal in one write -- `ESC ESC [ 1 ~` -- are read
/// as one Esc and the characters `[1~`, which went into the document as text. When a
/// plain Esc is followed at once by the characters of a CSI sequence, they are the key
/// the sequence names: Esc, then that key. Anything else is left as it came.
pub fn unglue_escape(events: Vec<Event>) -> Vec<Event> {
    let plain = |event: &Event| match event {
        Event::Key(key)
            if key.kind == KeyEventKind::Press
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            match key.code {
                KeyCode::Char(ch) => Some(ch),
                _ => None,
            }
        }
        _ => None,
    };
    let mut out = Vec::with_capacity(events.len());
    let mut at = 0;
    while at < events.len() {
        out.push(events[at].clone());
        let esc = is_plain_escape(&events[at]);
        at += 1;
        if !esc || events.get(at).and_then(plain) != Some('[') {
            continue;
        }
        let mut end = at + 1;
        let mut params = String::new();
        while let Some(ch) = events.get(end).and_then(plain) {
            if ch.is_ascii_digit() || ch == ';' {
                params.push(ch);
                end += 1;
            } else {
                break;
            }
        }
        let key = events
            .get(end)
            .and_then(plain)
            .and_then(|last| csi_key(&params, last));
        if let Some(key) = key {
            out.push(Event::Key(key));
            at = end + 1;
        }
    }
    out
}

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
    let mut toast_tick = toast_clock(Duration::from_secs(1));
    let mut toast_ageing = false;
    // Runs only while a syntax error waits for the cursor to leave it: two ticks without
    // a key between them mean the typing has paused.
    let mut pause_tick = tokio::time::interval(Duration::from_millis(700));
    pause_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
        arm_toast_clock(&mut toast_tick, &mut toast_ageing, model.messages.expires());
        tokio::select! {
            terminal_event = events.next() => {
                let Some(event) = terminal_event else { break };
                let mut batch = vec![event?];
                if is_plain_escape(&batch[0]) {
                    // What came with the Esc in the same write, to tell a key glued to it
                    // from text.
                    while batch.len() < 16
                        && let Some(Some(next)) = events.next().now_or_never()
                    {
                        batch.push(next?);
                    }
                    batch = unglue_escape(batch);
                }
                for event in batch {
                    let Some(action) = action_from_event(event) else { continue };
                    let effects = crate::update::update(&mut model, action);
                    if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                        return Ok(());
                    }
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
            _ = pause_tick.tick(), if model.editor.hides_errors() => {
                let _ = crate::update::update(&mut model, Action::DiagnosticsTick);
            }
            _ = agent_tick.tick(), if model.mcp_audit.open || model.mcp_profiles.open => {
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

/// The clock toasts age by. A late tick is skipped, not made up for.
fn toast_clock(period: Duration) -> tokio::time::Interval {
    let mut clock = tokio::time::interval(period);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    clock
}

/// Starts the toast clock over when a toast that ages out goes up. It is polled only
/// while one is up, so an idle spell left it behind, and the next toast took the missed
/// seconds in a burst and was gone within a few frames.
fn arm_toast_clock(clock: &mut tokio::time::Interval, ageing: &mut bool, now_ageing: bool) {
    if now_ageing && !*ageing {
        clock.reset();
    }
    *ageing = now_ageing;
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use futures_util::FutureExt;
    use std::time::Duration;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(text: &str) -> Vec<Event> {
        text.chars().map(|ch| key(KeyCode::Char(ch))).collect()
    }

    /// `Escape Home` sent in one write left `[1~` in the document.
    #[test]
    fn a_key_glued_to_an_escape_is_that_key_not_text() {
        for (glued, wanted) in [
            ("[1~", KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            ("[D", KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            ("[A", KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            ("[15~", KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE)),
            ("[1;5D", KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)),
        ] {
            let mut events = vec![key(KeyCode::Esc)];
            events.extend(typed(glued));
            assert_eq!(
                super::unglue_escape(events),
                vec![key(KeyCode::Esc), Event::Key(wanted)],
                "{glued}"
            );
        }
    }

    #[test]
    fn text_after_an_escape_and_lone_escapes_are_left_alone() {
        let mut events = vec![key(KeyCode::Esc)];
        events.extend(typed("[x"));
        assert_eq!(super::unglue_escape(events.clone()), events);
        let lone = vec![key(KeyCode::Esc)];
        assert_eq!(super::unglue_escape(lone.clone()), lone);
        // The text before the Esc is not touched, nor what follows a whole sequence.
        let mut mixed = typed("ab");
        mixed.push(key(KeyCode::Esc));
        mixed.extend(typed("[Bz"));
        let mut wanted = typed("ab");
        wanted.push(key(KeyCode::Esc));
        wanted.push(key(KeyCode::Down));
        wanted.extend(typed("z"));
        assert_eq!(super::unglue_escape(mixed), wanted);
    }

    /// A toast that goes up after the clock sat idle waits a whole period for its first
    /// tick instead of taking the missed ones at once.
    #[tokio::test]
    async fn a_toast_after_an_idle_spell_gets_its_whole_time() {
        let mut clock = super::toast_clock(Duration::from_millis(200));
        let mut ageing = false;
        clock.tick().await;
        tokio::time::sleep(Duration::from_millis(700)).await;
        super::arm_toast_clock(&mut clock, &mut ageing, true);
        assert!(
            clock.tick().now_or_never().is_none(),
            "a missed tick aged it"
        );
        // Still up: the clock is not started over on every frame.
        super::arm_toast_clock(&mut clock, &mut ageing, true);
        clock.tick().await;
        assert!(ageing);
    }
}
