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

fn events(rows: Vec<Vec<dexo_driver_api::DbValue>>) -> Vec<dexo_driver_api::QueryEvent> {
    use dexo_driver_api::{ColumnMeta, QueryEvent, RowBatch};
    let column = |name: &str| ColumnMeta {
        name: name.into(),
        type_name: "text".into(),
        nullable: true,
    };
    vec![
        QueryEvent::Columns(vec![column("name"), column("note")]),
        QueryEvent::Rows(RowBatch { rows }),
    ]
}

fn render(format: OutputFormat, rows: Vec<Vec<dexo_driver_api::DbValue>>) -> String {
    let mut stdout = Vec::new();
    present_events(format, &events(rows), &mut stdout, &mut Vec::new()).unwrap();
    String::from_utf8(stdout).unwrap()
}

/// CSV quotes a value holding a comma, a quote or a line break, and tells NULL from an
/// empty string, as `dexo export` does.
#[test]
fn csv_quotes_what_needs_quoting_and_keeps_null_apart() {
    use dexo_driver_api::DbValue::{Null, Text};
    let csv = render(
        OutputFormat::Csv,
        vec![
            vec![
                Text("Silva, Ana".into()),
                Text("said \"hi\"\nthen left".into()),
            ],
            vec![Text(String::new()), Null],
        ],
    );
    assert_eq!(
        csv,
        "name,note\n\"Silva, Ana\",\"said \"\"hi\"\"\nthen left\"\n,\\N\n"
    );
}

/// The table lines its columns up under their names, and shows NULL as `<null>`.
#[test]
fn the_table_lines_up_and_shows_null() {
    use dexo_driver_api::DbValue::{Null, Text};
    let table = render(
        OutputFormat::Table,
        vec![
            vec![Text("Ana".into()), Null],
            vec![Text("Bernardo".into()), Text("two\nlines".into())],
        ],
    );
    assert_eq!(
        table,
        "name     | note\n---------+----------\nAna      | <null>\nBernardo | two lines\n"
    );
}
