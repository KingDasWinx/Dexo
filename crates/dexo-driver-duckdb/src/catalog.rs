use std::collections::HashSet;

use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, DriverError, ForeignKeyRef,
    ObjectDdl, ObjectId, ObjectKind, QualifiedName,
};
use duckdb::{Connection, OptionalExt, params};
use serde_json::json;

use crate::error::map_error;
use crate::session::DuckdbSession;

/// Ids read `dk:<kind>:<database>/<schema>/<table>/<name>`, each part with `%` and `/`
/// escaped, as many parts as the kind has.
fn dk_id(kind: &str, parts: &[&str]) -> ObjectId {
    let key = parts
        .iter()
        .map(|part| part.replace('%', "%25").replace('/', "%2F"))
        .collect::<Vec<_>>()
        .join("/");
    ObjectId::new(format!("dk:{kind}:{key}"))
}

fn parse_id(id: &ObjectId) -> Option<(&str, Vec<String>)> {
    let (kind, key) = id.as_str().strip_prefix("dk:")?.split_once(':')?;
    let parts = key
        .split('/')
        .map(|part| part.replace("%2F", "/").replace("%25", "%"))
        .collect();
    Some((kind, parts))
}

/// A relation is named the way DuckDB writes it in full, `shop.main.orders`.
fn named(database: &str, schema: &str, object: impl Into<String>) -> QualifiedName {
    QualifiedName::new(Some(database), Some(schema), object)
}

/// `database`, `schema` and `table` of a relation id.
fn relation_parts(parts: &[String]) -> Option<(&str, &str, &str)> {
    match parts {
        [database, schema, table, ..] => Some((database, schema, table)),
        _ => None,
    }
}

pub(crate) fn rows<T>(
    conn: &Connection,
    sql: &str,
    params: impl duckdb::Params,
    map: impl FnMut(&duckdb::Row<'_>) -> duckdb::Result<T>,
) -> Result<Vec<T>, DriverError> {
    let mut statement = conn.prepare(sql).map_err(map_error)?;
    statement
        .query_map(params, map)
        .map_err(map_error)?
        .collect::<duckdb::Result<Vec<T>>>()
        .map_err(map_error)
}

/// A list column as JSON text, which reads back as strings whatever the list held.
fn strings(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// The database and schema a name means: its own parts, or where DuckDB looks for a
/// name written without them.
pub(crate) fn place_of(
    conn: &Connection,
    name: &QualifiedName,
) -> Result<(String, String), DriverError> {
    let (database, schema): (String, String) = conn
        .query_row("SELECT current_database(), current_schema()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(map_error)?;
    Ok((
        name.catalog().map_or(database, str::to_string),
        name.schema().map_or(schema, str::to_string),
    ))
}

#[async_trait::async_trait]
impl CatalogReader for DuckdbSession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        let include_system = options.include_system;
        let parent = parent.cloned();
        let objects = self
            .with_conn(move |conn| {
                let Some(parent) = parent else {
                    return databases(conn, include_system);
                };
                let Some((kind, parts)) = parse_id(&parent) else {
                    return Ok(Vec::new());
                };
                match (kind, parts.as_slice()) {
                    ("catalog", [database]) => schemas(conn, &parent, database, include_system),
                    ("schema", [database, schema]) => {
                        relations(conn, &parent, database, schema, include_system)
                    }
                    ("table" | "view", [database, schema, table]) => {
                        relation_children(conn, &parent, database, schema, table)
                    }
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
        let Some((kind, parts)) = parse_id(id) else {
            return Ok(None);
        };
        let parent = match (kind, parts.as_slice()) {
            ("catalog", _) => None,
            ("schema", [database, _]) => Some(dk_id("catalog", &[database])),
            ("table" | "view", [database, schema, _]) => Some(dk_id("schema", &[database, schema])),
            ("column" | "index" | "constraint", [database, schema, table, _]) => {
                let (database, schema, table) = (database.clone(), schema.clone(), table.clone());
                Some(
                    self.with_conn(move |conn| relation_id(conn, &database, &schema, &table))
                        .await?,
                )
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

    /// The CREATE text DuckDB keeps: a table's with its indexes after it.
    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        let Some((kind, parts)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let sql = match kind {
            "table" => {
                "SELECT sql FROM duckdb_tables() WHERE database_name = $1 AND schema_name = $2 AND table_name = $3
                 UNION ALL
                 SELECT sql FROM (SELECT sql FROM duckdb_indexes()
                     WHERE database_name = $1 AND schema_name = $2 AND table_name = $3 AND sql IS NOT NULL
                     ORDER BY index_name)"
            }
            "view" => {
                "SELECT sql FROM duckdb_views() WHERE database_name = $1 AND schema_name = $2 AND view_name = $3"
            }
            "index" => {
                "SELECT sql FROM duckdb_indexes()
                 WHERE database_name = $1 AND schema_name = $2 AND table_name = $3 AND index_name = $4"
            }
            _ => {
                return Err(DriverError::unsupported(format!(
                    "ddl unavailable for {kind}"
                )));
            }
        };
        let statements = self
            .with_conn(move |conn| {
                let params = duckdb::params_from_iter(parts.iter());
                rows(conn, sql, params, |row| row.get::<_, Option<String>>(0))
            })
            .await?;
        let statements: Vec<String> = statements
            .into_iter()
            .flatten()
            .map(|sql| sql.trim().trim_end_matches(';').to_string())
            .collect();
        if statements.is_empty() {
            return Err(DriverError::unsupported(
                "DuckDB keeps no DDL for this object; it made it for a constraint",
            ));
        }
        Ok(ObjectDdl {
            object_id: id.clone(),
            sql: format!("{};", statements.join(";\n\n")),
        })
    }

    /// The tables a table's foreign keys point at.
    async fn dependencies(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.relation_graph(id, true).await
    }

    /// The tables whose foreign keys point at it.
    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.relation_graph(id, false).await
    }

    async fn foreign_keys(&self, table: &QualifiedName) -> Result<Vec<ForeignKeyRef>, DriverError> {
        let table = table.clone();
        self.with_conn(move |conn| {
            let (database, schema) = place_of(conn, &table)?;
            let found = rows(
                conn,
                "SELECT table_name, constraint_name, to_json(constraint_column_names)::VARCHAR,
                        referenced_table, to_json(referenced_column_names)::VARCHAR
                 FROM duckdb_constraints()
                 WHERE constraint_type = 'FOREIGN KEY' AND database_name = $1 AND schema_name = $2
                   AND (lower(table_name) = lower($3) OR lower(referenced_table) = lower($3))
                 ORDER BY table_name, constraint_index",
                params![database, schema, table.object()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )?;
            Ok(found
                .into_iter()
                .map(|(from, name, local, to, referenced)| {
                    let from_columns = strings(&local);
                    ForeignKeyRef {
                        name: name
                            .unwrap_or_else(|| format!("{from}_{}_fkey", from_columns.join("_"))),
                        from: named(&database, &schema, from),
                        from_columns,
                        to: named(&database, &schema, to),
                        to_columns: strings(&referenced),
                    }
                })
                .collect())
        })
        .await
    }
}

impl DuckdbSession {
    async fn relation_graph(
        &self,
        id: &ObjectId,
        outgoing: bool,
    ) -> Result<Vec<ObjectId>, DriverError> {
        let Some((kind, parts)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        if !matches!(kind, "table" | "view") {
            return Ok(Vec::new());
        }
        self.with_conn(move |conn| {
            let Some((database, schema, table)) = relation_parts(&parts) else {
                return Ok(Vec::new());
            };
            let sql = if outgoing {
                "SELECT DISTINCT referenced_table FROM duckdb_constraints()
                 WHERE constraint_type = 'FOREIGN KEY' AND database_name = $1 AND schema_name = $2
                   AND table_name = $3 ORDER BY 1"
            } else {
                "SELECT DISTINCT table_name FROM duckdb_constraints()
                 WHERE constraint_type = 'FOREIGN KEY' AND database_name = $1 AND schema_name = $2
                   AND lower(referenced_table) = lower($3) ORDER BY 1"
            };
            let names = rows(conn, sql, params![database, schema, table], |row| {
                row.get::<_, String>(0)
            })?;
            Ok(names
                .iter()
                .map(|name| dk_id("table", &[database, schema, name]))
                .collect())
        })
        .await
    }
}

fn relation_id(
    conn: &Connection,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<ObjectId, DriverError> {
    let view = conn
        .query_row(
            "SELECT 1 FROM duckdb_views() WHERE database_name = $1 AND schema_name = $2 AND view_name = $3",
            params![database, schema, name],
            |_| Ok(()),
        )
        .optional()
        .map_err(map_error)?
        .is_some();
    Ok(dk_id(
        if view { "view" } else { "table" },
        &[database, schema, name],
    ))
}

/// The file's database first, then any ATTACH added. DuckDB's own `system` and `temp`
/// only with system objects.
fn databases(conn: &Connection, include_system: bool) -> Result<Vec<CatalogObject>, DriverError> {
    let found = rows(
        conn,
        "SELECT database_name, path, readonly FROM duckdb_databases()
         WHERE $1 OR NOT internal
         ORDER BY database_name <> current_database(), database_name",
        params![include_system],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, bool>(2)?,
            ))
        },
    )?;
    Ok(found
        .into_iter()
        .map(|(name, path, readonly)| {
            CatalogObject::new(
                dk_id("catalog", &[&name]),
                ObjectKind::Catalog,
                QualifiedName::new(None::<String>, None::<String>, name),
                None,
            )
            .with_attribute("driver.duckdb.path", json!(path))
            .with_attribute("driver.duckdb.readonly", json!(readonly))
        })
        .collect())
}

fn schemas(
    conn: &Connection,
    parent: &ObjectId,
    database: &str,
    include_system: bool,
) -> Result<Vec<CatalogObject>, DriverError> {
    let found = rows(
        conn,
        "SELECT schema_name, comment FROM duckdb_schemas()
         WHERE database_name = $1 AND ($2 OR schema_name NOT IN ('information_schema', 'pg_catalog'))
         ORDER BY schema_name <> 'main', schema_name",
        params![database, include_system],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    Ok(found
        .into_iter()
        .map(|(name, comment)| {
            let object = CatalogObject::new(
                dk_id("schema", &[database, &name]),
                ObjectKind::Schema,
                QualifiedName::new(Some(database), None::<String>, name),
                Some(parent.clone()),
            );
            with_comment(object, comment)
        })
        .collect())
}

fn with_comment(object: CatalogObject, comment: Option<String>) -> CatalogObject {
    match comment.filter(|comment| !comment.is_empty()) {
        Some(comment) => object.with_attribute("comment", json!(comment)),
        None => object,
    }
}

fn relations(
    conn: &Connection,
    parent: &ObjectId,
    database: &str,
    schema: &str,
    include_system: bool,
) -> Result<Vec<CatalogObject>, DriverError> {
    let found = rows(
        conn,
        "SELECT 'table', table_name, comment, estimated_size FROM duckdb_tables()
         WHERE database_name = $1 AND schema_name = $2 AND ($3 OR NOT internal)
         UNION ALL
         SELECT 'view', view_name, comment, NULL FROM duckdb_views()
         WHERE database_name = $1 AND schema_name = $2 AND ($3 OR NOT internal)
         ORDER BY 2",
        params![database, schema, include_system],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        },
    )?;
    Ok(found
        .into_iter()
        .map(|(kind, name, comment, estimated)| {
            let (key, kind) = if kind == "view" {
                ("view", ObjectKind::View)
            } else {
                ("table", ObjectKind::Table)
            };
            let mut object = CatalogObject::new(
                dk_id(key, &[database, schema, &name]),
                kind,
                named(database, schema, name),
                Some(parent.clone()),
            );
            if let Some(estimated) = estimated {
                object = object.with_attribute("driver.duckdb.estimated_rows", json!(estimated));
            }
            with_comment(object, comment)
        })
        .collect())
}

/// Columns, indexes and constraints of a table or view.
fn relation_children(
    conn: &Connection,
    parent: &ObjectId,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<CatalogObject>, DriverError> {
    let child = |kind: &str, name: &str| dk_id(kind, &[database, schema, table, name]);
    let place = params![database, schema, table];
    let constraints = rows(
        conn,
        "SELECT constraint_type, constraint_name, to_json(constraint_column_names)::VARCHAR,
                referenced_table, to_json(referenced_column_names)::VARCHAR, constraint_text
         FROM duckdb_constraints()
         WHERE database_name = $1 AND schema_name = $2 AND table_name = $3
           AND constraint_type IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY', 'CHECK')
         ORDER BY constraint_index",
        place,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        },
    )?;
    let primary: HashSet<String> = constraints
        .iter()
        .filter(|(kind, ..)| kind == "PRIMARY KEY")
        .flat_map(|(_, _, columns, ..)| strings(columns))
        .collect();

    let mut objects = Vec::new();
    let columns = rows(
        conn,
        "SELECT column_name, data_type, is_nullable, column_default, comment FROM duckdb_columns()
         WHERE database_name = $1 AND schema_name = $2 AND table_name = $3
         ORDER BY column_index",
        params![database, schema, table],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    for (name, data_type, nullable, default, comment) in columns {
        let mut object = CatalogObject::new(
            child("column", &name),
            ObjectKind::Column,
            named(database, schema, format!("{table}.{name}")),
            Some(parent.clone()),
        )
        .with_attribute("type", json!(data_type))
        .with_attribute("driver.duckdb.not_null", json!(!nullable));
        if primary.contains(&name) {
            object = object.with_attribute("driver.duckdb.primary_key", json!(true));
        }
        if let Some(default) = default {
            object = object.with_attribute("driver.duckdb.default", json!(default));
        }
        objects.push(with_comment(object, comment));
    }

    let indexes = rows(
        conn,
        "SELECT index_name, is_unique, expressions FROM duckdb_indexes()
         WHERE database_name = $1 AND schema_name = $2 AND table_name = $3
         ORDER BY index_name",
        params![database, schema, table],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    for (name, unique, expressions) in indexes {
        objects.push(
            CatalogObject::new(
                child("index", &name),
                ObjectKind::Index,
                named(database, schema, name.clone()),
                Some(parent.clone()),
            )
            .with_attribute("driver.duckdb.unique", json!(unique))
            .with_attribute("driver.duckdb.expressions", json!(expressions)),
        );
    }

    for (kind, name, columns, referenced_table, referenced, text) in constraints {
        let local = strings(&columns);
        let name = name.unwrap_or_else(|| {
            let suffix = match kind.as_str() {
                "PRIMARY KEY" => "pkey",
                "UNIQUE" => "key",
                "FOREIGN KEY" => "fkey",
                _ => "check",
            };
            format!("{table}_{}_{suffix}", local.join("_"))
        });
        let mut object = CatalogObject::new(
            child("constraint", &name),
            ObjectKind::Constraint,
            named(database, schema, name.clone()),
            Some(parent.clone()),
        )
        .with_attribute("driver.duckdb.constraint_type", json!(kind))
        .with_attribute("driver.duckdb.definition", json!(text));
        if let Some(referenced_table) = referenced_table.filter(|_| kind == "FOREIGN KEY") {
            object = object
                .with_attribute("fk_local", json!(local))
                .with_attribute(
                    "fk_referenced",
                    json!(referenced.as_deref().map(strings).unwrap_or_default()),
                )
                .with_attribute("fk_table", json!(referenced_table))
                .with_attribute("fk_schema", json!(schema));
        }
        objects.push(object);
    }
    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::{dk_id, parse_id};

    #[test]
    fn ids_keep_names_with_slashes() {
        let id = dk_id("table", &["shop", "a/b", "100%"]);
        let (kind, parts) = parse_id(&id).unwrap();
        assert_eq!(kind, "table");
        assert_eq!(parts, ["shop", "a/b", "100%"]);
    }
}
