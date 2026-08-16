use clap::CommandFactory;
use dexo_cli::args::Args;

#[test]
fn long_help_matches_checked_in_reference() {
    let mut cmd = Args::command();
    let mut buf = Vec::new();
    cmd.write_long_help(&mut buf).unwrap();
    let got = String::from_utf8(buf)
        .unwrap()
        .replace(env!("CARGO_PKG_VERSION"), "VERSION");
    let expected = include_str!("fixtures/help.txt");
    assert_eq!(got, expected);
}

#[test]
fn public_commands_are_documented() {
    let help = include_str!("fixtures/help.txt");
    for name in [
        "connections",
        "query",
        "run",
        "inspect",
        "export",
        "import",
        "schema",
        "explain",
        "sessions",
        "config",
        "completion",
        "mcp",
        "doctor",
    ] {
        assert!(help.contains(name), "missing {name}");
    }
}

#[test]
fn connections_help_documents_add_and_set_secret() {
    let mut cmd = Args::command();
    let add = cmd
        .find_subcommand_mut("connections")
        .unwrap()
        .find_subcommand_mut("add")
        .unwrap();
    let mut buf = Vec::new();
    add.write_long_help(&mut buf).unwrap();
    let help = String::from_utf8(buf).unwrap();
    assert!(help.contains("--password-stdin"));
    assert!(!help.contains("--password "));
    let connections_help = {
        let mut cmd = Args::command();
        let connections = cmd.find_subcommand_mut("connections").unwrap();
        let mut buf = Vec::new();
        connections.write_long_help(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    };
    assert!(connections_help.contains("add"));
    assert!(connections_help.contains("set-secret"));
}
