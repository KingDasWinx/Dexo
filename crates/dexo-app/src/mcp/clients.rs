//! The agents Dexo's MCP server is set up for -- Claude Code, Codex, Cursor and Claude
//! Desktop -- each with its config file, the one `dexo` entry merged into it, and the
//! skill file that tells the agent how Dexo behaves.

use std::path::{Path, PathBuf};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpClient {
    ClaudeCode,
    Codex,
    Cursor,
    ClaudeDesktop,
}

/// Where the user's files are: the project (the current directory) and the home and
/// config directories.
#[derive(Clone, Debug)]
pub struct Places {
    pub project: PathBuf,
    pub home: PathBuf,
    /// The platform's per-user config directory: `~/Library/Application Support`,
    /// `%APPDATA%`, `$XDG_CONFIG_HOME` or `~/.config`.
    pub config: PathBuf,
}

impl Places {
    /// From the environment, as each client looks for its files.
    pub fn discover() -> Result<Self, AppError> {
        let missing = || {
            AppError::new(
                ErrorCategory::Configuration,
                "cannot tell the home directory; set HOME",
            )
        };
        let project = std::env::current_dir()
            .map_err(|error| AppError::new(ErrorCategory::Configuration, error.to_string()))?;
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or_else(missing)?;
        let config = if cfg!(target_os = "macos") {
            home.join("Library/Application Support")
        } else if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Roaming"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".config"))
        };
        Ok(Self {
            project,
            home,
            config,
        })
    }
}

impl McpClient {
    pub const ALL: [McpClient; 4] = [
        McpClient::ClaudeCode,
        McpClient::Codex,
        McpClient::Cursor,
        McpClient::ClaudeDesktop,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::ClaudeDesktop => "claude-desktop",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|client| client.id() == id)
    }

    /// The file the client reads its MCP servers from.
    pub fn config_path(self, places: &Places) -> PathBuf {
        match self {
            Self::ClaudeCode => places.project.join(".mcp.json"),
            Self::Codex => places.home.join(".codex/config.toml"),
            Self::Cursor => places.home.join(".cursor/mcp.json"),
            Self::ClaudeDesktop => places.config.join("Claude/claude_desktop_config.json"),
        }
    }

    /// Where the agent finds a skill, or rule, file; Claude Desktop has no such place.
    pub fn skill_path(self, places: &Places) -> Option<PathBuf> {
        match self {
            Self::ClaudeCode => Some(places.project.join(".claude/skills/dexo/SKILL.md")),
            Self::Codex => Some(places.home.join(".codex/skills/dexo/SKILL.md")),
            Self::Cursor => Some(places.project.join(".cursor/rules/dexo.mdc")),
            Self::ClaudeDesktop => None,
        }
    }

    /// `existing` with Dexo's server entry in it, every other key left as it was. A file
    /// that does not parse is an error, and is not rewritten.
    pub fn merged(
        self,
        existing: Option<&str>,
        command: &str,
        args: &[String],
    ) -> Result<String, AppError> {
        match self {
            Self::Codex => Ok(merged_toml(existing.unwrap_or(""), command, args)),
            _ => merged_json(existing, command, args),
        }
    }

    /// Whether the file has a `dexo` entry, and the command it runs.
    pub fn configured_command(self, contents: &str) -> Option<String> {
        match self {
            Self::Codex => {
                let table: toml::Table = contents.parse().ok()?;
                table
                    .get("mcp_servers")?
                    .get("dexo")?
                    .get("command")?
                    .as_str()
                    .map(str::to_string)
            }
            _ => {
                let value: serde_json::Value = serde_json::from_str(contents).ok()?;
                value
                    .pointer("/mcpServers/dexo/command")?
                    .as_str()
                    .map(str::to_string)
            }
        }
    }
}

fn merged_json(existing: Option<&str>, command: &str, args: &[String]) -> Result<String, AppError> {
    let mut root = match existing.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => serde_json::from_str::<serde_json::Value>(text).map_err(|error| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("it is not JSON Dexo can read ({error}), so it was left as it is"),
            )
        })?,
        None => serde_json::json!({}),
    };
    let Some(object) = root.as_object_mut() else {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "it is not a JSON object, so it was left as it is",
        ));
    };
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "its mcpServers is not an object, so it was left as it is",
        ));
    };
    servers.insert(
        "dexo".into(),
        serde_json::json!({ "command": command, "args": args }),
    );
    let mut text = serde_json::to_string_pretty(&root)
        .map_err(|error| AppError::new(ErrorCategory::Internal, error.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// Codex's TOML is edited as text: the `[mcp_servers.dexo]` table is replaced, or added
/// at the end, so the user's comments and layout stay.
fn merged_toml(existing: &str, command: &str, args: &[String]) -> String {
    let quote = |text: &str| toml::Value::String(text.to_string()).to_string();
    let section = format!(
        "[mcp_servers.dexo]\ncommand = {}\nargs = [{}]\n",
        quote(command),
        args.iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let lines: Vec<&str> = existing.lines().collect();
    let header = |line: &str| line.trim_start().starts_with('[');
    let start = lines
        .iter()
        .position(|line| line.trim() == "[mcp_servers.dexo]");
    let mut out = String::new();
    match start {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|line| header(line))
                .map_or(lines.len(), |offset| start + 1 + offset);
            for line in &lines[..start] {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str(&section);
            if end < lines.len() {
                out.push('\n');
            }
            for line in &lines[end..] {
                out.push_str(line);
                out.push('\n');
            }
        }
        None => {
            out.push_str(existing.trim_end());
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&section);
        }
    }
    out
}

/// Writes `contents` to `path`, the old file copied to `<file>.dexo-backup` first.
pub fn write_with_backup(path: &Path, contents: &str) -> Result<Option<PathBuf>, AppError> {
    let storage = |error: std::io::Error| AppError::new(ErrorCategory::Storage, error.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(storage)?;
    }
    let backup = if path.exists() {
        let mut name = path.as_os_str().to_owned();
        name.push(".dexo-backup");
        let backup = PathBuf::from(name);
        std::fs::copy(path, &backup).map_err(storage)?;
        Some(backup)
    } else {
        None
    };
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".dexo-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, contents).map_err(storage)?;
    std::fs::rename(&tmp, path).map_err(storage)?;
    Ok(backup)
}

/// What an agent should know before it uses Dexo's tools.
pub fn skill_text(client: McpClient, profile: &str) -> String {
    let front = match client {
        McpClient::Cursor => "---\ndescription: Using Dexo's MCP tools on the user's databases\nalwaysApply: false\n---\n".to_string(),
        _ => "---\nname: dexo\ndescription: How to use Dexo's MCP tools on the user's databases -- read-only by default, writes only through grants a person makes.\n---\n".to_string(),
    };
    format!(
        "{front}
# Using Dexo

Dexo's MCP server (`dexo mcp serve --profile {profile}`) gives you the user's databases
through the connections the profile `{profile}` allows, and nothing else.

## Reading

- `list_connections` names the connections you may use; pass `connection` when there is
  more than one.
- `catalog_search` and `catalog_list` find tables and columns; `object_describe` gives a
  table's columns, keys and the notes people wrote about it -- read the notes, they say
  what the data means. `object_get_ddl` and `object_relationships` give the rest.
- `query_validate` checks a statement first; `query_execute_read` runs reads only, with a
  row limit; `query_explain` shows a plan, and can try hypothetical indexes on Postgres.
- `data_read` pages through a table.

## Writing

Access is read-only. A write tool appears only while a person has granted it with
`dexo mcp grant create`, for a connection and a set of tables, for a limited time.
Some grants ask: then each write waits until the person approves it in Dexo's Agent
Activity screen, or it is denied after two minutes -- say what you are about to change
and why before you call the tool. Every call is audited. Never retry a denied write with
a different statement to get around the decision; ask the person instead.
"
    )
}

#[cfg(test)]
mod tests {
    use super::McpClient;

    fn args() -> Vec<String> {
        ["mcp", "serve", "--profile", "agent"]
            .map(String::from)
            .to_vec()
    }

    /// Dexo's entry goes in; every other server and key stays as it was.
    #[test]
    fn a_json_config_keeps_what_it_had() {
        let existing = r#"{"theme": "dark", "mcpServers": {"other": {"command": "x"}, "dexo": {"command": "old"}}}"#;
        let merged = McpClient::Cursor
            .merged(Some(existing), "/bin/dexo", &args())
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["mcpServers"]["other"]["command"], "x");
        assert_eq!(value["mcpServers"]["dexo"]["command"], "/bin/dexo");
        assert_eq!(value["mcpServers"]["dexo"]["args"][3], "agent");
        assert_eq!(
            McpClient::Cursor.configured_command(&merged).as_deref(),
            Some("/bin/dexo")
        );
        assert!(
            McpClient::ClaudeDesktop
                .merged(Some("{ not json"), "/bin/dexo", &args())
                .is_err()
        );
        assert!(
            McpClient::ClaudeCode
                .merged(None, "/bin/dexo", &args())
                .is_ok()
        );
    }

    /// Codex's TOML is edited in place: comments and other tables stay, and the dexo
    /// table is replaced rather than repeated.
    #[test]
    fn a_toml_config_keeps_its_comments() {
        let existing = "# my settings\nmodel = \"o3\"\n\n[mcp_servers.dexo]\ncommand = \"old\"\nargs = []\n\n[mcp_servers.other]\ncommand = \"y\"\n";
        let merged = McpClient::Codex
            .merged(Some(existing), "/bin/dexo", &args())
            .unwrap();
        assert!(
            merged.starts_with("# my settings\nmodel = \"o3\"\n"),
            "{merged}"
        );
        assert_eq!(merged.matches("[mcp_servers.dexo]").count(), 1, "{merged}");
        assert!(
            merged.contains("[mcp_servers.other]\ncommand = \"y\""),
            "{merged}"
        );
        let table: toml::Table = merged.parse().unwrap();
        assert_eq!(
            table["mcp_servers"]["dexo"]["command"].as_str(),
            Some("/bin/dexo")
        );
        assert_eq!(
            McpClient::Codex.configured_command(&merged).as_deref(),
            Some("/bin/dexo")
        );
        let fresh = McpClient::Codex
            .merged(None, "C:\\dexo\\dexo.exe", &args())
            .unwrap();
        let table: toml::Table = fresh.parse().unwrap();
        assert_eq!(
            table["mcp_servers"]["dexo"]["command"].as_str(),
            Some("C:\\dexo\\dexo.exe")
        );
    }
}
