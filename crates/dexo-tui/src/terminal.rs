use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use thiserror::Error;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct TuiError(#[from] io::Error);

static KEYBOARD_ENHANCEMENT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub trait TerminalControl {
    fn enter(&self) -> Result<(), TuiError>;
    fn raw(&self, on: bool) -> Result<(), TuiError>;
    fn leave(&self) -> Result<(), TuiError>;
    fn show_cursor(&self) -> Result<(), TuiError>;
    fn mouse_capture(&self, on: bool) -> Result<(), TuiError>;
    fn keyboard_enhancement(&self, _on: bool) -> Result<bool, TuiError> {
        Ok(false)
    }
}

pub struct TerminalGuard<B: TerminalControl> {
    backend: B,
    restored: bool,
    raw: bool,
    mouse: bool,
    keyboard_enhanced: bool,
}

impl<B: TerminalControl> TerminalGuard<B> {
    pub fn start(backend: B) -> Result<Self, TuiError> {
        let mut guard = Self::enter(backend)?;
        guard.enable_raw()?;
        Ok(guard)
    }

    pub fn enter(backend: B) -> Result<Self, TuiError> {
        backend.enter()?;
        Ok(Self {
            backend,
            restored: false,
            raw: false,
            mouse: false,
            keyboard_enhanced: false,
        })
    }

    pub fn enable_raw(&mut self) -> Result<(), TuiError> {
        if self.raw {
            return Ok(());
        }
        if let Err(error) = self.backend.raw(true) {
            self.restore();
            return Err(error);
        }
        self.raw = true;
        match self.backend.keyboard_enhancement(true) {
            Ok(enabled) => self.keyboard_enhanced = enabled,
            Err(error) => {
                self.restore();
                return Err(error);
            }
        }
        Ok(())
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
        if self.keyboard_enhanced {
            let _ = self.backend.keyboard_enhancement(false);
            self.keyboard_enhanced = false;
        }
        if self.raw {
            let _ = self.backend.raw(false);
            self.raw = false;
        }
        let _ = self.backend.leave();
        let _ = self.backend.show_cursor();
        self.restored = true;
    }
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if KEYBOARD_ENHANCEMENT_ACTIVE.load(Ordering::Relaxed)
            && execute!(io::stdout(), PopKeyboardEnhancementFlags).is_ok()
        {
            KEYBOARD_ENHANCEMENT_ACTIVE.store(false, Ordering::Relaxed);
        }
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

    fn keyboard_enhancement(&self, on: bool) -> Result<bool, TuiError> {
        if on {
            if !matches!(
                crossterm::terminal::supports_keyboard_enhancement(),
                Ok(true)
            ) {
                return Ok(false);
            }
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            KEYBOARD_ENHANCEMENT_ACTIVE.store(true, Ordering::Relaxed);
            Ok(true)
        } else {
            if !KEYBOARD_ENHANCEMENT_ACTIVE.load(Ordering::Relaxed) {
                return Ok(false);
            }
            execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
            KEYBOARD_ENHANCEMENT_ACTIVE.store(false, Ordering::Relaxed);
            Ok(false)
        }
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

    fn keyboard_enhancement(&self, on: bool) -> Result<bool, TuiError> {
        self.push(if on { "keyboard_on" } else { "keyboard_off" });
        Ok(on)
    }
}
