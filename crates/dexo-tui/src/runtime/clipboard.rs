use std::sync::{Mutex, OnceLock};

pub fn copy_text(text: String) -> Result<(), String> {
    copy_with_adapter(text, os_adapter)
}

fn os_adapter(text: String) -> Result<(), String> {
    let slot = shared_clipboard();
    let mut guard = slot.lock().map_err(|e| e.to_string())?;
    set_text_reusing(
        &mut guard,
        || {
            Ok(ArboardBackend(
                arboard::Clipboard::new().map_err(|e| e.to_string())?,
            ))
        },
        text,
    )
}

/// Keeps one OS clipboard handle alive for the process lifetime.
///
/// On Linux, dropping `arboard::Clipboard` right after `set_text` tears down
/// selection ownership before clipboard managers can read the contents — and
/// arboard may `eprintln!` a warning that corrupts the alternate-screen TUI.
fn shared_clipboard() -> &'static Mutex<Option<ArboardBackend>> {
    static CLIPBOARD: OnceLock<Mutex<Option<ArboardBackend>>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(None))
}

pub fn copy_with_adapter<F>(text: String, adapter: F) -> Result<(), String>
where
    F: FnOnce(String) -> Result<(), String>,
{
    adapter(text)
}

fn set_text_reusing<B, F>(slot: &mut Option<B>, open: F, text: String) -> Result<(), String>
where
    B: ClipboardBackend,
    F: FnOnce() -> Result<B, String>,
{
    if slot.is_none() {
        *slot = Some(open()?);
    }
    slot.as_mut()
        .expect("backend just initialized")
        .set_text(text)
}

trait ClipboardBackend {
    fn set_text(&mut self, text: String) -> Result<(), String>;
}

struct ArboardBackend(arboard::Clipboard);

impl ClipboardBackend for ArboardBackend {
    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.0.set_text(text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardBackend, copy_text, copy_with_adapter, set_text_reusing};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn headless_adapter_failure_is_err() {
        let err = copy_with_adapter("secret".into(), |_| Err("denied".into()));
        assert_eq!(err.unwrap_err(), "denied");
    }

    #[test]
    fn headless_adapter_success_is_ok() {
        assert!(copy_with_adapter("ok".into(), |_| Ok(())).is_ok());
    }

    struct CountingBackend;

    impl ClipboardBackend for CountingBackend {
        fn set_text(&mut self, _text: String) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn clipboard_backend_is_reused_across_copies() {
        static OPENS: AtomicUsize = AtomicUsize::new(0);
        let mut slot = None;
        let open = || {
            OPENS.fetch_add(1, Ordering::SeqCst);
            Ok(CountingBackend)
        };

        set_text_reusing(&mut slot, open, "one".into()).unwrap();
        set_text_reusing(&mut slot, open, "two".into()).unwrap();

        assert_eq!(OPENS.load(Ordering::SeqCst), 1);
        assert!(slot.is_some());
    }

    #[test]
    fn os_clipboard_survives_immediate_reuse() {
        // Exercises the real Linux/X11 ownership path when a display is present.
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        copy_text("dexo-clipboard-smoke-1".into()).expect("first copy");
        copy_text("dexo-clipboard-smoke-2".into()).expect("second copy");
        // Read back through the still-alive shared handle (process-exit Drop is separate).
        let slot = super::shared_clipboard();
        let mut guard = slot.lock().unwrap();
        let text = guard
            .as_mut()
            .expect("shared clipboard")
            .0
            .get_text()
            .expect("read back");
        assert_eq!(text, "dexo-clipboard-smoke-2");
    }
}
