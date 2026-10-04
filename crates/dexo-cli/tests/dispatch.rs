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
    assert!(matches!(
        args.launch_mode(),
        LaunchMode::Tui(dexo_cli::args::TuiStart::Workbench)
    ));
    let args = Args::parse_from(["dexo", "--password-prompt", "postgres://ana@db/shop"]);
    assert!(matches!(
        args.launch_mode(),
        LaunchMode::Tui(dexo_cli::args::TuiStart::Url {
            password_prompt: true,
            ..
        })
    ));
    assert!(matches!(
        Args::parse_from(["dexo", "--demo"]).launch_mode(),
        LaunchMode::Tui(dexo_cli::args::TuiStart::Demo)
    ));
    assert!(Args::try_parse_from(["dexo", "--demo", "postgres://ana@db/shop"]).is_err());
    let args = Args::parse_from(["dexo"]);
    run_dispatch(args, DriverRegistry::new(), move || {
        flag.store(true, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert!(started.load(Ordering::SeqCst));
}

/// Options that cannot go together are refused rather than one of them dropped.
#[test]
fn connections_add_refuses_options_that_cannot_go_together() {
    let add = |extra: &[&str]| {
        let mut argv = vec![
            "dexo",
            "connections",
            "add",
            "--name",
            "n",
            "--driver",
            "sqlite",
        ];
        argv.extend_from_slice(extra);
        Args::try_parse_from(argv)
    };
    assert!(add(&["--path", "x.db"]).is_ok());
    assert!(add(&["--path", "x.db", "--password-command", "pass x"]).is_err());
    assert!(
        add(&[
            "--host",
            "h",
            "--database",
            "d",
            "--username",
            "u",
            "--password-command",
            "pass x",
            "--password-stdin"
        ])
        .is_err()
    );
}

/// An asking grant's wait is 1 s to an hour; zero or more is refused, not stored.
#[test]
fn approval_timeout_is_bounded() {
    let create = |timeout: &str| {
        Args::try_parse_from([
            "dexo",
            "mcp",
            "grant",
            "create",
            "--profile",
            "p",
            "--connection",
            "c",
            "--capability",
            "data_write",
            "--tool",
            "data_insert",
            "--selector",
            "db.public.t",
            "--ask",
            "--approval-timeout",
            timeout,
        ])
    };
    assert!(create("120").is_ok());
    assert!(create("3600").is_ok());
    assert!(create("0").is_err());
    assert!(create("3601").is_err());
}
