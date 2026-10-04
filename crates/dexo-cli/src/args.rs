use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "dexo",
    version,
    about = "A terminal database workbench with guardrails for AI agents",
    args_conflicts_with_subcommands = true
)]
pub struct Args {
    /// Open the workbench connected to this URL, without saving it: postgres://user@host/db,
    /// mysql://…, mariadb://…, sqlite:///path/to/file, duckdb:///path/to/file.parquet.
    #[arg(value_name = "URL")]
    pub url: Option<String>,
    /// Ask for the URL's password on the terminal instead of putting it in the URL, where
    /// shell history keeps it.
    #[arg(long, requires = "url")]
    pub password_prompt: bool,
    /// Open a sample shop (customers, products, orders) to try every screen on, with
    /// nothing to install or connect to. It starts over on every run.
    #[arg(long, conflicts_with = "url")]
    pub demo: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TuiStart {
    Workbench,
    Url { url: String, password_prompt: bool },
    Demo,
}

#[derive(Debug)]
pub enum LaunchMode {
    Tui(TuiStart),
    Cli(Command),
}

impl Args {
    pub fn launch_mode(self) -> LaunchMode {
        match (self.command, self.url) {
            (Some(command), _) => LaunchMode::Cli(command),
            (None, Some(url)) => LaunchMode::Tui(TuiStart::Url {
                url,
                password_prompt: self.password_prompt,
            }),
            (None, None) if self.demo => LaunchMode::Tui(TuiStart::Demo),
            (None, None) => LaunchMode::Tui(TuiStart::Workbench),
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
    /// Say whether Dexo runs, for a script that checks; `dexo mcp doctor` checks agents' setup
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Add, list and test saved connections, and set their passwords
    Connections {
        #[command(subcommand)]
        command: ConnectionsCommand,
    },
    /// Print a completion script for bash, zsh, fish, PowerShell or Elvish
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print, export and import the projects and connections, without passwords
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Run SQL on a connection and print the rows
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
        /// Run the destructive statements the connection asks about (DELETE or UPDATE
        /// without WHERE, DROP, TRUNCATE).
        #[arg(long)]
        confirm: bool,
        /// The connection's name: a production connection runs no write without it.
        #[arg(long = "confirm-target", value_name = "CONNECTION")]
        confirm_target: Option<String>,
    },
    /// Run a SQL file on a connection, statement by statement
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
        /// Run the destructive statements the connection asks about (DELETE or UPDATE
        /// without WHERE, DROP, TRUNCATE).
        #[arg(long)]
        confirm: bool,
        /// The connection's name: a production connection runs no write without it.
        #[arg(long = "confirm-target", value_name = "CONNECTION")]
        confirm_target: Option<String>,
    },
    /// Show a connection's catalog: an object, a search, the grants, or a cached snapshot
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
    /// Snapshot a schema, or diff two of them into a migration
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    /// Write a query's rows to a CSV, TSV, JSON, JSON Lines or SQL file
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
        /// The table the SQL format inserts into; the output file's name by default.
        #[arg(long)]
        table: Option<String>,
    },
    /// Load a CSV, TSV, JSON or JSON Lines file into a table
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
        /// The file's column into the table's, as source=target, matched by name;
        /// source= leaves it out. Repeat for several.
        #[arg(long)]
        mapping: Vec<String>,
        #[arg(long)]
        non_interactive: bool,
    },
    /// Show a statement's plan, estimated or analyzed
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
        /// Plan as if this index were built (Postgres with hypopg), e.g.
        /// "CREATE INDEX ON orders (customer_id)". Repeat for several.
        #[arg(long = "index", value_name = "DEFINITION")]
        indexes: Vec<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// List the server's sessions, and cancel or end one
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    /// Serve connections to AI agents over MCP, and manage what they may do
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Language server for editors: completion, diagnostics and formatting over stdio.
    Lsp {
        /// The connection whose cached catalog completes and checks documents; a file's
        /// first line `-- dexo: connection=name` names its own.
        #[arg(long)]
        connection: Option<String>,
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
    /// Save a connection's schema as a snapshot to diff against later
    Snapshot {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compare two saved snapshots as JSON or SQL; --apply runs the migration on a connection
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
    /// List the server's sessions
    List {
        #[arg(long)]
        connection: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Cancel the query a session is running
    Cancel {
        #[arg(long)]
        connection: String,
        #[arg(long)]
        session: String,
        #[arg(long)]
        confirm: bool,
    },
    /// End a session; its id has to be confirmed
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
pub enum ConnectionsCommand {
    /// List the saved connections
    List,
    /// Save a connection; a password it is given is kept in the OS keychain
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        driver: String,
        #[arg(long, default_value = "", required_unless_present = "path")]
        host: String,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, default_value = "", required_unless_present = "path")]
        database: String,
        #[arg(long, default_value = "", required_unless_present = "path")]
        username: String,
        /// The database file, for a driver that opens one (sqlite, duckdb).
        #[arg(
            long,
            conflicts_with_all = ["host", "port", "database", "username", "password_command", "password_stdin", "pre_connect"]
        )]
        path: Option<String>,
        #[arg(long, default_value = "local")]
        environment: String,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        password_stdin: bool,
        /// Read the password from this command at every connect (`op read …`, `pass show …`).
        #[arg(long, conflicts_with = "password_stdin")]
        password_command: Option<String>,
        /// Run this before connecting and wait for its port (`kubectl port-forward … ${port}:5432`).
        #[arg(long)]
        pre_connect: Option<String>,
        #[arg(long)]
        test: bool,
        #[arg(long)]
        no_test: bool,
    },
    /// Replace a connection's password in the keychain
    SetSecret {
        #[arg(long)]
        name: String,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Connect, and say whether it worked
    Test {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the projects and connections, as `config export` writes them
    Show,
    /// Print the path of Dexo's settings file
    Path,
    /// Write the projects and connections to a TOML file, without passwords
    Export {
        #[arg(long)]
        output: PathBuf,
    },
    /// Read projects and connections from a TOML file
    Import {
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Create, list and change the profiles an agent connects through
    Profile {
        #[command(subcommand)]
        command: McpProfileCommand,
    },
    /// Allow or deny objects to a profile, by selector
    Allow {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        selector: String,
        #[arg(long)]
        deny: bool,
        #[arg(long)]
        remove: bool,
    },
    /// Print a profile's limits and the objects it allows
    Policy {
        #[arg(long)]
        profile: String,
    },
    /// List each profile's tools; --probe starts the server and asks it, then checks
    /// every client's config.
    Doctor {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        probe: bool,
    },
    /// Write Dexo's server into an agent's MCP config, merged with what is there.
    Setup {
        #[arg(long, value_parser = ["claude-code", "codex", "cursor", "claude-desktop", "gemini-cli", "windsurf", "vscode"])]
        client: String,
        #[arg(long)]
        profile: String,
        /// Print what would be written, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Also write a skill file telling the agent how Dexo behaves.
        #[arg(long)]
        skill: bool,
    },
    /// Print the config an MCP client needs to start Dexo's server
    Config {
        #[command(subcommand)]
        command: McpConfigCommand,
    },
    /// Serve a profile to an agent over stdio
    Serve {
        #[arg(long)]
        profile: String,
    },
    /// Let a profile write for a while, or take that back
    Grant {
        #[command(subcommand)]
        command: McpGrantCommand,
    },
    /// Print what agents asked for and what they got
    Audit {
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpProfileCommand {
    /// List the profiles
    List,
    /// Create a profile, disabled and read-only
    Create {
        #[arg(long)]
        name: String,
    },
    /// Delete a profile, with its grants
    Delete {
        #[arg(long)]
        name: String,
    },
    /// Print a profile
    Show {
        #[arg(long)]
        name: String,
    },
    /// Let agents use a profile
    Enable {
        #[arg(long)]
        name: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Stop agents from using a profile
    Disable {
        #[arg(long)]
        name: String,
    },
    /// Change a profile's connections, query mode and limits
    Set {
        #[arg(long)]
        name: String,
        #[arg(long = "connection")]
        connections: Vec<String>,
        #[arg(long)]
        clear_connections: bool,
        #[arg(long, value_parser = ["structured", "raw-read"])]
        query_mode: Option<String>,
        #[arg(long)]
        max_rows: Option<u64>,
        #[arg(long)]
        max_bytes: Option<u64>,
        #[arg(long)]
        timeout_secs: Option<u64>,
        #[arg(long)]
        max_concurrency: Option<u32>,
        #[arg(long = "allow-tool")]
        allow_tools: Vec<String>,
        #[arg(long = "deny-tool")]
        deny_tools: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpGrantCommand {
    /// Let a profile write to a connection until the grant expires
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
        /// Every write the grant covers waits for a person to approve it on Dexo's Agents
        /// screen, under Approvals; the grant lasts until it expires.
        #[arg(long)]
        ask: bool,
        /// Seconds a write waits for approval before it is refused, 1 to 3600.
        #[arg(
            long,
            default_value_t = 120,
            requires = "ask",
            value_parser = clap::value_parser!(u32).range(1..=3600)
        )]
        approval_timeout: u32,
    },
    /// List a profile's grants
    List {
        #[arg(long)]
        profile: String,
    },
    /// Take back one grant
    Revoke {
        #[arg(long)]
        id: String,
    },
    /// Take back every grant of a profile
    RevokeAll {
        #[arg(long)]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpConfigCommand {
    /// Print a client's MCP config for a profile
    Print {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        client: Option<String>,
    },
}
