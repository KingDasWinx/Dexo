use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "dexo",
    version,
    about = "Local-first terminal database workbench"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug)]
pub enum LaunchMode {
    Tui,
    Cli(Command),
}

impl Args {
    pub fn launch_mode(self) -> LaunchMode {
        match self.command {
            None => LaunchMode::Tui,
            Some(command) => LaunchMode::Cli(command),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Csv,
    Tsv,
    Json,
    Jsonl,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Query {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        sql: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
        format: OutputFormat,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long = "param")]
        param: Vec<String>,
        #[arg(long)]
        continue_on_error: bool,
    },
    Run {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
        format: OutputFormat,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long = "param")]
        param: Vec<String>,
        #[arg(long)]
        continue_on_error: bool,
    },
    Inspect {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        object: Option<String>,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        snapshot: Option<String>,
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        grants: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    Export {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        sql: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = TransferCliFormat::Csv)]
        format: TransferCliFormat,
    },
    Import {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        table: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = TransferCliFormat::Csv)]
        format: TransferCliFormat,
        #[arg(long = "on-error", value_enum, default_value_t = OnError::Stop)]
        on_error: OnError,
        #[arg(long)]
        mapping: Vec<String>,
        #[arg(long)]
        non_interactive: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SchemaDiffFormat {
    Json,
    Sql,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TransferCliFormat {
    Csv,
    Tsv,
    Json,
    Jsonl,
    Sql,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OnError {
    Stop,
    Skip,
    Reject,
}

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    Snapshot {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Diff {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, value_enum, default_value_t = SchemaDiffFormat::Json)]
        format: SchemaDiffFormat,
        #[arg(long)]
        apply: bool,
        #[arg(long = "confirm-target")]
        confirm_target: Option<String>,
        #[arg(long)]
        rename: Vec<String>,
        #[arg(long)]
        connection: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Export {
        #[arg(long)]
        output: PathBuf,
    },
    Import {
        #[arg(long)]
        input: PathBuf,
    },
}
