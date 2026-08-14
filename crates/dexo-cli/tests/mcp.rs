use clap::Parser;
use dexo_cli::args::{Args, McpCommand, McpConfigCommand, McpGrantCommand, McpProfileCommand};

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
fn mcp_grant_args_parse() {
    let create = Args::parse_from([
        "dexo",
        "mcp",
        "grant",
        "create",
        "--profile",
        "assistant",
        "--connection",
        "local",
        "--capability",
        "data_write",
        "--tool",
        "data_insert",
        "--selector",
        "db.public.items",
        "--expires",
        "15m",
        "--confirm-target",
        "local",
    ]);
    assert!(matches!(
        create.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Grant {
                command: McpGrantCommand::Create { ref expires, .. }
            }
        }) if expires == "15m"
    ));
    let list = Args::parse_from(["dexo", "mcp", "grant", "list", "--profile", "assistant"]);
    assert!(matches!(
        list.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Grant {
                command: McpGrantCommand::List { .. }
            }
        })
    ));
    let revoke = Args::parse_from([
        "dexo",
        "mcp",
        "grant",
        "revoke",
        "--id",
        "00000000-0000-0000-0000-000000000001",
    ]);
    assert!(matches!(
        revoke.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Grant {
                command: McpGrantCommand::Revoke { .. }
            }
        })
    ));
}

#[test]
fn mcp_grant_lifecycle_and_audit_export() {
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
    let missing_confirm = run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "grant",
            "create",
            "--profile",
            "assistant",
            "--connection",
            "local",
            "--capability",
            "data_write",
            "--tool",
            "data_insert",
            "--selector",
            "db.public.items",
            "--expires",
            "15m",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    );
    assert!(missing_confirm.is_err());
    run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "grant",
            "create",
            "--profile",
            "assistant",
            "--connection",
            "local",
            "--capability",
            "data_write",
            "--tool",
            "data_insert",
            "--selector",
            "db.public.items",
            "--expires",
            "15m",
            "--confirm-target",
            "local",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from(["dexo", "mcp", "grant", "list", "--profile", "assistant"]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from([
            "dexo",
            "mcp",
            "grant",
            "revoke-all",
            "--profile",
            "assistant",
        ]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    run_dispatch(
        Args::parse_from(["dexo", "mcp", "audit", "--profile", "assistant"]),
        DriverRegistry::new(),
        || Ok(()),
    )
    .unwrap();
    let tools = dexo_app::mcp::advertised_tools(&dexo_app::mcp::McpProfile::new("assistant"));
    assert!(!tools.iter().any(|name| name.contains("grant")));
}
