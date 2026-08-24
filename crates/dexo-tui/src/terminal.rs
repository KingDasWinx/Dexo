use std::io;
use std::sync::{Arc, Mutex};

use crossterm::cursor::Show;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use thiserror::Error;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct TuiError(#[from] io::Error);

pub trait TerminalControl {
    fn enter(&self) -> Result<(), TuiError>;
    fn raw(&self, on: bool) -> Result<(), TuiError>;
    fn leave(&self) -> Result<(), TuiError>;
    fn show_cursor(&self) -> Result<(), TuiError>;
    fn mouse_capture(&self, on: bool) -> Result<(), TuiError>;
}

pub struct TerminalGuard<B: TerminalControl> {
    backend: B,
    restored: bool,
    mouse: bool,
}

impl<B: TerminalControl> TerminalGuard<B> {
    pub fn start(backend: B) -> Result<Self, TuiError> {
        backend.enter()?;
        if let Err(error) = backend.raw(true) {
            let _ = backend.leave();
            let _ = backend.show_cursor();
            return Err(error);
        }
        Ok(Self {
            backend,
            restored: false,
            mouse: false,
        })
    }

    pub fn set_mouse(&mut self, on: bool) -> Result<(), TuiError> {
        if self.mouse == on {
            return Ok(());
        }
        self.backend.mouse_capture(on)?;
        self.mouse = on;
        Ok(())
    }

    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        if self.mouse {
            let _ = self.backend.mouse_capture(false);
            self.mouse = false;
        }
        let _ = self.backend.raw(false);
        let _ = self.backend.leave();
        let _ = self.backend.show_cursor();
        self.restored = true;
    }
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
        previous(info);
    }));
}

impl<B: TerminalControl> Drop for TerminalGuard<B> {
    fn drop(&mut self) {
        self.restore();
    }
}

pub struct CrosstermTerminal;

impl TerminalControl for CrosstermTerminal {
    fn enter(&self) -> Result<(), TuiError> {
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(())
    }

    fn raw(&self, on: bool) -> Result<(), TuiError> {
        if on {
            enable_raw_mode()?;
        } else {
            disable_raw_mode()?;
        }
        Ok(())
    }

    fn leave(&self) -> Result<(), TuiError> {
        execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
        Ok(())
    }

    fn show_cursor(&self) -> Result<(), TuiError> {
        execute!(io::stdout(), Show)?;
        Ok(())
    }

    fn mouse_capture(&self, on: bool) -> Result<(), TuiError> {
        if on {
            execute!(io::stdout(), EnableMouseCapture)?;
        } else {
            execute!(io::stdout(), DisableMouseCapture)?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct RecordingTerminal {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingTerminal {
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .expect("recording terminal poisoned")
            .clone()
    }

    fn push(&self, call: &'static str) {
        self.calls
            .lock()
            .expect("recording terminal poisoned")
            .push(call);
    }
}

impl TerminalControl for RecordingTerminal {
    fn enter(&self) -> Result<(), TuiError> {
        self.push("enter");
        Ok(())
    }

    fn raw(&self, on: bool) -> Result<(), TuiError> {
        self.push(if on { "raw_on" } else { "raw_off" });
        Ok(())
    }

    fn leave(&self) -> Result<(), TuiError> {
        self.push("leave");
        Ok(())
    }

    fn show_cursor(&self) -> Result<(), TuiError> {
        self.push("cursor_show");
        Ok(())
    }

    fn mouse_capture(&self, on: bool) -> Result<(), TuiError> {
        self.push(if on { "mouse_on" } else { "mouse_off" });
        Ok(())
    }
}
