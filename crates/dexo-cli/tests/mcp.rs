use clap::Parser;
use dexo_cli::args::{Args, McpCommand, McpConfigCommand, McpProfileCommand};

#[test]
fn mcp_admin_args_parse() {
    let serve = Args::parse_from(["dexo", "mcp", "serve", "--profile", "assistant"]);
    assert!(matches!(
        serve.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Serve { ref profile }
        }) if profile == "assistant"
    ));
    let enable = Args::parse_from([
        "dexo",
        "mcp",
        "profile",
        "enable",
        "--name",
        "assistant",
        "--confirm",
    ]);
    assert!(matches!(
        enable.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Profile {
                command: McpProfileCommand::Enable { confirm: true, .. }
            }
        })
    ));
    let allow = Args::parse_from([
        "dexo",
        "mcp",
        "allow",
        "--profile",
        "assistant",
        "--selector",
        "db.public.*",
    ]);
    assert!(matches!(
        allow.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Allow { .. }
        })
    ));
    let policy = Args::parse_from(["dexo", "mcp", "policy", "--profile", "assistant"]);
    assert!(matches!(
        policy.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Policy { .. }
        })
    ));
    let doctor = Args::parse_from(["dexo", "mcp", "doctor", "--profile", "assistant", "--json"]);
    assert!(matches!(
        doctor.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Doctor { json: true, .. }
        })
    ));
    let config = Args::parse_from([
        "dexo",
        "mcp",
        "config",
        "print",
        "--profile",
        "assistant",
        "--client",
        "cursor",
    ]);
    assert!(matches!(
        config.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Config {
                command: McpConfigCommand::Print { .. }
            }
        })
    ));
}

#[test]
fn mcp_profile_lifecycle_and_config_print() {
    use dexo_app::DriverRegistry;
    use dexo_cli::run::run_dispatch;
    use std::sync::{Arc, atomic::AtomicBool};

    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("DEXO_DATA_HOME", dir.path()) }

    let noop = || {
        let _ = Arc::new(AtomicBool::new(false));
        Ok(())
    };
    run_dispatch(
        Args::parse_from(["dexo", "mcp", "profile", "create", "--name", "assistant"]),
        DriverRegistry::new(),
        noop,
    )
    .unwrap();
    let enable = run_dispatch(
        Args::parse_from(["dexo", "mcp", "profile", "enable", "--name", "assistant"]),
        DriverRegistry::new(),
        || Ok(()),
    );
    assert!(enable.is_err());
    run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "profile",
            "enable",
            "--name",
            "assistant",
            "--confirm",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "allow",
            "--profile",
            "assistant",
            "--selector",
            "db.public.*",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from(["dexo", "mcp", "policy", "--profile", "assistant"]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from(["dexo", "mcp", "doctor", "--profile", "assistant", "--json"]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "config",
            "print",
            "--profile",
            "assistant",
            "--client",
            "cursor",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
}
