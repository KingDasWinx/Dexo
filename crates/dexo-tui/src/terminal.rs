use std::io;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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
    /// Without this a paste arrives as the keys it happens to look like: every
    /// character dispatched and redrawn on its own, and every tab in the text firing
    /// whatever tab is bound to instead of landing as whitespace.
    fn bracketed_paste(&self, _on: bool) -> Result<(), TuiError> {
        Ok(())
    }
    /// Tells the terminal what colour to draw the caret, or restores its own. Dexo
    /// repaints the surface, so the colour the terminal was configured with is about a
    /// background that is no longer there.
    fn cursor_color(&self, _rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        Ok(())
    }
    /// Sets the terminal's default background, or hands its own back. The entrance
    /// resets to that default after every cell, so painting a background over it does
    /// not survive; only changing the default does.
    fn background_color(&self, _rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        Ok(())
    }
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
    caret: Option<(u8, u8, u8)>,
    background: Option<(u8, u8, u8)>,
    paste: bool,
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
            caret: None,
            background: None,
            paste: false,
        })
    }

    pub fn enable_paste(&mut self) -> Result<(), TuiError> {
        if self.paste {
            return Ok(());
        }
        self.backend.bracketed_paste(true)?;
        self.paste = true;
        Ok(())
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
        // Bracketed paste belongs with raw mode: both say the app is driving the
        // terminal now. Hanging it off `start` instead left the real entry path --
        // `enter` then `enable_raw` -- without it, and only the tests with it.
        self.enable_paste()?;
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

    /// Idempotent like `set_mouse`: the loop offers the theme's caret colour every
    /// frame and only a change reaches the terminal.
    pub fn set_cursor_color(&mut self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        if self.caret == rgb {
            return Ok(());
        }
        self.backend.cursor_color(rgb)?;
        self.caret = rgb;
        Ok(())
    }

    /// Idempotent like the caret: offered every frame, forwarded only when it changes.
    pub fn set_background_color(&mut self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        if self.background == rgb {
            return Ok(());
        }
        self.backend.background_color(rgb)?;
        self.background = rgb;
        Ok(())
    }

    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        if self.background.is_some() {
            let _ = self.backend.background_color(None);
            self.background = None;
        }
        if self.paste {
            let _ = self.backend.bracketed_paste(false);
            self.paste = false;
        }
        if self.caret.is_some() {
            let _ = self.backend.cursor_color(None);
            self.caret = None;
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
        let _ = write!(io::stdout(), "\x1b]112\x1b\\\x1b]111\x1b\\");
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
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

    fn bracketed_paste(&self, on: bool) -> Result<(), TuiError> {
        if on {
            execute!(io::stdout(), EnableBracketedPaste)?;
        } else {
            execute!(io::stdout(), DisableBracketedPaste)?;
        }
        Ok(())
    }

    fn background_color(&self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        let mut out = io::stdout();
        match rgb {
            Some((r, g, b)) => write!(out, "\x1b]11;#{r:02x}{g:02x}{b:02x}\x1b\\")?,
            None => write!(out, "\x1b]111\x1b\\")?,
        }
        out.flush()?;
        Ok(())
    }

    fn cursor_color(&self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        // OSC 12 sets it, OSC 112 hands it back. Terminals that do not know either
        // ignore the sequence, which is the whole point of asking this way.
        let mut out = io::stdout();
        match rgb {
            Some((r, g, b)) => write!(out, "\x1b]12;#{r:02x}{g:02x}{b:02x}\x1b\\")?,
            None => write!(out, "\x1b]112\x1b\\")?,
        }
        out.flush()?;
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

    fn bracketed_paste(&self, on: bool) -> Result<(), TuiError> {
        self.push(if on { "paste_on" } else { "paste_off" });
        Ok(())
    }

    fn background_color(&self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        self.push(if rgb.is_some() {
            "background_set"
        } else {
            "background_reset"
        });
        Ok(())
    }

    fn cursor_color(&self, rgb: Option<(u8, u8, u8)>) -> Result<(), TuiError> {
        self.push(if rgb.is_some() {
            "cursor_color_set"
        } else {
            "cursor_color_reset"
        });
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
