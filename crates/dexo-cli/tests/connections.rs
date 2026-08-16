use clap::Parser;
use dexo_cli::args::{Args, ConnectionsCommand};

#[test]
fn connections_add_args_parse() {
    let args = Args::parse_from([
        "dexo",
        "connections",
        "add",
        "--name",
        "local-pg",
        "--driver",
        "postgres",
        "--host",
        "127.0.0.1",
        "--username",
        "dexo",
        "--database",
        "dexo",
        "--non-interactive",
        "--password-stdin",
        "--no-test",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Connections {
            command: ConnectionsCommand::Add { ref name, no_test: true, .. }
        }) if name == "local-pg"
    ));
}

#[test]
fn connections_set_secret_args_parse() {
    let args = Args::parse_from([
        "dexo",
        "connections",
        "set-secret",
        "--name",
        "local-pg",
        "--password-stdin",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Connections {
            command: ConnectionsCommand::SetSecret { ref name, .. }
        }) if name == "local-pg"
    ));
}
