//! `dexo plugin …`: what the Plugins screen does, from the command line. Nothing here
//! prompts; what needs saying yes to takes a flag.

use std::path::Path;

use anyhow::{Context, bail};
use dexo_app::plugin::manifest::{self, Manifest};
use dexo_app::plugin::state::{self, Origin};
use dexo_app::plugin::{install, plugins_dir};
use serde_json::json;

use crate::args::PluginCommand;

pub fn run(command: PluginCommand) -> anyhow::Result<()> {
    let data_dir = dexo_storage::AppPaths::discover()?.data_dir;
    match command {
        PluginCommand::List { json } => list(&data_dir, json),
        PluginCommand::Info { id, json } => info(&data_dir, &id, json),
        PluginCommand::Install {
            source,
            version: _,
            trust,
            non_interactive: _,
        } => install_source(&data_dir, &source, trust),
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
    let installed = state::load(&plugins)
        .plugins
        .get(id)
        .cloned()
        .with_context(|| format!("no plugin '{id}' is installed"))?;
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

fn install_source(data_dir: &Path, source: &str, trust: bool) -> anyhow::Result<()> {
    let path = Path::new(source);
    if !path.exists() {
        bail!("there is no file or folder at {source}");
    }
    let staged = install::stage(data_dir, path)?;
    trust_or_refuse(&staged.manifest, trust)?;
    let manifest = install::commit(data_dir, staged, Origin::Local, None)?;
    println!(
        "Installed {} {} (local, not verified).",
        manifest.name, manifest.version
    );
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
