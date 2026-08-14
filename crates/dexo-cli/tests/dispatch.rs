use clap::Parser;
use dexo_app::DriverRegistry;
use dexo_cli::args::{Args, LaunchMode};
use dexo_cli::run::run_dispatch;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn doctor_json_does_not_enter_raw_mode() {
    let started = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&started);
    let args = Args::parse_from(["dexo", "doctor", "--json"]);
    assert!(matches!(
        Args::parse_from(["dexo", "doctor", "--json"]).launch_mode(),
        LaunchMode::Cli(_)
    ));
    run_dispatch(args, DriverRegistry::new(), move || {
        flag.store(true, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert!(
        !started.load(Ordering::SeqCst),
        "CLI doctor must not start the TUI runner"
    );
}

#[test]
fn bare_dexo_invokes_tui_runner() {
    let started = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&started);
    let args = Args::parse_from(["dexo"]);
    assert!(matches!(args.launch_mode(), LaunchMode::Tui));
    let args = Args::parse_from(["dexo"]);
    run_dispatch(args, DriverRegistry::new(), move || {
        flag.store(true, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert!(started.load(Ordering::SeqCst));
}
