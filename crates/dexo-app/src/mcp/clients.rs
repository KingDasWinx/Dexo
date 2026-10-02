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
    /// that does not parse is an error, and is not rewritten. A byte order mark, which
    /// some Windows editors write, is read past and kept.
    pub fn merged(
        self,
        existing: Option<&str>,
        command: &str,
        args: &[String],
    ) -> Result<String, AppError> {
        let (bom, existing) = match existing.and_then(|text| text.strip_prefix(BOM)) {
            Some(text) => (BOM, Some(text)),
            None => ("", existing),
        };
        let merged = match self {
            Self::Codex => merged_toml(existing.unwrap_or(""), command, args)?,
            _ => merged_json(existing, command, args)?,
        };
        Ok(format!("{bom}{merged}"))
    }

    /// Whether the file has a `dexo` entry, and the command it runs; an error when the
    /// file does not parse, which setup would leave alone.
    pub fn configured_command(self, contents: &str) -> Result<Option<String>, AppError> {
        let contents = contents.strip_prefix(BOM).unwrap_or(contents);
        let unparsed = |error: String| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("it cannot be parsed ({error}), and dexo mcp setup leaves it alone"),
            )
        };
        let command = match self {
            Self::Codex => {
                let table: toml::Table = contents
                    .parse()
                    .map_err(|error: toml::de::Error| unparsed(error.message().to_string()))?;
                table
                    .get("mcp_servers")
                    .and_then(|servers| servers.get("dexo"))
                    .and_then(|dexo| dexo.get("command"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
            }
            _ if contents.trim().is_empty() => None,
            _ => serde_json::from_str::<serde_json::Value>(contents)
                .map_err(|error| unparsed(error.to_string()))?
                .pointer("/mcpServers/dexo/command")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        };
        Ok(command)
    }
}

/// The file `command` runs, found the way a client starting it would: a path as it is,
/// a bare name on PATH.
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    let runnable = |path: &Path| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        path.is_file()
    };
    let path = Path::new(command);
    if path.components().count() > 1 {
        return runnable(path).then(|| path.to_path_buf());
    }
    let extensions: Vec<String> = if cfg!(windows) {
        std::iter::once(String::new())
            .chain(
                std::env::var("PATHEXT")
                    .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
                    .split(';')
                    .map(str::to_string),
            )
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| {
            extensions
                .iter()
                .map(move |extension| dir.join(format!("{command}{extension}")))
        })
        .find(|candidate| runnable(candidate))
}

/// A JSON value whose objects keep their keys in the file's order. serde_json's own map
/// sorts them, which would reorder a committed `.mcp.json` on every setup; its
/// `preserve_order` feature would do the same for every hash and payload in the build.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
enum Json {
    Object(indexmap::IndexMap<String, Json>),
    Array(Vec<Json>),
    Other(serde_json::Value),
}

impl Json {
    fn object() -> Self {
        Self::Object(indexmap::IndexMap::new())
    }

    fn as_object_mut(&mut self) -> Option<&mut indexmap::IndexMap<String, Json>> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }
}

const BOM: &str = "\u{feff}";

/// The client's file as text, or `None` when it has none yet. One that is there but
/// cannot be read, or is not UTF-8, is an error: it is left as it is, not replaced.
pub fn read_config(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::new(
            ErrorCategory::Configuration,
            format!("it cannot be read ({error}), so it was left as it is"),
        )),
    }
}

fn merged_json(existing: Option<&str>, command: &str, args: &[String]) -> Result<String, AppError> {
    let left = |what: String| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("{what}, so it was left as it is"),
        )
    };
    let mut root = match existing.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => serde_json::from_str::<Json>(text)
            .map_err(|error| left(format!("it is not JSON Dexo can read ({error})")))?,
        None => Json::object(),
    };
    let servers = root
        .as_object_mut()
        .ok_or_else(|| left("it is not a JSON object".into()))?
        .entry("mcpServers".into())
        .or_insert_with(Json::object)
        .as_object_mut()
        .ok_or_else(|| left("its mcpServers is not an object".into()))?;
    // Only the keys Dexo writes change: the entry's env, cwd, timeouts and the rest stay.
    let entry = servers
        .entry("dexo".into())
        .or_insert_with(Json::object)
        .as_object_mut()
        .ok_or_else(|| left("its mcpServers.dexo is not an object".into()))?;
    entry.insert("command".into(), Json::Other(serde_json::json!(command)));
    entry.insert("args".into(), Json::Other(serde_json::json!(args)));
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
    if !servers.contains_key("dexo") {
        let entry = if inline {
            toml_edit::value(toml_edit::InlineTable::new())
        } else {
            toml_edit::Item::Table(toml_edit::Table::new())
        };
        servers.insert("dexo", entry);
    }
    // Only the keys Dexo writes change: the entry's env, cwd, timeouts and the rest stay.
    let entry = servers
        .get_mut("dexo")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| left("its mcp_servers.dexo is not a table".into()))?;
    set_toml(entry, "command", command.into());
    set_toml(
        entry,
        "args",
        args.iter().collect::<toml_edit::Array>().into(),
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

/// `key` set to `value`, with the comment and spacing around the old value kept.
fn set_toml(table: &mut dyn toml_edit::TableLike, key: &str, value: toml_edit::Value) {
    match table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
}

/// Writes `contents` to `path`, the old file copied to `<file>.dexo-backup` first, or to
/// `<file>.dexo-backup.1`, `.2` and on when that is taken; unchanged text writes nothing. A
/// symlink is written through, so it stays a link to the file it named; the file keeps
/// its permissions, since it may hold other servers' keys, and a new one is the user's
/// alone.
pub fn write_with_backup(path: &Path, contents: &str) -> Result<Option<PathBuf>, AppError> {
    use std::io::Write as _;
    let storage = |error: std::io::Error| AppError::new(ErrorCategory::Storage, error.to_string());
    let path = &match std::fs::canonicalize(path) {
        Ok(real) => real,
        // A link to a file not made yet is followed by hand.
        Err(_) => match std::fs::read_link(path) {
            Ok(target) => path.parent().unwrap_or(Path::new("")).join(target),
            Err(_) => path.to_path_buf(),
        },
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(storage)?;
    }
    let existing = std::fs::metadata(path).ok();
    if std::fs::read(path).is_ok_and(|old| old == contents.as_bytes()) {
        return Ok(None);
    }
    // An earlier backup is never replaced: the first one is the file as it was before
    // Dexo ever touched it.
    let backup = if existing.is_some() {
        let backup = (0..)
            .map(|number| {
                let mut name = path.as_os_str().to_owned();
                name.push(".dexo-backup");
                if number > 0 {
                    name.push(format!(".{number}"));
                }
                PathBuf::from(name)
            })
            .find(|name| name.symlink_metadata().is_err())
            .expect("an unused backup name");
        std::fs::copy(path, &backup).map_err(storage)?;
        Some(backup)
    } else {
        None
    };
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".dexo-tmp");
    let tmp = PathBuf::from(tmp);
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(storage)?;
    file.write_all(contents.as_bytes()).map_err(storage)?;
    file.sync_all().map_err(storage)?;
    if let Some(existing) = &existing {
        std::fs::set_permissions(&tmp, existing.permissions()).map_err(storage)?;
    }
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
            McpClient::Cursor
                .configured_command(&merged)
                .unwrap()
                .as_deref(),
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
            McpClient::Codex
                .configured_command(&merged)
                .unwrap()
                .as_deref(),
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

    /// An existing dexo entry keeps every key Dexo does not write -- its environment,
    /// working directory, timeouts -- in JSON and in TOML, inline or not.
    #[test]
    fn an_existing_entry_keeps_its_other_keys() {
        let json = r#"{"mcpServers": {"dexo": {"type": "stdio", "command": "old", "cwd": "/w", "env": {"DEXO_DATA_HOME": "/d"}}}}"#;
        let merged = McpClient::ClaudeCode
            .merged(Some(json), "/bin/dexo", &args())
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        let dexo = &value["mcpServers"]["dexo"];
        assert_eq!(dexo["command"], "/bin/dexo");
        assert_eq!(dexo["args"][3], "agent");
        assert_eq!(dexo["type"], "stdio");
        assert_eq!(dexo["cwd"], "/w");
        assert_eq!(dexo["env"]["DEXO_DATA_HOME"], "/d");
        assert!(
            McpClient::Cursor
                .merged(Some(r#"{"mcpServers": {"dexo": 1}}"#), "/bin/dexo", &args())
                .is_err()
        );
        for toml in [
            "[mcp_servers.dexo]\ncommand = \"old\" # where it lives\nstartup_timeout_sec = 30\nenv = { DEXO_DATA_HOME = \"/d\" }\nmatrix = [\n  [1, 2],\n]\n",
            "[mcp_servers]\ndexo = { command = \"old\", startup_timeout_sec = 30, env = { DEXO_DATA_HOME = \"/d\" }, matrix = [[1, 2]] }\n",
        ] {
            let merged = McpClient::Codex
                .merged(Some(toml), "/bin/dexo", &args())
                .unwrap();
            let table: toml::Table = merged.parse().unwrap();
            let dexo = &table["mcp_servers"]["dexo"];
            assert_eq!(dexo["command"].as_str(), Some("/bin/dexo"), "{merged}");
            assert_eq!(dexo["startup_timeout_sec"].as_integer(), Some(30));
            assert_eq!(dexo["env"]["DEXO_DATA_HOME"].as_str(), Some("/d"));
            assert_eq!(dexo["matrix"][0][1].as_integer(), Some(2));
            assert_eq!(
                merged.contains("# where it lives"),
                toml.contains("# where")
            );
            assert_eq!(merged.contains("dexo = {"), toml.contains("dexo = {"));
        }
    }

    /// Keys stay in the order the file has them, at every depth, so a committed
    /// `.mcp.json` changes only where Dexo's entry does.
    #[test]
    fn a_json_config_keeps_its_key_order() {
        let existing = "{\n  \"zeta\": {\"b\": 1, \"a\": 2.5},\n  \"mcpServers\": {\n    \"zz\": {\"command\": \"z\"},\n    \"dexo\": {\"type\": \"stdio\", \"command\": \"old\"}\n  },\n  \"alpha\": [true, null]\n}\n";
        let merged = McpClient::ClaudeCode
            .merged(Some(existing), "/bin/dexo", &args())
            .unwrap();
        let order = |text: &str, keys: &[&str]| {
            let at: Vec<usize> = keys
                .iter()
                .map(|key| text.find(&format!("\"{key}\"")).unwrap())
                .collect();
            at.windows(2).all(|pair| pair[0] < pair[1])
        };
        assert!(
            order(
                &merged,
                &["zeta", "b", "a", "mcpServers", "zz", "dexo", "alpha"]
            ),
            "{merged}"
        );
        assert!(
            order(
                &merged[merged.find("\"dexo\"").unwrap()..],
                &["type", "command", "args"]
            ),
            "{merged}"
        );
        assert!(
            merged.contains("2.5") && merged.contains("null"),
            "{merged}"
        );
    }

    /// A symlinked config stays a link and its target gets the text; a private file
    /// stays private, and a new one is made private.
    #[cfg(unix)]
    #[test]
    fn a_config_is_written_through_its_link_with_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        std::fs::write(&real, "{}").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        super::write_with_backup(&link, "{\"a\": 1}").unwrap();
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{\"a\": 1}");
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&real), 0o600);
        let fresh = dir.path().join("new/mcp.json");
        super::write_with_backup(&fresh, "{}").unwrap();
        assert_eq!(mode(&fresh), 0o600);
        let shared = dir.path().join("shared.json");
        std::fs::write(&shared, "{}").unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o644)).unwrap();
        super::write_with_backup(&shared, "[]").unwrap();
        assert_eq!(mode(&shared), 0o644);
    }

    /// Running setup again never loses the file as it was before Dexo: each old version
    /// gets a backup of its own, and text that did not change writes nothing.
    #[test]
    fn a_backup_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        std::fs::write(&path, "original").unwrap();
        let first = super::write_with_backup(&path, "second").unwrap().unwrap();
        let again = super::write_with_backup(&path, "third").unwrap().unwrap();
        assert_ne!(first, again);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "original");
        assert_eq!(std::fs::read_to_string(&again).unwrap(), "second");
        assert_eq!(super::write_with_backup(&path, "third").unwrap(), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third");
    }

    /// A file with a byte order mark is merged and keeps it; one that exists but is not
    /// UTF-8 is an error, not a missing file to start afresh.
    #[test]
    fn a_bom_is_kept_and_an_unreadable_file_is_refused() {
        for client in [McpClient::ClaudeDesktop, McpClient::Codex] {
            let existing = match client {
                McpClient::Codex => "\u{feff}model = \"o3\"\n",
                _ => "\u{feff}{\"theme\": \"dark\"}",
            };
            let merged = client.merged(Some(existing), "/bin/dexo", &args()).unwrap();
            assert!(merged.starts_with('\u{feff}'), "{merged}");
            assert!(!merged[3..].contains('\u{feff}'), "{merged}");
            assert_eq!(
                client.configured_command(&merged).unwrap().as_deref(),
                Some("/bin/dexo")
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        assert!(super::read_config(&path).unwrap().is_none());
        std::fs::write(&path, b"{\"a\": \"\xff\"}").unwrap();
        assert!(super::read_config(&path).is_err());
    }

    /// The probe tells a file it cannot parse from one without Dexo, and finds a bare
    /// command on PATH as the client would.
    #[test]
    fn the_probe_reads_what_setup_would() {
        assert!(McpClient::Codex.configured_command("[mcp_servers").is_err());
        assert!(McpClient::Cursor.configured_command("{ nope").is_err());
        assert_eq!(McpClient::Cursor.configured_command("  ").unwrap(), None);
        assert_eq!(
            McpClient::Codex.configured_command("model = 1").unwrap(),
            None
        );
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(super::resolve_command(shell).is_some());
        assert!(super::resolve_command("dexo-surely-not-on-path").is_none());
        let here = std::env::current_exe().unwrap();
        assert_eq!(super::resolve_command(here.to_str().unwrap()), Some(here));
    }
}
