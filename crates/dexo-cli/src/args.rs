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
    Explain {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        sql: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        analyze: bool,
        #[arg(long)]
        confirm: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
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
pub enum SessionsCommand {
    List {
        #[arg(long)]
        connection: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    Cancel {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        session: String,
        #[arg(long)]
        confirm: bool,
    },
    Terminate {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        session: String,
        #[arg(long = "confirm-target")]
        confirm_target: Option<String>,
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

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    Profile {
        #[command(subcommand)]
        command: McpProfileCommand,
    },
    Allow {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        selector: String,
        #[arg(long)]
        deny: bool,
    },
    Policy {
        #[arg(long)]
        profile: String,
    },
    Doctor {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: McpConfigCommand,
    },
    Serve {
        #[arg(long)]
        profile: String,
    },
    Grant {
        #[command(subcommand)]
        command: McpGrantCommand,
    },
    Audit {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpProfileCommand {
    List,
    Create {
        #[arg(long)]
        name: String,
    },
    Show {
        #[arg(long)]
        name: String,
    },
    Enable {
        #[arg(long)]
        name: String,
        #[arg(long)]
        confirm: bool,
    },
    Disable {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpGrantCommand {
    Create {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        capability: String,
        #[arg(long)]
        tool: Vec<String>,
        #[arg(long)]
        selector: String,
        #[arg(long, default_value = "15m")]
        expires: String,
        #[arg(long = "confirm-target")]
        confirm_target: Option<String>,
    },
    List {
        #[arg(long)]
        profile: String,
    },
    Revoke {
        #[arg(long)]
        id: String,
    },
    RevokeAll {
        #[arg(long)]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpConfigCommand {
    Print {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        client: Option<String>,
    },
}
