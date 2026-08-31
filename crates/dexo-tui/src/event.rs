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
        _ => None,
    }
}

pub fn run(registry: DriverRegistry) -> Result<(), TuiError> {
    crate::terminal::install_panic_hook();
    tokio::runtime::Runtime::new()?.block_on(run_async(registry))
}

fn map_tui(error: impl std::fmt::Display) -> TuiError {
    std::io::Error::other(error.to_string()).into()
}

async fn run_async(registry: DriverRegistry) -> Result<(), TuiError> {
    let paths = AppPaths::discover().map_err(map_tui)?;
    let first_run = !crate::entrance::is_complete(&paths.data_dir);
    let animate_entrance = crate::entrance::should_animate(&paths.data_dir);
    let worker = StorageWorker::start(paths.database).map_err(map_tui)?;
    let bootstrap = worker.bootstrap().await.map_err(map_tui)?;
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(32);
    let mut runtime = WorkbenchRuntime::new(action_tx, worker, registry);
    let mut guard = TerminalGuard::enter(CrosstermTerminal)?;
    if animate_entrance {
        let _ = crate::entrance::play_animation();
        crate::entrance::clear_animation()?;
    }
    let animate_logo = first_run && animate_entrance && std::env::var_os("NO_COLOR").is_none();
    let logo_frames = Arc::new(crate::entrance::logo_frames(animate_logo));
    guard.enable_raw()?;
    let result = run_loop(
        bootstrap,
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
    show_onboarding: bool,
    logo_frames: Arc<Vec<crate::entrance::LogoFrame>>,
    runtime: &mut WorkbenchRuntime,
    mut action_rx: tokio::sync::mpsc::Receiver<Action>,
    guard: &mut TerminalGuard<CrosstermTerminal>,
) -> Result<(), TuiError> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut model = Model::default();
    let _ = crate::update::update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));
    model.onboarding.open = show_onboarding;
    model.onboarding.logo_frames = logo_frames;
    guard.set_mouse(model.mouse)?;
    let mut events = EventStream::new();
    let mut onboarding_tick = tokio::time::interval(Duration::from_millis(66));
    onboarding_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut checkpoint = tokio::time::interval(Duration::from_secs(2));
    checkpoint.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let mut hits = crate::mouse::HitMap::default();
        terminal.draw(|frame| crate::render::render(frame, &model, &mut hits))?;
        model.hits = hits;
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
            _ = checkpoint.tick() => {
                let effects = crate::update::update(&mut model, Action::CheckpointTick);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
        }
        guard.set_mouse(model.mouse)?;
    }
    Ok(())
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
