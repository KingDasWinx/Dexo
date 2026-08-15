use crossterm::event::{Event, EventStream, KeyEventKind};
use dexo_app::DriverRegistry;
use dexo_storage::AppPaths;
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::VecDeque;
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
    let worker = StorageWorker::start(paths.database).map_err(map_tui)?;
    let bootstrap = worker.bootstrap().await.map_err(map_tui)?;
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(32);
    let mut runtime = WorkbenchRuntime::new(action_tx, worker, registry);
    let mut guard = TerminalGuard::start(CrosstermTerminal)?;
    let result = run_loop(bootstrap, &mut runtime, action_rx).await;
    runtime.dispatch(Effect::Shutdown).await;
    guard.restore();
    result
}

async fn run_loop(
    bootstrap: crate::runtime::storage_worker::BootstrapState,
    runtime: &mut WorkbenchRuntime,
    mut action_rx: tokio::sync::mpsc::Receiver<Action>,
) -> Result<(), TuiError> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut model = Model::default();
    let _ = crate::update::update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));
    let mut events = EventStream::new();
    let mut checkpoint = tokio::time::interval(Duration::from_secs(2));
    checkpoint.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        terminal.draw(|frame| crate::render::render(frame, &model))?;
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
            _ = checkpoint.tick() => {
                let effects = crate::update::update(&mut model, Action::CheckpointTick);
                if dispatch_effects(runtime, &mut action_rx, &mut model, effects).await {
                    return Ok(());
                }
            }
        }
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
