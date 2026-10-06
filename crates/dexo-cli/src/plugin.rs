//! `dexo plugin …`: what the Plugins screen does, from the command line. Nothing here
//! prompts; what needs saying yes to takes a flag.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, bail};
use dexo_app::plugin::index::{self, IndexSource};
use dexo_app::plugin::manifest::{self, Manifest};
use dexo_app::plugin::state::{self, Origin};
use dexo_app::plugin::{install, plugins_dir};
use serde_json::json;

use crate::args::PluginCommand;

pub fn run(command: PluginCommand) -> anyhow::Result<()> {
    let data_dir = dexo_storage::AppPaths::discover()?.data_dir;
    match command {
        PluginCommand::List {
            available: true,
            json,
        } => available(&data_dir, json),
        PluginCommand::List {
            available: false,
            json,
        } => list(&data_dir, json),
        PluginCommand::Info { id, json } => info(&data_dir, &id, json),
        PluginCommand::Install {
            source,
            version,
            trust,
            non_interactive: _,
        } => install_source(&data_dir, &source, version.as_deref(), trust),
        PluginCommand::Update { id, all } => update(&data_dir, id.as_deref(), all),
        PluginCommand::Doctor { json } => doctor(&data_dir, json),
        PluginCommand::Remove { id, confirm } => {
            if !confirm {
                bail!("removing {id} deletes its files: run again with --confirm");
            }
            install::remove(&data_dir, &id, None)?;
            println!("Removed {id}.");
            Ok(())
        }
        PluginCommand::Enable { id } => {
            install::set_enabled(&data_dir, &id, true, None)?;
            println!("Enabled {id}.");
            Ok(())
        }
        PluginCommand::Disable { id } => {
            install::set_enabled(&data_dir, &id, false, None)?;
            println!("Disabled {id}: its drivers and themes are left out until it is enabled.");
            Ok(())
        }
    }
}

fn origin_word(origin: Origin) -> &'static str {
    match origin {
        Origin::Official => "official",
        Origin::Local => "local",
    }
}

fn list(data_dir: &Path, json: bool) -> anyhow::Result<()> {
    let state = state::load(&plugins_dir(data_dir));
    if json {
        let rows: Vec<_> = state
            .plugins
            .iter()
            .map(|(id, installed)| {
                json!({"id": id, "kind": installed.kind.as_str(), "version": installed.version,
                       "enabled": installed.enabled, "origin": origin_word(installed.origin)})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if state.plugins.is_empty() {
        println!("No plugins are installed.");
        return Ok(());
    }
    let width = state.plugins.keys().map(String::len).max().unwrap_or(0);
    for (id, installed) in &state.plugins {
        println!(
            "{id:<width$}  {:<6}  {:<10}  {:<8}  {}",
            installed.kind.as_str(),
            installed.version,
            if installed.enabled {
                "enabled"
            } else {
                "disabled"
            },
            origin_word(installed.origin)
        );
    }
    Ok(())
}

fn info(data_dir: &Path, id: &str, json: bool) -> anyhow::Result<()> {
    let plugins = plugins_dir(data_dir);
    let installed = state::load(&plugins).plugins.get(id).cloned();
    let Some(installed) = installed else {
        return info_available(data_dir, id, json);
    };
    let manifest = manifest::read(&plugins.join(id))
        .with_context(|| format!("the dexo-plugin.toml of {id} cannot be read"))?;
    let provides = manifest::provides(&manifest, &plugins.join(id));
    if json {
        let info = json!({
            "id": id, "name": manifest.name, "kind": manifest.kind.as_str(),
            "version": installed.version, "enabled": installed.enabled,
            "origin": origin_word(installed.origin), "authors": manifest.authors,
            "license": manifest.license, "homepage": manifest.homepage,
            "min_dexo": manifest.min_dexo, "provides": provides,
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    println!(
        "{} {} ({})",
        manifest.name,
        installed.version,
        manifest.kind.as_str()
    );
    println!(
        "  origin:    {}",
        match installed.origin {
            Origin::Official => "official, signed",
            Origin::Local => "local, not verified",
        }
    );
    println!(
        "  enabled:   {}",
        if installed.enabled { "yes" } else { "no" }
    );
    println!("  authors:   {}", manifest.authors.join(", "));
    println!("  license:   {}", manifest.license);
    println!("  needs:     Dexo {} or newer", manifest.min_dexo);
    println!("  provides:  {}", provides.join(", "));
    Ok(())
}

/// A plugin the index lists and this Dexo does not have.
fn info_available(data_dir: &Path, id: &str, json: bool) -> anyhow::Result<()> {
    let fetched = fresh_index(data_dir)?;
    let choice = index::choose(
        &fetched.index,
        id,
        None,
        env!("CARGO_PKG_VERSION"),
        index::target(),
    )
    .map_err(anyhow::Error::msg)?;
    if json {
        let info = json!({"id": id, "name": choice.plugin.name, "kind": choice.plugin.kind.as_str(),
            "version": choice.version.version, "installed": false, "origin": "official",
            "authors": choice.plugin.authors, "license": choice.plugin.license,
            "min_dexo": choice.version.min_dexo, "size": choice.artifact.size,
            "description": choice.plugin.description});
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    println!(
        "{} {} ({}, not installed)",
        choice.plugin.name,
        choice.version.version,
        choice.plugin.kind.as_str()
    );
    println!("  {}", choice.plugin.description);
    println!(
        "  download:  {} bytes for this platform",
        choice.artifact.size
    );
    println!("  needs:     Dexo {} or newer", choice.version.min_dexo);
    Ok(())
}

/// The index, fetched now: a command is asked for, so the answer is current. Offline,
/// the cached one, said so.
fn fresh_index(data_dir: &Path) -> anyhow::Result<index::Fetched> {
    let fetched = index::load(data_dir, &IndexSource::from_env(), true)?;
    if fetched.offline {
        eprintln!("{}", index::offline_note(fetched.fetched_at));
    }
    Ok(fetched)
}

fn available(data_dir: &Path, json: bool) -> anyhow::Result<()> {
    let fetched = fresh_index(data_dir)?;
    let installed = state::load(&plugins_dir(data_dir));
    let dexo = env!("CARGO_PKG_VERSION");
    let rows: Vec<_> = fetched
        .index
        .plugins
        .iter()
        .filter(|plugin| !installed.plugins.contains_key(&plugin.id))
        .filter_map(|plugin| {
            let choice =
                index::choose(&fetched.index, &plugin.id, None, dexo, index::target()).ok()?;
            Some(json!({"id": plugin.id, "kind": plugin.kind.as_str(),
                "version": choice.version.version, "size": choice.artifact.size,
                "description": plugin.description}))
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else if rows.is_empty() {
        println!("Every plugin this Dexo runs is installed.");
    } else {
        for row in &rows {
            println!(
                "{:<16}  {:<6}  {:<10}  {}",
                row["id"].as_str().unwrap_or_default(),
                row["kind"].as_str().unwrap_or_default(),
                row["version"].as_str().unwrap_or_default(),
                row["description"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

/// A download's progress on stderr, a line rewritten in place.
fn progress(name: &str) -> impl FnMut(u64, Option<u64>) + '_ {
    let mut shown = 0u64;
    move |done, total| {
        let percent = total.map_or(0, |total| done * 100 / total.max(1));
        if percent != shown || Some(done) == total {
            shown = percent;
            eprint!("\rDownloading {name}: {percent}%");
            if Some(done) == total {
                eprintln!();
            }
        }
    }
}

fn install_source(
    data_dir: &Path,
    source: &str,
    version: Option<&str>,
    trust: bool,
) -> anyhow::Result<()> {
    let path = Path::new(source);
    let never = AtomicBool::new(false);
    if path.exists() || source.starts_with("http://") || source.starts_with("https://") {
        let staged = if path.exists() {
            install::stage(data_dir, path)?
        } else {
            let bytes = dexo_app::plugin::http::get(
                source,
                install::DOWNLOAD_LIMIT,
                &mut progress(source),
                &never,
            )
            .map_err(anyhow::Error::msg)?;
            install::stage_bytes(data_dir, &bytes)?
        };
        trust_or_refuse(&staged.manifest, trust)?;
        let manifest = install::commit(data_dir, staged, Origin::Local, None)?;
        println!(
            "Installed {} {} (local, not verified).",
            manifest.name, manifest.version
        );
        return Ok(());
    }
    if !manifest::valid_id(source) {
        bail!("{source} is no file, folder, URL or plugin id");
    }
    fresh_index(data_dir)?;
    let manifest = install::install_from_index(
        data_dir,
        &IndexSource::from_env(),
        source,
        version,
        None,
        &mut progress(source),
        &never,
    )?;
    println!(
        "Installed {} {} (official, signed).",
        manifest.name, manifest.version
    );
    Ok(())
}

fn update(data_dir: &Path, id: Option<&str>, all: bool) -> anyhow::Result<()> {
    let ids: Vec<String> = match (id, all) {
        (Some(id), false) => vec![id.to_string()],
        (None, true) => state::load(&plugins_dir(data_dir))
            .plugins
            .into_iter()
            .filter(|(_, installed)| installed.origin == Origin::Official)
            .map(|(id, _)| id)
            .collect(),
        _ => bail!("name a plugin to update, or --all"),
    };
    fresh_index(data_dir)?;
    let never = AtomicBool::new(false);
    for id in ids {
        match install::upgrade(
            data_dir,
            &IndexSource::from_env(),
            &id,
            None,
            &mut progress(&id),
            &never,
        )? {
            Some(manifest) => println!("Updated {id} to {}.", manifest.version),
            None => println!("{id} is up to date."),
        }
    }
    Ok(())
}

fn doctor(data_dir: &Path, json: bool) -> anyhow::Result<()> {
    let checkups = install::doctor(data_dir);
    if json {
        println!("{}", serde_json::to_string_pretty(&checkups)?);
    } else if checkups.is_empty() {
        println!("No plugins are installed.");
    } else {
        for checkup in &checkups {
            if checkup.problems.is_empty() {
                println!("{} {}: ok", checkup.id, checkup.version);
            } else {
                for problem in &checkup.problems {
                    println!("{} {}: {problem}", checkup.id, checkup.version);
                }
            }
        }
    }
    if checkups.iter().any(|checkup| !checkup.problems.is_empty()) {
        bail!("a plugin has problems");
    }
    Ok(())
}

/// The plugins' line of `dexo doctor`.
pub fn summary(json: bool) -> anyhow::Result<()> {
    let data_dir = dexo_storage::AppPaths::discover()?.data_dir;
    let checkups = install::doctor(&data_dir);
    let problems = checkups
        .iter()
        .filter(|checkup| !checkup.problems.is_empty())
        .count();
    if json {
        println!(
            "{}",
            json!({"status": "ok", "plugins": {"installed": checkups.len(), "problems": problems}})
        );
    } else {
        println!("Dexo: ok");
        if problems > 0 {
            println!(
                "Plugins: {} installed, {problems} with problems (dexo plugin doctor)",
                checkups.len()
            );
        } else {
            println!("Plugins: {} installed", checkups.len());
        }
    }
    Ok(())
}

/// A plugin from outside the signed index installs only with `--trust`.
fn trust_or_refuse(manifest: &Manifest, trust: bool) -> anyhow::Result<()> {
    if trust {
        return Ok(());
    }
    eprintln!("{}", dexo_app::plugin::trust_text(manifest));
    bail!("run again with --trust to install it")
}
