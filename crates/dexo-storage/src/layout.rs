use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const LAYOUT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkbenchLayout {
    pub version: u32,
    pub explorer_visible: bool,
    pub inspector_visible: bool,
    pub results_visible: bool,
    pub explorer_width: u16,
    pub inspector_width: u16,
    pub results_height: u16,
    pub focused_panel: String,
    pub active_tab: usize,
    pub tabs: Vec<String>,
    #[serde(default)]
    pub document_ids: Vec<String>,
    #[serde(default)]
    pub active_document_id: Option<String>,
    #[serde(default)]
    pub active_connection_id: Option<String>,
    #[serde(default)]
    pub active_result_tab: usize,
}

impl Default for WorkbenchLayout {
    fn default() -> Self {
        Self {
            version: LAYOUT_VERSION,
            explorer_visible: true,
            inspector_visible: true,
            results_visible: true,
            explorer_width: 28,
            inspector_width: 28,
            results_height: 12,
            focused_panel: "editor".into(),
            active_tab: 0,
            tabs: vec![
                "SQL".into(),
                "Data".into(),
                "DDL".into(),
                "Properties".into(),
                "Explain".into(),
            ],
            document_ids: Vec::new(),
            active_document_id: None,
            active_connection_id: None,
            active_result_tab: 0,
        }
    }
}

impl WorkbenchLayout {
    pub fn clamp(mut self, width: u16, height: u16) -> Self {
        let max_side = width.saturating_div(2).max(8);
        let max_results = height.saturating_sub(6).max(3);
        self.explorer_width = self.explorer_width.min(max_side).max(8);
        self.inspector_width = self.inspector_width.min(max_side).max(8);
        self.results_height = self.results_height.min(max_results).max(3);
        if width < 80 {
            self.inspector_visible = false;
        }
        if width < 60 || height < 24 {
            self.explorer_visible = false;
            self.inspector_visible = false;
            self.results_visible = false;
        }
        self
    }
}

pub struct LayoutRepository<'a> {
    conn: &'a Connection,
}

impl<'a> LayoutRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, project_id: &str, layout: &WorkbenchLayout) -> anyhow::Result<()> {
        let json = serde_json::to_string(layout)?;
        self.conn.execute(
            "INSERT INTO workbench_layouts (project_id, version, json, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(project_id) DO UPDATE SET
               version = excluded.version,
               json = excluded.json,
               updated_at = excluded.updated_at",
            params![project_id, layout.version as i64, json],
        )?;
        Ok(())
    }

    pub fn load(&self, project_id: &str) -> anyhow::Result<Option<WorkbenchLayout>> {
        self.conn
            .query_row(
                "SELECT json FROM workbench_layouts WHERE project_id = ?1",
                params![project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: String,
    pub keymap: String,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: bool,
    extra: toml::Table,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            keymap: "default".into(),
            mouse: true,
            animation: true,
            unicode: true,
            extra: toml::Table::new(),
        }
    }
}

impl Preferences {
    pub fn from_toml(src: &str) -> anyhow::Result<Self> {
        let table: toml::Table = src.parse()?;
        Ok(Self::from_table(table))
    }

    fn from_table(mut table: toml::Table) -> Self {
        let take_str = |table: &mut toml::Table, key: &str, default: &str| {
            table
                .remove(key)
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| default.into())
        };
        let take_bool = |table: &mut toml::Table, key: &str, default: bool| {
            table
                .remove(key)
                .and_then(|v| v.as_bool())
                .unwrap_or(default)
        };
        Self {
            theme: take_str(&mut table, "theme", "dark"),
            keymap: take_str(&mut table, "keymap", "default"),
            mouse: take_bool(&mut table, "mouse", true),
            animation: take_bool(&mut table, "animation", true),
            unicode: take_bool(&mut table, "unicode", true),
            extra: table,
        }
    }

    pub fn to_toml(&self) -> anyhow::Result<String> {
        let mut table = self.extra.clone();
        table.insert("theme".into(), self.theme.clone().into());
        table.insert("keymap".into(), self.keymap.clone().into());
        table.insert("mouse".into(), self.mouse.into());
        table.insert("animation".into(), self.animation.into());
        table.insert("unicode".into(), self.unicode.into());
        Ok(toml::to_string(&table)?)
    }

    pub fn load_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    pub fn save_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LAYOUT_VERSION, LayoutRepository, Preferences, WorkbenchLayout};
    use crate::migrations::{self, MIGRATION_1};
    use crate::{Database, ProjectRepository};
    use dexo_app::{Project, ProjectId};

    fn db_with_project() -> (Database, String) {
        let db = Database::open_in_memory().unwrap();
        let id = uuid::Uuid::new_v4();
        ProjectRepository::new(db.connection())
            .save(&Project {
                id: ProjectId(id),
                name: "demo".into(),
                created_at: "now".into(),
            })
            .unwrap();
        (db, id.to_string())
    }

    #[test]
    fn layout_round_trip_and_version() {
        let (db, project) = db_with_project();
        let layout = WorkbenchLayout {
            explorer_width: 40,
            focused_panel: "results".into(),
            active_tab: 2,
            ..WorkbenchLayout::default()
        };
        LayoutRepository::new(db.connection())
            .save(&project, &layout)
            .unwrap();
        let loaded = LayoutRepository::new(db.connection())
            .load(&project)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, LAYOUT_VERSION);
        assert_eq!(loaded, layout);
    }

    #[test]
    fn clamp_fits_compact_terminal() {
        let layout = WorkbenchLayout {
            explorer_width: 200,
            inspector_width: 200,
            results_height: 80,
            ..WorkbenchLayout::default()
        }
        .clamp(50, 18);
        assert!(!layout.explorer_visible);
        assert!(!layout.inspector_visible);
        assert!(!layout.results_visible);
        assert!(layout.explorer_width <= 25);
        assert!(layout.results_height <= 18);
    }

    #[test]
    fn migrates_v6_to_v7() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(crate::migrations::MIGRATION_2).unwrap();
        conn.execute_batch(crate::migrations::MIGRATION_3).unwrap();
        conn.execute_batch(crate::migrations::MIGRATION_4).unwrap();
        conn.execute_batch(crate::migrations::MIGRATION_5).unwrap();
        conn.execute_batch(crate::migrations::MIGRATION_6).unwrap();
        assert_eq!(migrations::read_schema_version(&conn), 6);
        migrations::apply_pending(&conn).unwrap();
        assert_eq!(migrations::read_schema_version(&conn), 9);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='workbench_layouts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "workbench_layouts");
    }

    #[test]
    fn unknown_toml_fields_round_trip() {
        let src = "theme = \"light\"\nkeymap = \"vim\"\nmouse = false\nanimation = false\nunicode = false\nfuture_flag = true\nextra_map = { a = 1 }\n";
        let prefs = Preferences::from_toml(src).unwrap();
        assert_eq!(prefs.theme, "light");
        assert_eq!(prefs.keymap, "vim");
        assert!(!prefs.mouse);
        let out = prefs.to_toml().unwrap();
        assert!(out.contains("future_flag = true"));
        assert!(out.contains("extra_map"));
        let again = Preferences::from_toml(&out).unwrap();
        assert_eq!(again.theme, "light");
        assert!(again.to_toml().unwrap().contains("future_flag = true"));
    }
}
