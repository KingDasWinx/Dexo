use dexo_cli::args::OutputFormat;
use dexo_cli::run::{present_events, sample_select_one};

#[test]
fn jsonl_query_keeps_diagnostics_off_stdout() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    present_events(
        OutputFormat::Jsonl,
        &sample_select_one(),
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(String::from_utf8(stdout).unwrap(), "{\"n\":1}\n");
    assert!(stderr.is_empty());
}

#[test]
fn query_args_parse() {
    use clap::Parser;
    use dexo_cli::args::Args;
    let args = Args::parse_from([
        "dexo",
        "query",
        "--connection",
        "fixture",
        "--sql",
        "select 1 as n",
        "--format",
        "jsonl",
        "--non-interactive",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Query { .. })
    ));
}

#[test]
fn continue_on_error_flag_parses() {
    use clap::Parser;
    use dexo_cli::args::Args;
    let args = Args::parse_from([
        "dexo",
        "query",
        "--connection",
        "fixture",
        "--sql",
        "select 1; select 2",
        "--continue-on-error",
    ]);
    assert!(matches!(
        args.command,
        Some(dexo_cli::args::Command::Query {
            continue_on_error: true,
            ..
        })
    ));
}
