use dexo_tui::{RecordingTerminal, TerminalGuard};

#[test]
fn terminal_guard_restores_once() {
    let backend = RecordingTerminal::default();
    {
        let _guard = TerminalGuard::start(backend.clone()).unwrap();
    }
    assert_eq!(
        backend.calls(),
        vec!["enter", "raw_on", "raw_off", "leave", "cursor_show"]
    );
}

#[test]
fn terminal_guard_restores_on_panic() {
    let backend = RecordingTerminal::default();
    let _ = std::panic::catch_unwind(|| {
        let _guard = TerminalGuard::start(backend.clone()).unwrap();
        panic!("boom");
    });
    assert_eq!(
        backend.calls(),
        vec!["enter", "raw_on", "raw_off", "leave", "cursor_show"]
    );
}

#[test]
fn terminal_guard_restore_is_idempotent() {
    let backend = RecordingTerminal::default();
    {
        let mut guard = TerminalGuard::start(backend.clone()).unwrap();
        guard.restore();
        guard.restore();
    }
    assert_eq!(
        backend.calls(),
        vec!["enter", "raw_on", "raw_off", "leave", "cursor_show"]
    );
}
