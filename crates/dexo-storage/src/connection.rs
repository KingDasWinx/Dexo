use dexo_app::{
    AppError, ConnectionId, ConnectionProfile, ConnectionProfiles, ErrorCategory, SecretRef,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

const SECRET_CONFIG_KEYS: &[&str] = &["password", "secret", "pass", "token", "api_key"];

pub struct ConnectionRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ConnectionRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, profile: &ConnectionProfile) -> anyhow::Result<()> {
        if profile.secret_ref.as_str().is_empty() {
            anyhow::bail!("secret_ref must not be empty");
        }
        let config = strip_secret_keys(&profile.config);
        let project_id = profile.project_id.map(|id| id.to_string());
        self.conn.execute(
            "INSERT INTO connections (id, project_id, name, driver, environment, config_json, secret_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               project_id = excluded.project_id,
               name = excluded.name,
               driver = excluded.driver,
               environment = excluded.environment,
               config_json = excluded.config_json,
               secret_ref = excluded.secret_ref",
            params![
                profile.id.0.to_string(),
                project_id,
                profile.name,
                profile.driver,
                profile.environment,
                config.to_string(),
                profile.secret_ref.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn get_by_name(&self, name: &str) -> anyhow::Result<Option<ConnectionProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, driver, environment, config_json, secret_ref
             FROM connections WHERE name = ?1",
        )?;
        let rows = stmt
            .query_map(params![name], row_to_profile)?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > 1 {
            anyhow::bail!("multiple connections named '{name}'");
        }
        Ok(rows.into_iter().next())
    }

    pub fn get(&self, id: ConnectionId) -> anyhow::Result<Option<ConnectionProfile>> {
        self.conn
            .query_row(
                "SELECT id, project_id, name, driver, environment, config_json, secret_ref
                 FROM connections WHERE id = ?1",
                params![id.0.to_string()],
                row_to_profile,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete(&self, id: ConnectionId) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM connections WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        Ok(())
    }

    pub fn list(&self) -> anyhow::Result<Vec<ConnectionProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, driver, environment, config_json, secret_ref
             FROM connections ORDER BY name",
        )?;
        let rows = stmt.query_map([], row_to_profile)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

impl ConnectionProfiles for ConnectionRepository<'_> {
    fn get_by_name(&self, name: &str) -> Result<Option<ConnectionProfile>, AppError> {
        ConnectionRepository::get_by_name(self, name)
            .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))
    }

    fn save(&self, profile: &ConnectionProfile) -> Result<(), AppError> {
        ConnectionRepository::save(self, profile)
            .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))
    }
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionProfile> {
    let id: String = row.get(0)?;
    let project_id: Option<String> = row.get(1)?;
    let config_json: String = row.get(5)?;
    let secret_ref: String = row.get(6)?;
    Ok(ConnectionProfile {
        id: ConnectionId(parse_uuid(&id)?),
        project_id: project_id.as_deref().map(parse_uuid).transpose()?,
        name: row.get(2)?,
        driver: row.get(3)?,
        environment: row.get(4)?,
        config: serde_json::from_str(&config_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        secret_ref: SecretRef::new(secret_ref),
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn strip_secret_keys(config: &Value) -> Value {
    let mut config = config.clone();
    if let Some(obj) = config.as_object_mut() {
        for key in SECRET_CONFIG_KEYS {
            obj.remove(*key);
        }
    }
    config
}

const PORTABLE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct PortableConfig {
    version: u32,
    projects: Vec<PortableProject>,
    connections: Vec<PortableConnection>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PortableProject {
    id: String,
    name: String,
    created_at: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PortableConnection {
    id: String,
    project_id: Option<String>,
    name: String,
    driver: String,
    environment: String,
    config_json: String,
    secret_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub connections_needing_secret: Vec<String>,
}

pub fn export_portable(conn: &Connection) -> anyhow::Result<String> {
    let projects = crate::ProjectRepository::new(conn)
        .list()?
        .into_iter()
        .map(|p| PortableProject {
            id: p.id.0.to_string(),
            name: p.name,
            created_at: p.created_at,
        })
        .collect();
    let connections = ConnectionRepository::new(conn)
        .list()?
        .into_iter()
        .map(|c| PortableConnection {
            id: c.id.0.to_string(),
            project_id: c.project_id.map(|id| id.to_string()),
            name: c.name,
            driver: c.driver,
            environment: c.environment,
            config_json: strip_secret_keys(&c.config).to_string(),
            secret_ref: String::new(),
        })
        .collect();
    Ok(toml::to_string(&PortableConfig {
        version: PORTABLE_VERSION,
        projects,
        connections,
    })?)
}

pub fn import_portable(conn: &Connection, toml_text: &str) -> anyhow::Result<ImportReport> {
    let portable: PortableConfig = toml::from_str(toml_text)?;
    if portable.version != PORTABLE_VERSION {
        anyhow::bail!("unsupported config version {}", portable.version);
    }
    let projects = crate::ProjectRepository::new(conn);
    for project in portable.projects {
        projects.save(&dexo_app::Project {
            id: dexo_app::ProjectId(parse_uuid_anyhow(&project.id)?),
            name: project.name,
            created_at: project.created_at,
        })?;
    }
    let connections = ConnectionRepository::new(conn);
    let mut connections_needing_secret = Vec::new();
    for item in portable.connections {
        let config: Value = serde_json::from_str(&item.config_json)?;
        let profile = ConnectionProfile {
            id: ConnectionId(parse_uuid_anyhow(&item.id)?),
            project_id: item
                .project_id
                .as_deref()
                .map(parse_uuid_anyhow)
                .transpose()?,
            name: item.name.clone(),
            driver: item.driver,
            environment: item.environment,
            config,
            secret_ref: SecretRef::new(uuid::Uuid::new_v4().to_string()),
        };
        connections.save(&profile)?;
        connections_needing_secret.push(item.name);
    }
    Ok(ImportReport {
        connections_needing_secret,
    })
}

fn parse_uuid_anyhow(value: &str) -> anyhow::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(Into::into)
}
