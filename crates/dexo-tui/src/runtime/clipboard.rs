use std::sync::{Mutex, OnceLock};

/// Reads the system clipboard. Ctrl+V is not a paste gesture in most terminals --
/// Ghostty and Alacritty both put it on Shift+Insert -- so the key reaches the app as
/// a key, and an editor that wants it to paste has to fetch the text itself.
pub fn read_text() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|error| error.to_string())
}

pub fn copy_text(text: String) -> Result<(), String> {
    copy_with_adapter(text, os_adapter)
}

/// Asks the terminal to put the text on the system clipboard (OSC 52). It reaches places
/// arboard does not: over SSH, inside tmux, where arboard fell back to XWayland, and after
/// Dexo exits on Wayland, where arboard's copy dies with the process. Terminals that do
/// not know the sequence ignore it.
pub fn copy_via_terminal(text: &str) -> std::io::Result<()> {
    use base64::Engine;
    use std::io::Write;
    let payload = base64::engine::general_purpose::STANDARD.encode(text);
    let mut out = std::io::stdout().lock();
    write!(out, "\x1b]52;c;{payload}\x07")?;
    out.flush()
}

fn os_adapter(text: String) -> Result<(), String> {
    let slot = shared_clipboard();
    // A panic while copying leaves the handle as usable as it was; refusing every later
    // copy for it would be worse.
    let mut guard = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    /// The tests below share the one system clipboard; run in parallel, each could read
    /// the other's text back.
    static SYSTEM_CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn os_clipboard_survives_immediate_reuse() {
        // Exercises the real Linux/X11 ownership path when a display is present.
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let _turn = SYSTEM_CLIPBOARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        copy_text("dexo-clipboard-smoke-1".into()).expect("first copy");
        copy_text("dexo-clipboard-smoke-2".into()).expect("second copy");
        // Read back through the still-alive shared handle (process-exit Drop is separate).
        // On Wayland the compositor hands the selection over a moment later, and a
        // clipboard manager watching it can briefly answer with the one before.
        let slot = super::shared_clipboard();
        let mut text = String::new();
        for _ in 0..40 {
            let mut guard = slot.lock().unwrap();
            text = guard
                .as_mut()
                .expect("shared clipboard")
                .0
                .get_text()
                .expect("read back");
            if text == "dexo-clipboard-smoke-2" {
                break;
            }
            drop(guard);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(text, "dexo-clipboard-smoke-2");
    }

    /// Reading back through the process's own handle proved nothing on Wayland: arboard
    /// wrote to XWayland, which does not pass a windowless client's selection on, so
    /// every copy reported success and reached no other program.
    #[test]
    fn a_copy_reaches_other_programs_on_wayland() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let Ok(probe) = std::process::Command::new("wl-paste")
            .arg("--version")
            .output()
        else {
            return;
        };
        if !probe.status.success() {
            return;
        }
        let _turn = SYSTEM_CLIPBOARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let marker = format!("dexo-wayland-{}", std::process::id());
        copy_text(marker.clone()).expect("copy");
        let pasted = std::process::Command::new("wl-paste")
            .arg("--no-newline")
            .output()
            .expect("wl-paste");
        assert_eq!(String::from_utf8_lossy(&pasted.stdout), marker);
    }
}
