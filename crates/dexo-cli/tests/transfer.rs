use clap::Parser;
use dexo_cli::args::{Args, OnError, TransferCliFormat};

#[test]
fn export_and_import_args_parse() {
    let export = Args::parse_from([
        "dexo",
        "export",
        "--connection",
        "c",
        "--sql",
        "select 1",
        "--format",
        "csv",
        "--output",
        "out.csv",
    ]);
    assert!(matches!(
        export.command,
        Some(dexo_cli::args::Command::Export {
            format: TransferCliFormat::Csv,
            ..
        })
    ));
    let import = Args::parse_from([
        "dexo",
        "import",
        "--connection",
        "c",
        "--table",
        "public.t",
        "--file",
        "in.csv",
        "--format",
        "jsonl",
        "--on-error",
        "skip",
        "--mapping",
        "a=id",
        "--non-interactive",
    ]);
    assert!(matches!(
        import.command,
        Some(dexo_cli::args::Command::Import {
            on_error: OnError::Skip,
            format: TransferCliFormat::Jsonl,
            non_interactive: true,
            ..
        })
    ));
}
