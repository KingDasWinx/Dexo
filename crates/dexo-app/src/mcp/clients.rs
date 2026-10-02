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
            Self::Codex => merged_toml(existing.unwrap_or(""), command, args),
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

/// Codex's TOML is parsed into a document that keeps its layout, so the user's comments
/// and formatting stay wherever the dexo entry is, and whatever shape it was written in.
fn merged_toml(existing: &str, command: &str, args: &[String]) -> Result<String, AppError> {
    let left = |what: String| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("{what}, so it was left as it is"),
        )
    };
    let mut document: toml_edit::DocumentMut = existing
        .parse()
        .map_err(|error| left(format!("it is not TOML Dexo can read ({error})")))?;
    let servers = document.entry("mcp_servers").or_insert_with(|| {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        toml_edit::Item::Table(table)
    });
    let inline = servers.is_inline_table();
    let servers = servers
        .as_table_like_mut()
        .ok_or_else(|| left("its mcp_servers is not a table".into()))?;
    let mut entry = toml_edit::Table::new();
    entry.insert("command", toml_edit::value(command));
    entry.insert(
        "args",
        toml_edit::value(args.iter().collect::<toml_edit::Array>()),
    );
    servers.insert(
        "dexo",
        if inline {
            toml_edit::value(entry.into_inline_table())
        } else {
            toml_edit::Item::Table(entry)
        },
    );
    let text = document.to_string();
    // What was written must read back as the entry it meant to write.
    let written: toml::Table = text
        .parse()
        .map_err(|error| left(format!("Dexo could not write its entry into it ({error})")))?;
    let dexo = written
        .get("mcp_servers")
        .and_then(|servers| servers.get("dexo"));
    let wrote_args: Option<Vec<&str>> = dexo
        .and_then(|dexo| dexo.get("args"))
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(toml::Value::as_str).collect());
    if dexo
        .and_then(|dexo| dexo.get("command"))
        .and_then(toml::Value::as_str)
        != Some(command)
        || wrote_args != Some(args.iter().map(String::as_str).collect())
    {
        return Err(left("Dexo could not write its entry into it".into()));
    }
    Ok(text)
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

    /// However the dexo entry is written -- a commented header, an inline table, dotted
    /// or quoted keys -- the result parses and runs Dexo; a file that does not parse is
    /// refused rather than rewritten.
    #[test]
    fn a_toml_config_is_parsed_not_matched_by_line() {
        for existing in [
            "[mcp_servers.dexo] # mine\ncommand = \"old\"\n",
            "[mcp_servers]\ndexo = { command = \"old\" }\nother = { command = \"y\" }\n",
            "mcp_servers.dexo.command = \"old\"\nmcp_servers.other.command = \"y\"\n",
            "[mcp_servers.\"dexo\"]\ncommand = \"old\"\n",
            "[mcp_servers.dexo]\ncommand = \"old\"\n\n[tools]\nmatrix = [\n  [1, 2],\n  [3, 4],\n]\n",
        ] {
            let merged = McpClient::Codex
                .merged(Some(existing), "/bin/dexo", &args())
                .unwrap();
            let table: toml::Table = merged
                .parse()
                .unwrap_or_else(|error| panic!("{error}\n{merged}"));
            let dexo = &table["mcp_servers"]["dexo"];
            assert_eq!(dexo["command"].as_str(), Some("/bin/dexo"), "{merged}");
            assert_eq!(dexo["args"][3].as_str(), Some("agent"), "{merged}");
            if existing.contains("other") {
                assert_eq!(table["mcp_servers"]["other"]["command"].as_str(), Some("y"));
            }
            if existing.contains("matrix") {
                assert_eq!(table["tools"]["matrix"][1][0].as_integer(), Some(3));
            }
        }
        assert!(
            McpClient::Codex
                .merged(
                    Some("[mcp_servers.dexo\ncommand = 1\n"),
                    "/bin/dexo",
                    &args()
                )
                .is_err()
        );
        assert!(
            McpClient::Codex
                .merged(Some("mcp_servers = 3\n"), "/bin/dexo", &args())
                .is_err()
        );
    }
}
