use std::collections::{BTreeMap, HashMap};

use dexo_app::{
    AppError, ConnectionId, ConnectionPolicyOverrides, ConnectionProfile, ConnectionProfiles,
    ErrorCategory, PURPOSE_DATABASE_PASSWORD, SecretRef,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

const SECRET_CONFIG_KEYS: &[&str] = &["password", "secret", "pass", "token", "api_key"];

const PROFILE_COLUMNS: &str =
    "id, project_id, name, driver, environment, config_json, secret_ref, group_path, policy_json";

pub struct ConnectionRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ConnectionRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, profile: &ConnectionProfile) -> anyhow::Result<()> {
        let refs = persist_secret_refs(profile)?;
        let password_ref = refs
            .get(PURPOSE_DATABASE_PASSWORD)
            .ok_or_else(|| anyhow::anyhow!("secret_ref must not be empty"))?;
        let config = strip_secret_keys(&profile.config);
        let project_id = profile.project_id.map(|id| id.to_string());
        let policy_json = serde_json::to_string(&profile.policy)?;
        self.conn.execute(
            "INSERT INTO connections (id, project_id, name, driver, environment, config_json, secret_ref, group_path, policy_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               project_id = excluded.project_id,
               name = excluded.name,
               driver = excluded.driver,
               environment = excluded.environment,
               config_json = excluded.config_json,
               secret_ref = excluded.secret_ref,
               group_path = excluded.group_path,
               policy_json = excluded.policy_json",
            params![
                profile.id.0.to_string(),
                project_id,
                profile.name,
                profile.driver,
                profile.environment,
                config.to_string(),
                password_ref.as_str(),
                profile.group_path,
                policy_json,
            ],
        )?;
        self.replace_secret_refs(profile.id, &refs)?;
        Ok(())
    }

    pub fn update(&self, profile: &ConnectionProfile) -> anyhow::Result<()> {
        if self.get(profile.id)?.is_none() {
            anyhow::bail!("unknown connection {}", profile.id.0);
        }
        self.save(profile)
    }

    pub fn duplicate(&self, id: ConnectionId) -> anyhow::Result<ConnectionProfile> {
        let mut profile = self
            .get(id)?
            .ok_or_else(|| anyhow::anyhow!("unknown connection {}", id.0))?;
        profile.id = ConnectionId(Uuid::new_v4());
        profile.name = unique_copy_name(&profile.name, self)?;
        profile.secret_refs = profile
            .secret_refs
            .keys()
            .map(|purpose| (purpose.clone(), SecretRef::new(Uuid::new_v4().to_string())))
            .collect();
        if profile.secret_refs.is_empty() {
            profile.secret_refs.insert(
                PURPOSE_DATABASE_PASSWORD.to_string(),
                SecretRef::new(Uuid::new_v4().to_string()),
            );
        }
        profile.secret_ref = profile
            .secret_refs
            .get(PURPOSE_DATABASE_PASSWORD)
            .cloned()
            .unwrap_or_else(|| SecretRef::new(Uuid::new_v4().to_string()));
        self.save(&profile)?;
        Ok(profile)
    }

    pub fn move_group(&self, id: ConnectionId, group_path: Option<&str>) -> anyhow::Result<()> {
        let mut profile = self
            .get(id)?
            .ok_or_else(|| anyhow::anyhow!("unknown connection {}", id.0))?;
        profile.group_path = group_path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string);
        self.save(&profile)
    }

    pub fn get_by_name(&self, name: &str) -> anyhow::Result<Option<ConnectionProfile>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM connections WHERE name = ?1"
        ))?;
        let mut rows = stmt
            .query_map(params![name], row_to_profile)?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > 1 {
            anyhow::bail!("multiple connections named '{name}'");
        }
        self.attach_secret_refs(&mut rows)?;
        Ok(rows.into_iter().next())
    }

    pub fn get(&self, id: ConnectionId) -> anyhow::Result<Option<ConnectionProfile>> {
        let mut rows = self
            .conn
            .query_row(
                &format!("SELECT {PROFILE_COLUMNS} FROM connections WHERE id = ?1"),
                params![id.0.to_string()],
                row_to_profile,
            )
            .optional()?
            .into_iter()
            .collect::<Vec<_>>();
        self.attach_secret_refs(&mut rows)?;
        Ok(rows.into_iter().next())
    }

    pub fn delete(&self, id: ConnectionId) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM connections WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        Ok(())
    }

    pub fn list(&self) -> anyhow::Result<Vec<ConnectionProfile>> {
        self.list_filtered(None)
    }

    pub fn list_for_project(&self, project_id: Uuid) -> anyhow::Result<Vec<ConnectionProfile>> {
        self.list_filtered(Some(project_id))
    }

    fn list_filtered(&self, project_id: Option<Uuid>) -> anyhow::Result<Vec<ConnectionProfile>> {
        let sql = match project_id {
            Some(_) => format!(
                "SELECT {PROFILE_COLUMNS} FROM connections WHERE project_id = ?1 ORDER BY group_path, name"
            ),
            None => format!("SELECT {PROFILE_COLUMNS} FROM connections ORDER BY group_path, name"),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = match project_id {
            Some(id) => stmt
                .query_map(params![id.to_string()], row_to_profile)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], row_to_profile)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        self.attach_secret_refs(&mut rows)?;
        Ok(rows)
    }

    fn replace_secret_refs(
        &self,
        id: ConnectionId,
        refs: &BTreeMap<String, SecretRef>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM connection_secret_refs WHERE connection_id = ?1",
            params![id.0.to_string()],
        )?;
        for (purpose, secret_ref) in refs {
            self.conn.execute(
                "INSERT INTO connection_secret_refs(connection_id, purpose, secret_ref)
                 VALUES (?1, ?2, ?3)",
                params![id.0.to_string(), purpose, secret_ref.as_str()],
            )?;
        }
        Ok(())
    }

    fn attach_secret_refs(&self, profiles: &mut [ConnectionProfile]) -> anyhow::Result<()> {
        if profiles.is_empty() {
            return Ok(());
        }
        // ponytail: load every secret-ref row then join in memory. Ceiling: all local profiles.
        // Filter with WHERE connection_id IN (...) if this store ever holds thousands of connections.
        let mut stmt = self
            .conn
            .prepare("SELECT connection_id, purpose, secret_ref FROM connection_secret_refs")?;
        let mut by_id: HashMap<String, BTreeMap<String, SecretRef>> = HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (connection_id, purpose, secret_ref) = row?;
            by_id
                .entry(connection_id)
                .or_default()
                .insert(purpose, SecretRef::new(secret_ref));
        }
        for profile in profiles {
            if let Some(refs) = by_id.remove(&profile.id.0.to_string()) {
                if let Some(password) = refs.get(PURPOSE_DATABASE_PASSWORD) {
                    profile.secret_ref = password.clone();
                }
                profile.secret_refs = refs;
            } else if !profile.secret_ref.as_str().is_empty() {
                profile.secret_refs.insert(
                    PURPOSE_DATABASE_PASSWORD.to_string(),
                    profile.secret_ref.clone(),
                );
            }
        }
        Ok(())
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

fn persist_secret_refs(profile: &ConnectionProfile) -> anyhow::Result<BTreeMap<String, SecretRef>> {
    let mut refs = profile.secret_refs.clone();
    if !profile.secret_ref.as_str().is_empty() {
        refs.entry(PURPOSE_DATABASE_PASSWORD.to_string())
            .or_insert_with(|| profile.secret_ref.clone());
    }
    if refs
        .get(PURPOSE_DATABASE_PASSWORD)
        .map(|value| value.as_str().is_empty())
        .unwrap_or(true)
    {
        anyhow::bail!("secret_ref must not be empty");
    }
    for (purpose, secret_ref) in &refs {
        if purpose.trim().is_empty() || secret_ref.as_str().is_empty() {
            anyhow::bail!("secret purpose and ref must not be empty");
        }
    }
    Ok(refs)
}

fn unique_copy_name(name: &str, repo: &ConnectionRepository<'_>) -> anyhow::Result<String> {
    let candidate = format!("{name} (copy)");
    if repo.get_by_name(&candidate)?.is_none() {
        return Ok(candidate);
    }
    for n in 2..1000 {
        let candidate = format!("{name} (copy {n})");
        if repo.get_by_name(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("too many copies of '{name}'")
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionProfile> {
    let id: String = row.get(0)?;
    let project_id: Option<String> = row.get(1)?;
    let config_json: String = row.get(5)?;
    let secret_ref: String = row.get(6)?;
    let group_path: Option<String> = row.get(7)?;
    let policy_json: String = row.get(8)?;
    let policy: ConnectionPolicyOverrides = serde_json::from_str(&policy_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(ConnectionProfile {
        id: ConnectionId(parse_uuid(&id)?),
        project_id: project_id.as_deref().map(parse_uuid).transpose()?,
        name: row.get(2)?,
        driver: row.get(3)?,
        environment: row.get(4)?,
        config: serde_json::from_str(&config_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        group_path,
        policy,
        secret_ref: SecretRef::new(secret_ref),
        secret_refs: BTreeMap::new(),
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
    #[serde(default)]
    group_path: Option<String>,
    #[serde(default)]
    policy_json: String,
    secret_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub connections_needing_secret: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ImportResolution {
    Replace,
    Rename(String),
    #[default]
    Skip,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPreview {
    pub conflicts: Vec<String>,
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
            group_path: c.group_path,
            policy_json: serde_json::to_string(&c.policy).unwrap_or_else(|_| "{}".into()),
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
        let policy = if item.policy_json.trim().is_empty() {
            ConnectionPolicyOverrides::default()
        } else {
            serde_json::from_str(&item.policy_json)?
        };
        let mut profile = ConnectionProfile::new(
            ConnectionId(parse_uuid_anyhow(&item.id)?),
            item.project_id
                .as_deref()
                .map(parse_uuid_anyhow)
                .transpose()?,
            item.name.clone(),
            item.driver,
            item.environment,
            config,
            SecretRef::new(uuid::Uuid::new_v4().to_string()),
        );
        profile.group_path = item.group_path;
        profile.policy = policy;
        connections.save(&profile)?;
        connections_needing_secret.push(item.name);
    }
    Ok(ImportReport {
        connections_needing_secret,
    })
}

pub fn preview_import(conn: &Connection, toml_text: &str) -> anyhow::Result<ImportPreview> {
    let portable: PortableConfig = toml::from_str(toml_text)?;
    if portable.version != PORTABLE_VERSION {
        anyhow::bail!("unsupported config version {}", portable.version);
    }
    let existing: HashMap<String, ConnectionProfile> = ConnectionRepository::new(conn)
        .list()?
        .into_iter()
        .map(|profile| (profile.name.clone(), profile))
        .collect();
    let mut conflicts = Vec::new();
    let mut connections_needing_secret = Vec::new();
    for item in &portable.connections {
        if existing.contains_key(&item.name) {
            conflicts.push(item.name.clone());
        }
        connections_needing_secret.push(item.name.clone());
    }
    Ok(ImportPreview {
        conflicts,
        connections_needing_secret,
    })
}

pub fn import_portable_resolved(
    conn: &Connection,
    toml_text: &str,
    resolutions: &HashMap<String, ImportResolution>,
) -> anyhow::Result<ImportReport> {
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
    let existing: HashMap<String, ConnectionProfile> = connections
        .list()?
        .into_iter()
        .map(|profile| (profile.name.clone(), profile))
        .collect();
    let mut connections_needing_secret = Vec::new();
    for item in portable.connections {
        let resolution = if existing.contains_key(&item.name) {
            resolutions
                .get(&item.name)
                .cloned()
                .unwrap_or(ImportResolution::Skip)
        } else {
            ImportResolution::Replace
        };
        let replace = matches!(resolution, ImportResolution::Replace);
        let name = match resolution {
            ImportResolution::Skip => continue,
            ImportResolution::Rename(name) => name,
            ImportResolution::Replace => item.name.clone(),
        };
        let config: Value = serde_json::from_str(&item.config_json)?;
        let policy = if item.policy_json.trim().is_empty() {
            ConnectionPolicyOverrides::default()
        } else {
            serde_json::from_str(&item.policy_json)?
        };
        let id = if replace {
            existing
                .get(&item.name)
                .map(|profile| profile.id)
                .unwrap_or_else(|| {
                    ConnectionId(parse_uuid_anyhow(&item.id).unwrap_or_else(|_| Uuid::new_v4()))
                })
        } else {
            ConnectionId(Uuid::new_v4())
        };
        let mut profile = ConnectionProfile::new(
            id,
            item.project_id
                .as_deref()
                .map(parse_uuid_anyhow)
                .transpose()?,
            name.clone(),
            item.driver,
            item.environment,
            config,
            SecretRef::new(Uuid::new_v4().to_string()),
        );
        profile.group_path = item.group_path;
        profile.policy = policy;
        connections.save(&profile)?;
        connections_needing_secret.push(name);
    }
    Ok(ImportReport {
        connections_needing_secret,
    })
}

fn parse_uuid_anyhow(value: &str) -> anyhow::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(Into::into)
}
