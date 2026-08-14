use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use crate::args::{
    Args, Command, ConfigCommand, LaunchMode, McpCommand, McpConfigCommand, McpProfileCommand,
    OnError, OutputFormat, SchemaCommand, SchemaDiffFormat, SessionsCommand, TransferCliFormat,
};
use crate::presenter;
use dexo_app::mcp::{Effect, McpProfile, McpService, SelectorRule, advertised_tools};
use dexo_app::schema_diff::{RenameMapping, SchemaSnapshot, plan_migration, render_unquoted};
use dexo_app::search_service::SearchService;
use dexo_app::{
    AppError, CatalogService, DriverRegistry, ErrorCategory, ExecutionTarget, QueryService,
    ScriptPolicy, map_driver_error,
};
use dexo_driver_api::{
    CatalogListOptions, CatalogObject, CatalogReader, DbValue, QueryEvent, RowBatch,
};
use dexo_runtime::TaskRegistry;
use dexo_secrets::{KeyringSecretStore, SecretStore};
use dexo_storage::{
    AppPaths, CatalogCache, ConnectionRepository, Database, McpProfileRepository,
    SchemaSnapshotStore, export_portable, import_portable,
};

pub fn run(args: Args) -> anyhow::Result<()> {
    run_with(args, DriverRegistry::new())
}

pub trait TuiRunner {
    fn run(self) -> anyhow::Result<()>;
}

impl<F> TuiRunner for F
where
    F: FnOnce() -> anyhow::Result<()>,
{
    fn run(self) -> anyhow::Result<()> {
        self()
    }
}

pub fn run_with(args: Args, registry: DriverRegistry) -> anyhow::Result<()> {
    run_dispatch(args, registry, || {
        anyhow::bail!("TUI runner is required for interactive mode")
    })
}

pub fn run_dispatch(
    args: Args,
    registry: DriverRegistry,
    tui: impl TuiRunner,
) -> anyhow::Result<()> {
    match args.launch_mode() {
        LaunchMode::Tui => tui.run(),
        LaunchMode::Cli(command) => run_cli(command, registry),
    }
}

fn run_cli(command: Command, registry: DriverRegistry) -> anyhow::Result<()> {
    match command {
        Command::Doctor { json: true } => println!(r#"{{"status":"ok"}}"#),
        Command::Doctor { json: false } => println!("Dexo: ok"),
        Command::Config { command } => run_config(command)?,
        Command::Query {
            connection,
            sql,
            file,
            format,
            non_interactive,
            param,
            continue_on_error,
        } => run_query(
            registry,
            connection,
            sql,
            file,
            format,
            non_interactive,
            param,
            false,
            continue_on_error,
        )?,
        Command::Run {
            connection,
            file,
            format,
            non_interactive,
            param,
            continue_on_error,
        } => run_query(
            registry,
            connection,
            None,
            file,
            format,
            non_interactive,
            param,
            true,
            continue_on_error,
        )?,
        Command::Inspect {
            connection,
            object,
            search,
            snapshot,
            refresh,
            grants,
            format,
        } => run_inspect(
            registry, connection, object, search, snapshot, refresh, grants, format,
        )?,
        Command::Schema { command } => run_schema(registry, command)?,
        Command::Export {
            connection,
            sql,
            file,
            output,
            format,
        } => run_export(registry, connection, sql, file, output, format)?,
        Command::Import {
            connection,
            table,
            file,
            format,
            on_error,
            mapping,
            non_interactive,
        } => run_import(
            registry,
            connection,
            table,
            file,
            format,
            on_error,
            mapping,
            non_interactive,
        )?,
        Command::Explain {
            connection,
            sql,
            file,
            analyze,
            confirm,
            format,
        } => run_explain(registry, connection, sql, file, analyze, confirm, format)?,
        Command::Sessions { command } => run_sessions(registry, command)?,
        Command::Mcp { command } => run_mcp(registry, command)?,
    }
    Ok(())
}

fn to_transfer_format(format: TransferCliFormat) -> dexo_app::transfer::TransferFormat {
    match format {
        TransferCliFormat::Csv => dexo_app::transfer::TransferFormat::Csv,
        TransferCliFormat::Tsv => dexo_app::transfer::TransferFormat::Tsv,
        TransferCliFormat::Json => dexo_app::transfer::TransferFormat::Json,
        TransferCliFormat::Jsonl => dexo_app::transfer::TransferFormat::Jsonl,
        TransferCliFormat::Sql => dexo_app::transfer::TransferFormat::Sql,
    }
}

fn run_export(
    registry: DriverRegistry,
    connection: String,
    sql: Option<String>,
    file: Option<std::path::PathBuf>,
    output: std::path::PathBuf,
    format: TransferCliFormat,
) -> anyhow::Result<()> {
    let sql = load_sql(sql, file, false)?;
    let batches = tokio::runtime::Runtime::new()?.block_on(execute_script(
        registry,
        connection,
        sql,
        false,
        Vec::new(),
        ScriptPolicy::StopOnError,
    ))?;
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    for batch in batches {
        for event in batch? {
            match event {
                QueryEvent::Columns(meta) => {
                    columns = meta.into_iter().map(|column| column.name).collect();
                }
                QueryEvent::Rows(RowBatch { rows: chunk }) => rows.extend(chunk),
                _ => {}
            }
        }
    }
    let mut options = dexo_app::transfer::FormatOptions::default();
    if format == TransferCliFormat::Tsv {
        options.delimiter = b'\t';
    }
    // ponytail: CLI buffers query events from execute_script; million-row bound lives in export_rows.
    dexo_app::transfer::export_rows(
        &output,
        to_transfer_format(format),
        &options,
        &columns,
        rows,
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_import(
    registry: DriverRegistry,
    connection: String,
    table: String,
    file: Option<std::path::PathBuf>,
    format: TransferCliFormat,
    on_error: OnError,
    mapping: Vec<String>,
    non_interactive: bool,
) -> anyhow::Result<()> {
    let _ = non_interactive;
    let bytes = if let Some(path) = file {
        std::fs::read(path)?
    } else {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf)?;
        buf
    };
    let mut options = dexo_app::transfer::FormatOptions::default();
    if format == TransferCliFormat::Tsv {
        options.delimiter = b'\t';
    }
    let detected = dexo_app::transfer::detect(&bytes);
    let _ = detected;
    let (columns, decoded) =
        dexo_app::transfer::decode_document(to_transfer_format(format), &options, &bytes)
            .map_err(|error| anyhow::anyhow!(error))?;
    let mapped = if mapping.is_empty() {
        columns.clone()
    } else {
        mapping
            .into_iter()
            .map(|item| {
                item.split_once('=')
                    .map(|(_, target)| target.to_string())
                    .ok_or_else(|| anyhow::anyhow!("--mapping must be source=target"))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let strategy = match on_error {
        OnError::Stop => dexo_app::transfer::ErrorStrategy::Stop,
        OnError::Skip => dexo_app::transfer::ErrorStrategy::Skip,
        OnError::Reject => dexo_app::transfer::ErrorStrategy::RejectFile,
    };
    let rows: Vec<_> = decoded
        .into_iter()
        .enumerate()
        .map(|(index, values)| {
            let original = values.iter().map(|value| format!("{value:?}")).collect();
            (index + 2, values, original)
        })
        .collect();
    tokio::runtime::Runtime::new()?.block_on(import_live(
        registry, connection, table, mapped, rows, strategy,
    ))
}

async fn import_live(
    registry: DriverRegistry,
    connection: String,
    table: String,
    columns: Vec<String>,
    rows: Vec<(usize, Vec<dexo_driver_api::DbValue>, Vec<String>)>,
    strategy: dexo_app::transfer::ErrorStrategy,
) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let session = connect_session(&registry, &profile).await?;
    let writer = session
        .bulk()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "bulk import is unavailable"))?;
    let target = dexo_app::parse_qualified(&table);
    let report = dexo_app::transfer::import_rows(
        writer,
        &target,
        &columns,
        rows,
        strategy,
        &std::sync::atomic::AtomicBool::new(false),
        None,
        |_| {},
    )
    .await
    .map_err(|error| anyhow::anyhow!(error))?;
    println!("committed={} skipped={}", report.committed, report.skipped);
    Ok(())
}

fn run_config(command: ConfigCommand) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    match command {
        ConfigCommand::Export { output } => {
            std::fs::write(output, export_portable(db.connection())?)?;
        }
        ConfigCommand::Import { input } => {
            let toml_text = std::fs::read_to_string(input)?;
            let report = import_portable(db.connection(), &toml_text)?;
            if report.connections_needing_secret.is_empty() {
                println!("Imported 0 connection(s).");
            } else {
                println!(
                    "Imported {} connection(s). Secrets required for: {}",
                    report.connections_needing_secret.len(),
                    report.connections_needing_secret.join(", ")
                );
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_inspect(
    registry: DriverRegistry,
    connection: String,
    object: Option<String>,
    search: Option<String>,
    snapshot: Option<String>,
    refresh: bool,
    grants: bool,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let cache = CatalogCache::new(db.connection());
    let database_name = profile
        .config
        .get("database")
        .or_else(|| profile.config.get("dbname"))
        .and_then(|value| value.as_str())
        .unwrap_or("default");
    if refresh {
        let objects =
            tokio::runtime::Runtime::new()?.block_on(refresh_catalog(&registry, &profile))?;
        cache.replace_snapshot(&profile.id.0.to_string(), database_name, &objects)?;
    }
    let cached = cache.load_latest(&profile.id.0.to_string(), database_name)?;
    let use_snapshot = snapshot.as_deref() == Some("latest")
        || (!cached.is_empty() && object.is_none() && search.is_some());
    let payload = if grants {
        tokio::runtime::Runtime::new()?.block_on(inspect_grants(
            &registry,
            &profile,
            object.as_deref(),
        ))?
    } else if let Some(query) = search {
        let objects = if cached.is_empty() && !use_snapshot {
            tokio::runtime::Runtime::new()?.block_on(refresh_catalog(&registry, &profile))?
        } else {
            cached
        };
        let hits = SearchService::from_objects(objects).search(&query);
        serde_json::to_value(hits.iter().map(|hit| &hit.object).collect::<Vec<_>>())?
    } else if let Some(name) = object {
        if snapshot.as_deref() == Some("latest") || refresh {
            cached
                .into_iter()
                .find(|item| {
                    item.qualified_name.display_unquoted() == name
                        || item.qualified_name.object() == name
                })
                .map(serde_json::to_value)
                .transpose()?
                .ok_or_else(|| AppError::new(ErrorCategory::Configuration, "object not found"))?
        } else {
            tokio::runtime::Runtime::new()?.block_on(inspect_live(&registry, &profile, &name))?
        }
    } else if snapshot.as_deref() == Some("latest") {
        serde_json::to_value(&cached)?
    } else {
        anyhow::bail!("provide --object, --search, --grants, or --snapshot latest");
    };
    let mut stdout = std::io::stdout();
    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            serde_json::to_writer(&mut stdout, &payload)?;
            writeln!(stdout)?;
        }
        _ => {
            serde_json::to_writer(&mut stdout, &payload)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

pub fn schema_apply_guard(apply: bool, confirm_target: Option<&str>) -> anyhow::Result<()> {
    if apply && confirm_target.is_none() {
        anyhow::bail!("CLI never applies unless --apply --confirm-target is supplied");
    }
    Ok(())
}

fn run_schema(registry: DriverRegistry, command: SchemaCommand) -> anyhow::Result<()> {
    match command {
        SchemaCommand::Snapshot {
            connection,
            name,
            output,
        } => run_schema_snapshot(registry, connection, name, output),
        SchemaCommand::Diff {
            from,
            to,
            format,
            apply,
            confirm_target,
            rename,
            connection,
        } => run_schema_diff(
            registry,
            from,
            to,
            format,
            apply,
            confirm_target,
            rename,
            connection,
        ),
    }
}

fn run_schema_snapshot(
    registry: DriverRegistry,
    connection: String,
    name: String,
    output: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let cache = CatalogCache::new(db.connection());
    let database_name = profile
        .config
        .get("database")
        .or_else(|| profile.config.get("dbname"))
        .and_then(|value| value.as_str())
        .unwrap_or("default");
    let objects = {
        let cached = cache.load_latest(&profile.id.0.to_string(), database_name)?;
        if cached.is_empty() {
            tokio::runtime::Runtime::new()?.block_on(refresh_catalog(&registry, &profile))?
        } else {
            cached
        }
    };
    let snapshot = SchemaSnapshot::capture(
        profile.driver.clone(),
        String::new(),
        chrono_now(),
        database_name.to_string(),
        objects,
    );
    SchemaSnapshotStore::new(db.connection()).save(&name, &snapshot)?;
    if let Some(path) = output {
        std::fs::write(path, serde_json::to_string_pretty(&snapshot)?)?;
    }
    println!("{}", snapshot.digest);
    Ok(())
}

fn chrono_now() -> String {
    // ponytail: RFC3339 via system clock is enough for snapshot labels; inject a clock if tests need freeze.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "0".into())
}

fn parse_renames(raw: Vec<String>) -> anyhow::Result<Vec<RenameMapping>> {
    raw.into_iter()
        .map(|item| {
            item.split_once('=')
                .map(|(from, to)| RenameMapping {
                    from: from.to_string(),
                    to: to.to_string(),
                })
                .ok_or_else(|| anyhow::anyhow!("--rename must be from=to"))
        })
        .collect()
}

fn load_named_snapshot(
    store: &SchemaSnapshotStore<'_>,
    name_or_path: &str,
) -> anyhow::Result<SchemaSnapshot> {
    if let Some(snapshot) = store.load_by_name(name_or_path)? {
        return Ok(snapshot);
    }
    if std::path::Path::new(name_or_path).is_file() {
        let json = std::fs::read_to_string(name_or_path)?;
        return SchemaSnapshotStore::load_json(&json).map_err(anyhow::Error::from);
    }
    anyhow::bail!("snapshot '{name_or_path}' not found")
}

#[allow(clippy::too_many_arguments)]
fn run_schema_diff(
    registry: DriverRegistry,
    from: String,
    to: String,
    format: SchemaDiffFormat,
    apply: bool,
    confirm_target: Option<String>,
    rename: Vec<String>,
    connection: Option<String>,
) -> anyhow::Result<()> {
    schema_apply_guard(apply, confirm_target.as_deref())?;
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let store = SchemaSnapshotStore::new(db.connection());
    let from_snap = load_named_snapshot(&store, &from)?;
    let to_snap = load_named_snapshot(&store, &to)?;
    let renames = parse_renames(rename)?;
    let (changes, _ordered, script) =
        plan_migration(&from_snap, &to_snap, &renames, render_unquoted);
    match format {
        SchemaDiffFormat::Json => {
            serde_json::to_writer(std::io::stdout(), &changes)?;
            println!();
        }
        SchemaDiffFormat::Sql => {
            print!("{}", script.forward);
        }
    }
    if apply {
        let target = confirm_target.expect("guarded");
        if target != to_snap.scope && Some(target.as_str()) != connection.as_deref() {
            anyhow::bail!("confirm-target does not match snapshot scope or connection");
        }
        let connection =
            connection.ok_or_else(|| anyhow::anyhow!("--connection is required with --apply"))?;
        let batches = tokio::runtime::Runtime::new()?.block_on(execute_script(
            registry,
            connection,
            script.forward.clone(),
            true,
            Vec::new(),
            ScriptPolicy::StopOnError,
        ))?;
        for batch in batches {
            batch?;
        }
    }
    Ok(())
}

async fn inspect_live(
    registry: &DriverRegistry,
    profile: &dexo_app::ConnectionProfile,
    qualified: &str,
) -> anyhow::Result<serde_json::Value> {
    let session = connect_session(registry, profile).await?;
    let reader = session
        .catalog()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "catalog is unavailable"))?;
    let found =
        CatalogService::find_by_qualified_name(reader, qualified, &CatalogListOptions::default())
            .await?;
    found
        .map(|object| serde_json::to_value(object).map_err(anyhow::Error::from))
        .transpose()?
        .ok_or_else(|| AppError::new(ErrorCategory::Configuration, "object not found").into())
}

async fn inspect_grants(
    registry: &DriverRegistry,
    profile: &dexo_app::ConnectionProfile,
    principal: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let session = connect_session(registry, profile).await?;
    let admin = session
        .security()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "security admin is unavailable"))?;
    let name = principal.map(dexo_app::parse_qualified);
    let grants = admin
        .list_grants(name.as_ref())
        .await
        .map_err(map_driver_error)?;
    serde_json::to_value(grants).map_err(anyhow::Error::from)
}

async fn refresh_catalog(
    registry: &DriverRegistry,
    profile: &dexo_app::ConnectionProfile,
) -> anyhow::Result<Vec<CatalogObject>> {
    let session = connect_session(registry, profile).await?;
    let reader = session
        .catalog()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "catalog is unavailable"))?;
    collect_snapshot(reader, None).await
}

async fn collect_snapshot(
    reader: &dyn CatalogReader,
    parent: Option<&dexo_driver_api::ObjectId>,
) -> anyhow::Result<Vec<CatalogObject>> {
    let page =
        CatalogService::list_children(reader, parent, &CatalogListOptions::default()).await?;
    let mut objects = page.objects;
    let children = objects.clone();
    for child in children {
        if matches!(
            child.kind,
            dexo_driver_api::ObjectKind::Catalog
                | dexo_driver_api::ObjectKind::Schema
                | dexo_driver_api::ObjectKind::Table
                | dexo_driver_api::ObjectKind::View
                | dexo_driver_api::ObjectKind::MaterializedView
        ) {
            objects.extend(Box::pin(collect_snapshot(reader, Some(&child.id))).await?);
        }
    }
    Ok(objects)
}

async fn connect_session(
    registry: &DriverRegistry,
    profile: &dexo_app::ConnectionProfile,
) -> anyhow::Result<Box<dyn dexo_driver_api::Session>> {
    let secret = KeyringSecretStore
        .get(profile.secret_ref.as_str())?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Authentication,
                "secret is missing for this connection",
            )
        })?;
    let factory = registry.get(&profile.driver)?;
    let (connect, _) = profile.connect_request(secret)?;
    Ok(factory.connect(connect).await.map_err(map_driver_error)?)
}

#[allow(clippy::too_many_arguments)]
fn run_query(
    registry: DriverRegistry,
    connection: String,
    sql: Option<String>,
    file: Option<std::path::PathBuf>,
    format: OutputFormat,
    non_interactive: bool,
    param: Vec<String>,
    from_run: bool,
    continue_on_error: bool,
) -> anyhow::Result<()> {
    let sql = load_sql(sql, file, from_run)?;
    if sql.trim().is_empty() {
        anyhow::bail!("SQL is required");
    }
    let mutating = looks_mutating(&sql);
    if mutating && non_interactive {
        return Err(AppError::new(
            ErrorCategory::Permission,
            "non-interactive mode cannot confirm a mutating statement",
        )
        .into());
    }
    let parameters = parse_params(param)?;
    let policy = if continue_on_error {
        ScriptPolicy::ContinueOnError
    } else {
        ScriptPolicy::StopOnError
    };
    let batches = tokio::runtime::Runtime::new()?.block_on(execute_script(
        registry, connection, sql, mutating, parameters, policy,
    ))?;
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut first_error = None;
    for (index, batch) in batches.into_iter().enumerate() {
        match batch {
            Ok(events) => present_events(format, &events, &mut stdout, &mut stderr)?,
            Err(error) => {
                writeln!(stderr, "statement {}: {error}", index + 1)?;
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error
        && policy == ScriptPolicy::StopOnError
    {
        return Err(error.into());
    }
    Ok(())
}

async fn execute_script(
    registry: DriverRegistry,
    connection: String,
    sql: String,
    mutating: bool,
    parameters: Vec<DbValue>,
    policy: ScriptPolicy,
) -> anyhow::Result<Vec<Result<Vec<QueryEvent>, AppError>>> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let secret = KeyringSecretStore
        .get(profile.secret_ref.as_str())?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Authentication,
                "secret is missing for this connection",
            )
        })?;
    let factory = registry.get(&profile.driver)?;
    let (connect, conn_policy) = profile.connect_request(secret)?;
    let session = factory.connect(connect).await.map_err(map_driver_error)?;
    let service = QueryService::new(Arc::new(TaskRegistry::default()));
    let batches = service
        .execute_script(
            Arc::from(session),
            &sql,
            ExecutionTarget::Document,
            0,
            None,
            policy,
            conn_policy.max_rows,
            mutating,
            parameters,
            Duration::from_secs(conn_policy.timeout_secs),
        )
        .await;
    Ok(batches)
}

fn run_explain(
    registry: DriverRegistry,
    connection: String,
    sql: Option<String>,
    file: Option<std::path::PathBuf>,
    analyze: bool,
    confirm: bool,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let sql = load_sql(sql, file, false)?;
    if analyze && !confirm {
        anyhow::bail!("EXPLAIN ANALYZE executes the statement; pass --confirm");
    }
    let plan = tokio::runtime::Runtime::new()?
        .block_on(explain_live(registry, connection, sql, analyze))?;
    let mut stdout = std::io::stdout();
    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            serde_json::to_writer(&mut stdout, &plan)?;
            writeln!(stdout)?;
        }
        _ => {
            writeln!(stdout, "{}", dexo_app::explain_service::render_tree(&plan))?;
        }
    }
    Ok(())
}

async fn explain_live(
    registry: DriverRegistry,
    connection: String,
    sql: String,
    analyze: bool,
) -> anyhow::Result<dexo_driver_api::ExplainPlan> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let session = connect_session(&registry, &profile).await?;
    let provider = session
        .explain()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "explain is unavailable"))?;
    Ok(provider
        .explain(dexo_driver_api::ExplainRequest { sql, analyze })
        .await
        .map_err(map_driver_error)?)
}

fn run_sessions(registry: DriverRegistry, command: SessionsCommand) -> anyhow::Result<()> {
    match command {
        SessionsCommand::List { connection, format } => {
            let list =
                tokio::runtime::Runtime::new()?.block_on(admin_list(registry, connection))?;
            writeln!(
                std::io::stderr(),
                "captured_at={} restriction={}",
                list.captured_at,
                list.restriction.as_deref().unwrap_or("-")
            )?;
            match format {
                OutputFormat::Json | OutputFormat::Jsonl => {
                    serde_json::to_writer(std::io::stdout(), &list.items)?;
                    println!();
                }
                _ => {
                    for session in list.items {
                        println!(
                            "{}\t{}\t{}\t{}\t{}",
                            session.id,
                            session.user.unwrap_or_else(|| "-".into()),
                            session.database.unwrap_or_else(|| "-".into()),
                            session.state,
                            session.current_query.unwrap_or_else(|| "-".into())
                        );
                    }
                }
            }
        }
        SessionsCommand::Cancel {
            connection,
            session,
            confirm,
        } => {
            if !confirm {
                anyhow::bail!("cancel requires --confirm");
            }
            let outcome = tokio::runtime::Runtime::new()?.block_on(admin_action(
                registry,
                connection,
                dexo_driver_api::AdminAction::CancelQuery {
                    session_id: session,
                },
            ))?;
            println!(
                "ok={} noop={} {}",
                outcome.ok, outcome.idempotent_noop, outcome.message
            );
        }
        SessionsCommand::Terminate {
            connection,
            session,
            confirm_target,
        } => {
            let target = confirm_target.ok_or_else(|| {
                anyhow::anyhow!("terminate requires --confirm-target <session id>")
            })?;
            if target != session {
                anyhow::bail!("confirm-target does not match session id");
            }
            let outcome = tokio::runtime::Runtime::new()?.block_on(admin_action(
                registry,
                connection,
                dexo_driver_api::AdminAction::TerminateSession {
                    session_id: session,
                },
            ))?;
            println!(
                "ok={} noop={} {}",
                outcome.ok, outcome.idempotent_noop, outcome.message
            );
        }
    }
    Ok(())
}

async fn admin_list(
    registry: DriverRegistry,
    connection: String,
) -> anyhow::Result<dexo_driver_api::AdminList<dexo_driver_api::SessionInfo>> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let session = connect_session(&registry, &profile).await?;
    let admin = session
        .admin()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "admin is unavailable"))?;
    Ok(admin.list_sessions().await.map_err(map_driver_error)?)
}

async fn admin_action(
    registry: DriverRegistry,
    connection: String,
    action: dexo_driver_api::AdminAction,
) -> anyhow::Result<dexo_driver_api::AdminOutcome> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = ConnectionRepository::new(db.connection())
        .get_by_name(&connection)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("unknown connection '{connection}'"),
            )
        })?;
    let session = connect_session(&registry, &profile).await?;
    let admin = session
        .admin()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "admin is unavailable"))?;
    let preview = admin.preview(&action).map_err(map_driver_error)?;
    writeln!(
        std::io::stderr(),
        "command={} lock={:?}",
        preview.command,
        preview.lock_risk
    )?;
    Ok(admin
        .execute_action(action)
        .await
        .map_err(map_driver_error)?)
}

fn parse_params(param: Vec<String>) -> anyhow::Result<Vec<DbValue>> {
    param
        .into_iter()
        .map(|item| {
            item.split_once('=')
                .map(|(_, value)| DbValue::Text(value.to_string()))
                .ok_or_else(|| anyhow::anyhow!("--param must be name=value"))
        })
        .collect()
}

fn load_sql(
    sql: Option<String>,
    file: Option<std::path::PathBuf>,
    from_run: bool,
) -> anyhow::Result<String> {
    match (sql, file, from_run) {
        (Some(_), Some(_), _) => anyhow::bail!("--sql and --file are mutually exclusive"),
        (Some(sql), None, false) => Ok(sql),
        (None, Some(path), _) => Ok(std::fs::read_to_string(path)?),
        (None, None, true) => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
            Ok(buf)
        }
        (Some(_), None, true) => anyhow::bail!("run reads a file or stdin, not --sql"),
        (None, None, false) => anyhow::bail!("provide --sql or --file"),
    }
}

fn looks_mutating(sql: &str) -> bool {
    let trimmed = sql.trim_start().to_ascii_lowercase();
    let explain_analyze = trimmed.starts_with("explain") && trimmed.contains("analyze");
    trimmed.starts_with("insert")
        || trimmed.starts_with("update")
        || trimmed.starts_with("delete")
        || trimmed.starts_with("drop")
        || trimmed.starts_with("truncate")
        || trimmed.starts_with("alter")
        || explain_analyze
}

pub fn present_events(
    format: OutputFormat,
    events: &[QueryEvent],
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    presenter::present(to_presenter_format(format), events, stdout, stderr)
}

fn to_presenter_format(format: OutputFormat) -> presenter::OutputFormat {
    match format {
        OutputFormat::Table => presenter::OutputFormat::Table,
        OutputFormat::Csv => presenter::OutputFormat::Csv,
        OutputFormat::Tsv => presenter::OutputFormat::Tsv,
        OutputFormat::Json => presenter::OutputFormat::Json,
        OutputFormat::Jsonl => presenter::OutputFormat::Jsonl,
    }
}

pub fn sample_select_one() -> Vec<QueryEvent> {
    vec![
        QueryEvent::Columns(vec![dexo_driver_api::ColumnMeta {
            name: "n".into(),
            type_name: "int4".into(),
            nullable: false,
        }]),
        QueryEvent::Rows(RowBatch {
            rows: vec![vec![DbValue::I64(1)]],
        }),
        QueryEvent::Finished {
            rows_affected: Some(1),
        },
    ]
}

fn run_mcp(registry: DriverRegistry, command: McpCommand) -> anyhow::Result<()> {
    match command {
        McpCommand::Profile { command } => run_mcp_profile(command)?,
        McpCommand::Allow {
            profile,
            selector,
            deny,
        } => mcp_allow(&profile, &selector, deny)?,
        McpCommand::Policy { profile } => mcp_policy(&profile)?,
        McpCommand::Doctor { profile, json } => mcp_doctor(profile.as_deref(), json)?,
        McpCommand::Config { command } => match command {
            McpConfigCommand::Print { profile, client } => {
                mcp_config_print(&profile, client.as_deref())?
            }
        },
        McpCommand::Serve { profile } => {
            tokio::runtime::Runtime::new()?.block_on(mcp_serve(registry, profile))?;
        }
    }
    Ok(())
}

fn run_mcp_profile(command: McpProfileCommand) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let repo = McpProfileRepository::new(db.connection());
    match command {
        McpProfileCommand::List => {
            for profile in repo.list()? {
                println!(
                    "{} enabled={} access=read_only",
                    profile.name, profile.enabled
                );
            }
        }
        McpProfileCommand::Create { name } => {
            let profile = McpProfile::new(&name);
            repo.save(&profile)?;
            println!("created {name} enabled=false access=read_only");
        }
        McpProfileCommand::Show { name } => mcp_policy(&name)?,
        McpProfileCommand::Enable { name, confirm } => {
            let mut profile = load_profile(&repo, &name)?;
            println!("scopes:");
            for rule in &profile.selectors {
                println!("  {:?}", rule.effect);
            }
            println!("tools: {}", advertised_tools(&profile).join(", "));
            if !confirm {
                anyhow::bail!("pass --confirm to enable profile '{name}'");
            }
            profile.enabled = true;
            repo.save(&profile)?;
            println!("enabled {name}");
        }
        McpProfileCommand::Disable { name } => {
            let mut profile = load_profile(&repo, &name)?;
            profile.enabled = false;
            repo.save(&profile)?;
            println!("disabled {name}");
        }
    }
    Ok(())
}

fn mcp_allow(name: &str, selector: &str, deny: bool) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let repo = McpProfileRepository::new(db.connection());
    let mut profile = load_profile(&repo, name)?;
    let effect = if deny { Effect::Deny } else { Effect::Allow };
    profile
        .selectors
        .push(SelectorRule::parse(effect, selector)?);
    repo.save(&profile)?;
    println!("{} {selector}", if deny { "deny" } else { "allow" });
    Ok(())
}

fn mcp_policy(name: &str) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = load_profile(&McpProfileRepository::new(db.connection()), name)?;
    println!(
        "name={} enabled={} access=read_only query_mode={:?} max_rows={} max_bytes={} timeout_secs={} max_concurrency={}",
        profile.name,
        profile.enabled,
        profile.query_mode,
        profile.limits.max_rows,
        profile.limits.max_bytes,
        profile.limits.timeout_secs,
        profile.limits.max_concurrency
    );
    for rule in &profile.selectors {
        println!("selector {:?}", rule.effect);
    }
    println!("tools: {}", advertised_tools(&profile).join(", "));
    Ok(())
}

fn mcp_doctor(name: Option<&str>, json: bool) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let repo = McpProfileRepository::new(db.connection());
    let profiles = if let Some(name) = name {
        vec![load_profile(&repo, name)?]
    } else {
        repo.list()?
    };
    if json {
        println!(
            "{}",
            serde_json::json!({
                "profiles": profiles.iter().map(|p| serde_json::json!({
                    "name": p.name,
                    "enabled": p.enabled,
                    "access": "read_only",
                    "tools": advertised_tools(p),
                })).collect::<Vec<_>>()
            })
        );
    } else {
        for profile in profiles {
            println!(
                "{} enabled={} tools={}",
                profile.name,
                profile.enabled,
                advertised_tools(&profile).join(",")
            );
        }
    }
    Ok(())
}

fn mcp_config_print(name: &str, client: Option<&str>) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    load_profile(&McpProfileRepository::new(db.connection()), name)?;
    let snippet = match client.unwrap_or("cursor") {
        "claude" => format!(
            "{{\n  \"mcpServers\": {{\n    \"dexo\": {{\n      \"command\": \"dexo\",\n      \"args\": [\"mcp\", \"serve\", \"--profile\", \"{name}\"]\n    }}\n  }}\n}}"
        ),
        _ => format!(
            "{{\n  \"mcpServers\": {{\n    \"dexo\": {{\n      \"command\": \"dexo\",\n      \"args\": [\"mcp\", \"serve\", \"--profile\", \"{name}\"]\n    }}\n  }}\n}}"
        ),
    };
    println!("{snippet}");
    Ok(())
}

fn load_profile(repo: &McpProfileRepository<'_>, name: &str) -> anyhow::Result<McpProfile> {
    repo.get_by_name(name)?.ok_or_else(|| {
        anyhow::anyhow!(AppError::new(
            ErrorCategory::Configuration,
            format!("unknown MCP profile '{name}'")
        ))
    })
}

async fn mcp_serve(registry: DriverRegistry, name: String) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = load_profile(&McpProfileRepository::new(db.connection()), &name)?;
    if !profile.enabled {
        anyhow::bail!("profile '{name}' is disabled");
    }
    let mut objects = CatalogCache::new(db.connection()).load_latest_any()?;
    if objects.is_empty() {
        for connection in &profile.connections {
            if let Some(conn) =
                ConnectionRepository::new(db.connection()).get_by_name(connection)?
            {
                objects = CatalogCache::new(db.connection())
                    .load_latest(&conn.id.0.to_string(), "")
                    .unwrap_or_default();
            }
        }
    }
    let service = McpService::new(profile, objects);
    let session = if let Some(connection) = service.profile.connections.first() {
        match ConnectionRepository::new(db.connection()).get_by_name(connection)? {
            Some(conn) => connect_session(&registry, &conn)
                .await
                .ok()
                .map(std::sync::Arc::from),
            None => None,
        }
    } else {
        None
    };
    dexo_mcp::serve_with_session(service, session).await
}
