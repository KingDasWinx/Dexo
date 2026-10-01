use std::collections::BTreeMap;

use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, DriverError, ObjectDdl,
    ObjectId, ObjectKind, QualifiedName,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use crate::decode::quote;
use crate::error::map_error;
use crate::session::SqliteSession;

/// Ids read `sl:<kind>:<schema>/<table>/<name>`, as MySQL's do. The schema is `main`, or
/// the name an ATTACH gave a file.
fn sl_id(kind: &str, key: impl std::fmt::Display) -> ObjectId {
    ObjectId::new(format!("sl:{kind}:{key}"))
}

fn parse_id(id: &ObjectId) -> Option<(&str, &str)> {
    id.as_str()
        .strip_prefix("sl:")
        .and_then(|rest| rest.split_once(':'))
}

fn split2(key: &str) -> (&str, &str) {
    key.split_once('/').unwrap_or(("main", key))
}

fn split3(key: &str) -> (&str, &str, &str) {
    let (schema, rest) = split2(key);
    let (table, name) = rest.split_once('/').unwrap_or((rest, rest));
    (schema, table, name)
}

/// A relation is named the way SQLite writes it, `main.orders`: the schema is its
/// database, and there is no catalog above it.
fn named(schema: &str, object: impl Into<String>) -> QualifiedName {
    QualifiedName::new(None::<String>, Some(schema), object)
}

fn rows<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, DriverError> {
    let mut statement = conn.prepare(sql).map_err(map_error)?;
    statement
        .query_map(params, map)
        .map_err(map_error)?
        .collect::<rusqlite::Result<Vec<T>>>()
        .map_err(map_error)
}

#[async_trait::async_trait]
impl CatalogReader for SqliteSession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        let include_system = options.include_system;
        let Some(parent) = parent.cloned() else {
            return self
                .with_conn(move |conn| {
                    Ok(CatalogList {
                        objects: databases(conn, include_system)?,
                        restrictions: Vec::new(),
                    })
                })
                .await;
        };
        let objects = self
            .with_conn(move |conn| {
                let Some((kind, key)) = parse_id(&parent) else {
                    return Ok(Vec::new());
                };
                match kind {
                    "catalog" => relations(conn, &parent, key, include_system),
                    "table" | "view" => relation_children(conn, &parent, key),
                    _ => Ok(Vec::new()),
                }
            })
            .await?;
        Ok(CatalogList {
            objects,
            restrictions: Vec::new(),
        })
    }

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Ok(None);
        };
        let parent = match kind {
            "catalog" => None,
            "table" | "view" => Some(sl_id("catalog", split2(key).0)),
            "column" | "index" | "constraint" | "trigger" => {
                let (schema, table, _) = split3(key);
                let relation = self.relation_id(schema, table).await?;
                Some(relation)
            }
            _ => return Ok(None),
        };
        let list = self
            .list_children(
                parent.as_ref(),
                &CatalogListOptions {
                    include_system: true,
                },
            )
            .await?;
        Ok(list.objects.into_iter().find(|object| object.id == *id))
    }

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let (schema, by, name) = match kind {
            "table" | "view" => {
                let (schema, name) = split2(key);
                (schema, "tbl_name", name)
            }
            "index" | "trigger" => {
                let (schema, _, name) = split3(key);
                (schema, "name", name)
            }
            _ => {
                return Err(DriverError::unsupported(format!(
                    "ddl unavailable for {kind}"
                )));
            }
        };
        // A table's DDL carries its indexes and triggers after it, as the file holds
        // them; SQLite keeps no CREATE text for the indexes it made for a constraint.
        let sql = format!(
            "SELECT sql FROM {}.sqlite_master WHERE {by} = ?1 AND sql IS NOT NULL
             ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'view' THEN 0 WHEN 'index' THEN 1 ELSE 2 END, name",
            quote(schema)
        );
        let name = name.to_string();
        let statements = self
            .with_conn(move |conn| rows(conn, &sql, [name], |row| row.get::<_, String>(0)))
            .await?;
        if statements.is_empty() {
            return Err(DriverError::unsupported(
                "SQLite keeps no DDL for this object; it made it for a constraint",
            ));
        }
        Ok(ObjectDdl {
            object_id: id.clone(),
            sql: format!("{};", statements.join(";\n\n")),
        })
    }

    async fn dependencies(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.relation_graph(id, true).await
    }

    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.relation_graph(id, false).await
    }
}

impl SqliteSession {
    async fn relation_id(&self, schema: &str, name: &str) -> Result<ObjectId, DriverError> {
        let (schema, name) = (schema.to_string(), name.to_string());
        self.with_conn(move |conn| relation_kind(conn, &schema, &name))
            .await
    }

    /// Foreign keys both ways, and a table's triggers. SQLite records no view
    /// dependencies, so a view has none to report.
    async fn relation_graph(
        &self,
        id: &ObjectId,
        outgoing: bool,
    ) -> Result<Vec<ObjectId>, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let (kind, key) = (kind.to_string(), key.to_string());
        self.with_conn(move |conn| match (kind.as_str(), outgoing) {
            ("table", true) => {
                let (schema, table) = split2(&key);
                let sql = format!(
                    "SELECT DISTINCT m.name FROM pragma_foreign_key_list(?1, ?2) f
                     JOIN {}.sqlite_master m ON m.type = 'table' AND m.name = f.\"table\" COLLATE NOCASE",
                    quote(schema)
                );
                let names = rows(conn, &sql, params![table, schema], |row| row.get(0))?;
                Ok(names
                    .into_iter()
                    .map(|name: String| sl_id("table", format!("{schema}/{name}")))
                    .collect())
            }
            ("table" | "view", false) => {
                let (schema, table) = split2(&key);
                let sql = format!(
                    "SELECT DISTINCT 'table', m.name FROM {schema_q}.sqlite_master m
                     JOIN pragma_foreign_key_list(m.name, ?2) f
                     WHERE m.type = 'table' AND f.\"table\" = ?1 COLLATE NOCASE
                     UNION ALL
                     SELECT 'trigger', name FROM {schema_q}.sqlite_master
                     WHERE type = 'trigger' AND tbl_name = ?1",
                    schema_q = quote(schema)
                );
                let found = rows(conn, &sql, params![table, schema], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                Ok(found
                    .into_iter()
                    .map(|(kind, name)| match kind.as_str() {
                        "trigger" => sl_id("trigger", format!("{schema}/{table}/{name}")),
                        _ => sl_id("table", format!("{schema}/{name}")),
                    })
                    .collect())
            }
            ("trigger", true) => {
                let (schema, table, _) = split3(&key);
                Ok(vec![relation_kind(conn, schema, table)?])
            }
            _ => Ok(Vec::new()),
        })
        .await
    }
}

fn relation_kind(conn: &Connection, schema: &str, name: &str) -> Result<ObjectId, DriverError> {
    let sql = format!(
        "SELECT type FROM {}.sqlite_master WHERE name = ?1",
        quote(schema)
    );
    let kind: Option<String> = conn
        .query_row(&sql, [name], |row| row.get(0))
        .optional()
        .map_err(map_error)?;
    let kind = if kind.as_deref() == Some("view") {
        "view"
    } else {
        "table"
    };
    Ok(sl_id(kind, format!("{schema}/{name}")))
}

/// `main`, and every database an ATTACH added. `temp` only with system objects: it is
/// the connection's scratch space, not part of the file.
fn databases(conn: &Connection, include_system: bool) -> Result<Vec<CatalogObject>, DriverError> {
    let found = rows(
        conn,
        "SELECT name, file FROM pragma_database_list ORDER BY seq",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    Ok(found
        .into_iter()
        .filter(|(name, _)| include_system || name != "temp")
        .map(|(name, file)| {
            CatalogObject::new(
                sl_id("catalog", &name),
                ObjectKind::Catalog,
                QualifiedName::new(None::<String>, None::<String>, name),
                None,
            )
            .with_attribute("driver.sqlite.file", json!(file))
        })
        .collect())
}

fn relations(
    conn: &Connection,
    parent: &ObjectId,
    schema: &str,
    include_system: bool,
) -> Result<Vec<CatalogObject>, DriverError> {
    let sql = format!(
        "SELECT type, name FROM {}.sqlite_master
         WHERE type IN ('table', 'view') AND (?1 OR name NOT LIKE 'sqlite\\_%' ESCAPE '\\')
         ORDER BY name",
        quote(schema)
    );
    let found = rows(conn, &sql, [include_system], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(found
        .into_iter()
        .map(|(kind, name)| {
            let (key, kind) = if kind == "view" {
                ("view", ObjectKind::View)
            } else {
                ("table", ObjectKind::Table)
            };
            CatalogObject::new(
                sl_id(key, format!("{schema}/{name}")),
                kind,
                named(schema, name),
                Some(parent.clone()),
            )
        })
        .collect())
}

/// Columns, indexes, foreign keys and triggers of a table or view.
fn relation_children(
    conn: &Connection,
    parent: &ObjectId,
    key: &str,
) -> Result<Vec<CatalogObject>, DriverError> {
    let (schema, table) = split2(key);
    let child = |kind: &str, name: &str| sl_id(kind, format!("{schema}/{table}/{name}"));
    let mut objects = Vec::new();

    // `hidden` is 1 for a virtual table's hidden columns, 2 and 3 for generated ones,
    // which `table_info` would leave out.
    let columns = rows(
        conn,
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden
         FROM pragma_table_xinfo(?1, ?2) WHERE hidden <> 1 ORDER BY cid",
        params![table, schema],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )?;
    for (name, data_type, not_null, default, pk, hidden) in columns {
        let mut object = CatalogObject::new(
            child("column", &name),
            ObjectKind::Column,
            named(schema, format!("{table}.{name}")),
            Some(parent.clone()),
        )
        .with_attribute("type", json!(data_type))
        .with_attribute("driver.sqlite.not_null", json!(not_null));
        if pk > 0 {
            object = object.with_attribute("driver.sqlite.primary_key", json!(pk));
        }
        if let Some(default) = default {
            object = object.with_attribute("driver.sqlite.default", json!(default));
        }
        if hidden > 1 {
            object = object.with_attribute("driver.sqlite.generated", json!(true));
        }
        objects.push(object);
    }

    let indexes = rows(
        conn,
        "SELECT name, \"unique\", origin FROM pragma_index_list(?1, ?2) ORDER BY name",
        params![table, schema],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    for (name, unique, origin) in indexes {
        objects.push(
            CatalogObject::new(
                child("index", &name),
                ObjectKind::Index,
                named(schema, name.clone()),
                Some(parent.clone()),
            )
            .with_attribute("driver.sqlite.unique", json!(unique))
            .with_attribute("driver.sqlite.origin", json!(origin)),
        );
    }

    for key in foreign_keys(conn, schema, table)? {
        let name = format!("{table}_{}_fkey", key.local.join("_"));
        objects.push(
            CatalogObject::new(
                child("constraint", &name),
                ObjectKind::Constraint,
                named(schema, name.clone()),
                Some(parent.clone()),
            )
            .with_attribute("driver.sqlite.constraint_type", json!("FOREIGN KEY"))
            .with_attribute("fk_local", json!(key.local))
            .with_attribute("fk_referenced", json!(key.referenced))
            .with_attribute("fk_table", json!(key.table))
            .with_attribute("fk_schema", json!(schema)),
        );
    }

    let sql = format!(
        "SELECT name FROM {}.sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 ORDER BY name",
        quote(schema)
    );
    for name in rows(conn, &sql, [table], |row| row.get::<_, String>(0))? {
        objects.push(CatalogObject::new(
            child("trigger", &name),
            ObjectKind::Trigger,
            named(schema, name.clone()),
            Some(parent.clone()),
        ));
    }
    Ok(objects)
}

struct ForeignKey {
    table: String,
    local: Vec<String>,
    referenced: Vec<String>,
}

/// A table's foreign keys, one per constraint. `REFERENCES parent` without a column
/// list means the parent's primary key, which SQLite reports as no column at all.
fn foreign_keys(
    conn: &Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKey>, DriverError> {
    let parts = rows(
        conn,
        "SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1, ?2) ORDER BY id, seq",
        params![table, schema],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    let mut keys: BTreeMap<i64, ForeignKey> = BTreeMap::new();
    for (id, parent, from, to) in parts {
        let key = keys.entry(id).or_insert_with(|| ForeignKey {
            table: parent,
            local: Vec::new(),
            referenced: Vec::new(),
        });
        key.local.push(from);
        key.referenced.extend(to);
    }
    keys.into_values()
        .map(|mut key| {
            if key.referenced.len() != key.local.len() {
                key.referenced = rows(
                    conn,
                    "SELECT name FROM pragma_table_info(?1, ?2) WHERE pk > 0 ORDER BY pk",
                    params![key.table, schema],
                    |row| row.get(0),
                )?;
            }
            Ok(key)
        })
        .collect()
}
