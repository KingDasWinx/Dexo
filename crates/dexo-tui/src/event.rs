use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::action::{Action, Effect};
use crate::model::Model;
use crate::terminal::{CrosstermTerminal, TerminalGuard, TuiError};

pub fn action_from_event(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Action::Key(key)),
        Event::Mouse(mouse) => Some(Action::Mouse(mouse)),
        Event::Resize(width, height) => Some(Action::Resize { width, height }),
        _ => None,
    }
}

pub fn run() -> Result<(), TuiError> {
    tokio::runtime::Runtime::new()?.block_on(run_async())
}

async fn run_async() -> Result<(), TuiError> {
    let mut guard = TerminalGuard::start(CrosstermTerminal)?;
    let result = run_loop().await;
    guard.restore();
    result
}

async fn run_loop() -> Result<(), TuiError> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut model = Model::default();
    let mut events = EventStream::new();
    loop {
        terminal.draw(|frame| crate::render::render(frame, &model))?;
        let Some(event) = events.next().await else {
            break;
        };
        let Some(action) = action_from_event(event?) else {
            continue;
        };
        for effect in crate::update::update(&mut model, action) {
            // ponytail: query/tx effects stay idle until a connected session is injected into the loop
            if matches!(effect, Effect::Quit) {
                return Ok(());
            }
        }
    }
    Ok(())
}
