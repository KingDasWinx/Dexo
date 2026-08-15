use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, CatalogRestriction, DriverError,
    ObjectDdl, ObjectId, ObjectKind, QualifiedName,
};

use crate::error::{is_permission, map_error};
use crate::session::PostgresSession;

const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

fn pg_id(kind: &str, key: impl std::fmt::Display) -> ObjectId {
    ObjectId::new(format!("pg:{kind}:{key}"))
}

fn parse_id(id: &ObjectId) -> Option<(&str, &str)> {
    id.as_str()
        .strip_prefix("pg:")
        .and_then(|rest| rest.split_once(':'))
}

fn oid_attr(oid: i64) -> (String, serde_json::Value) {
    ("driver.postgres.oid".into(), serde_json::json!(oid))
}

fn is_system_schema(name: &str) -> bool {
    SYSTEM_SCHEMAS.contains(&name) || name.starts_with("pg_temp") || name.starts_with("pg_toast")
}

fn relkind_to_kind(relkind: &str) -> ObjectKind {
    match relkind {
        "r" | "p" => ObjectKind::Table,
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        "S" => ObjectKind::Sequence,
        "i" => ObjectKind::Index,
        _ => ObjectKind::DriverSpecific(relkind.to_string()),
    }
}

fn relkind_key(relkind: &str) -> &'static str {
    match relkind {
        "r" | "p" => "table",
        "v" => "view",
        "m" => "materialized_view",
        "S" => "sequence",
        "i" => "index",
        _ => "class",
    }
}

impl PostgresSession {
    async fn current_catalog(&self) -> Result<CatalogObject, DriverError> {
        let row = self
            .client
            .query_one(
                "SELECT d.oid::bigint, d.datname::text FROM pg_database d WHERE d.datname = current_database()",
                &[],
            )
            .await
            .map_err(map_error)?;
        let oid: i64 = row.get(0);
        let name: String = row.get(1);
        Ok(CatalogObject::new(
            pg_id("catalog", oid),
            ObjectKind::Catalog,
            QualifiedName::new(Some(name.clone()), None::<String>, name),
            None,
        )
        .with_attribute(oid_attr(oid).0, oid_attr(oid).1))
    }

    async fn list_schemas(
        &self,
        parent: &ObjectId,
        catalog: &str,
        include_system: bool,
    ) -> Result<Vec<CatalogObject>, DriverError> {
        let rows = self
            .client
            .query(
                "SELECT n.oid::bigint, n.nspname::text FROM pg_namespace n ORDER BY n.nspname",
                &[],
            )
            .await
            .map_err(map_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let oid: i64 = row.get(0);
                let name: String = row.get(1);
                if !include_system && is_system_schema(&name) {
                    return None;
                }
                Some(
                    CatalogObject::new(
                        pg_id("schema", oid),
                        ObjectKind::Schema,
                        QualifiedName::new(Some(catalog), Some(name.clone()), name),
                        Some(parent.clone()),
                    )
                    .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
                )
            })
            .collect())
    }

    async fn list_roles(
        &self,
        parent: &ObjectId,
        catalog: &str,
    ) -> Result<(Vec<CatalogObject>, Option<CatalogRestriction>), DriverError> {
        match self
            .client
            .query(
                "SELECT oid::bigint, rolname::text, rolcanlogin FROM pg_roles ORDER BY rolname",
                &[],
            )
            .await
        {
            Ok(rows) => Ok((
                rows.into_iter()
                    .map(|row| {
                        let oid: i64 = row.get(0);
                        let name: String = row.get(1);
                        let can_login: bool = row.get(2);
                        let kind = if can_login {
                            ObjectKind::User
                        } else {
                            ObjectKind::Role
                        };
                        let key = if can_login { "user" } else { "role" };
                        CatalogObject::new(
                            pg_id(key, oid),
                            kind,
                            QualifiedName::new(Some(catalog), None::<String>, name),
                            Some(parent.clone()),
                        )
                        .with_attribute(oid_attr(oid).0, oid_attr(oid).1)
                    })
                    .collect(),
                None,
            )),
            Err(error) if is_permission(&error) => Ok((
                Vec::new(),
                Some(CatalogRestriction {
                    parent: Some(parent.clone()),
                    capability: "postgres.roles".into(),
                    reason: error.to_string(),
                }),
            )),
            Err(error) => Err(map_error(error)),
        }
    }

    async fn list_driver_objects(
        &self,
        parent: &ObjectId,
        catalog: &str,
        sql: &str,
        kind: &str,
    ) -> Result<(Vec<CatalogObject>, Option<CatalogRestriction>), DriverError> {
        match self.client.query(sql, &[]).await {
            Ok(rows) => Ok((
                rows.into_iter()
                    .map(|row| {
                        let oid: i64 = row.get(0);
                        let name: String = row.get(1);
                        CatalogObject::new(
                            pg_id(kind, oid),
                            ObjectKind::DriverSpecific(kind.to_string()),
                            QualifiedName::new(Some(catalog), None::<String>, name),
                            Some(parent.clone()),
                        )
                        .with_attribute(oid_attr(oid).0, oid_attr(oid).1)
                    })
                    .collect(),
                None,
            )),
            Err(error) if is_permission(&error) => Ok((
                Vec::new(),
                Some(CatalogRestriction {
                    parent: Some(parent.clone()),
                    capability: format!("postgres.{kind}"),
                    reason: error.to_string(),
                }),
            )),
            Err(error) => Err(map_error(error)),
        }
    }

    async fn list_schema_children(
        &self,
        parent: &ObjectId,
        schema_oid: i64,
        catalog: &str,
        schema: &str,
    ) -> Result<CatalogList, DriverError> {
        let allowed: bool = match self
            .client
            .query_one(
                "SELECT has_schema_privilege($1::bigint::oid, 'USAGE')",
                &[&schema_oid],
            )
            .await
        {
            Ok(row) => row.get(0),
            Err(error) if is_permission(&error) => {
                return Ok(CatalogList {
                    objects: Vec::new(),
                    restrictions: vec![CatalogRestriction {
                        parent: Some(parent.clone()),
                        capability: "schema.usage".into(),
                        reason: error.to_string(),
                    }],
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        if !allowed {
            return Ok(CatalogList {
                objects: Vec::new(),
                restrictions: vec![CatalogRestriction {
                    parent: Some(parent.clone()),
                    capability: "schema.usage".into(),
                    reason: format!("permission denied for schema {schema}"),
                }],
            });
        }
        let mut objects = Vec::new();
        let classes = self
            .client
            .query(
                "SELECT c.oid::bigint, c.relname::text, c.relkind::text, pg_get_partkeydef(c.oid)
                 FROM pg_class c
                 WHERE c.relnamespace = $1::bigint::oid
                   AND c.relkind IN ('r','p','v','m','S')
                   AND NOT c.relispartition
                 ORDER BY c.relname",
                &[&schema_oid],
            )
            .await
            .map_err(map_error)?;
        for row in classes {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            let relkind: String = row.get(2);
            let partkey: Option<String> = row.get(3);
            let mut object = CatalogObject::new(
                pg_id(relkind_key(&relkind), oid),
                relkind_to_kind(&relkind),
                QualifiedName::new(Some(catalog), Some(schema), name),
                Some(parent.clone()),
            )
            .with_attribute(oid_attr(oid).0, oid_attr(oid).1)
            .with_attribute("driver.postgres.relkind", serde_json::json!(relkind));
            if let Some(partkey) = partkey.filter(|value| !value.is_empty()) {
                object = object
                    .with_attribute("driver.postgres.partition_key", serde_json::json!(partkey));
            }
            objects.push(object);
        }

        let routines = self
            .client
            .query(
                "SELECT p.oid::bigint, p.proname::text, p.prokind::text
                 FROM pg_proc p
                 WHERE p.pronamespace = $1::bigint::oid
                 ORDER BY p.proname",
                &[&schema_oid],
            )
            .await
            .map_err(map_error)?;
        for row in routines {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            let prokind: String = row.get(2);
            let (key, kind) = if prokind == "p" {
                ("procedure", ObjectKind::Procedure)
            } else {
                ("function", ObjectKind::Function)
            };
            objects.push(
                CatalogObject::new(
                    pg_id(key, oid),
                    kind,
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
            );
        }

        let types = self
            .client
            .query(
                "SELECT t.oid::bigint, t.typname::text, t.typtype::text
                 FROM pg_type t
                 WHERE t.typnamespace = $1::bigint::oid AND t.typtype IN ('e','d')
                 ORDER BY t.typname",
                &[&schema_oid],
            )
            .await
            .map_err(map_error)?;
        for row in types {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            let typtype: String = row.get(2);
            let kind = if typtype == "e" { "enum" } else { "domain" };
            objects.push(
                CatalogObject::new(
                    pg_id(kind, oid),
                    ObjectKind::DriverSpecific(kind.to_string()),
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
            );
        }
        Ok(CatalogList {
            objects,
            restrictions: vec![],
        })
    }

    async fn list_relation_children(
        &self,
        parent: &ObjectId,
        relid: i64,
        catalog: &str,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<CatalogObject>, DriverError> {
        let mut objects = Vec::new();
        let columns = self
            .client
            .query(
                "SELECT a.attnum::bigint, a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull
                 FROM pg_attribute a
                 WHERE a.attrelid = $1::bigint::oid AND a.attnum > 0 AND NOT a.attisdropped
                 ORDER BY a.attnum",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in columns {
            let attnum: i64 = row.get(0);
            let name: String = row.get(1);
            let type_name: String = row.get(2);
            let not_null: bool = row.get(3);
            objects.push(
                CatalogObject::new(
                    pg_id("column", format!("{relid}.{attnum}")),
                    ObjectKind::Column,
                    QualifiedName::new(Some(catalog), Some(schema), format!("{relation}.{name}")),
                    Some(parent.clone()),
                )
                .with_attribute("driver.postgres.attnum", serde_json::json!(attnum))
                .with_attribute("driver.postgres.type", serde_json::json!(type_name))
                .with_attribute("driver.postgres.not_null", serde_json::json!(not_null)),
            );
        }

        let indexes = self
            .client
            .query(
                "SELECT c.oid::bigint, c.relname::text
                 FROM pg_index i
                 JOIN pg_class c ON c.oid = i.indexrelid
                 WHERE i.indrelid = $1::bigint::oid
                 ORDER BY c.relname",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in indexes {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            objects.push(
                CatalogObject::new(
                    pg_id("index", oid),
                    ObjectKind::Index,
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
            );
        }

        let constraints = self
            .client
            .query(
                "SELECT oid::bigint, conname::text, contype::text
                 FROM pg_constraint WHERE conrelid = $1::bigint::oid ORDER BY conname",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in constraints {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            let contype: String = row.get(2);
            objects.push(
                CatalogObject::new(
                    pg_id("constraint", oid),
                    ObjectKind::Constraint,
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1)
                .with_attribute("driver.postgres.contype", serde_json::json!(contype)),
            );
        }

        let triggers = self
            .client
            .query(
                "SELECT oid::bigint, tgname::text FROM pg_trigger
                 WHERE tgrelid = $1::bigint::oid AND NOT tgisinternal ORDER BY tgname",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in triggers {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            objects.push(
                CatalogObject::new(
                    pg_id("trigger", oid),
                    ObjectKind::Trigger,
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
            );
        }

        let partitions = self
            .client
            .query(
                "SELECT c.oid::bigint, c.relname::text, pg_get_expr(c.relpartbound, c.oid)
                 FROM pg_inherits i
                 JOIN pg_class c ON c.oid = i.inhrelid
                 WHERE i.inhparent = $1::bigint::oid
                 ORDER BY c.relname",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in partitions {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            let bound: Option<String> = row.get(2);
            let mut object = CatalogObject::new(
                pg_id("partition", oid),
                ObjectKind::DriverSpecific("partition".into()),
                QualifiedName::new(Some(catalog), Some(schema), name),
                Some(parent.clone()),
            )
            .with_attribute(oid_attr(oid).0, oid_attr(oid).1);
            if let Some(bound) = bound {
                object = object
                    .with_attribute("driver.postgres.partition_bound", serde_json::json!(bound));
            }
            objects.push(object);
        }

        let policies = self
            .client
            .query(
                "SELECT oid::bigint, polname::text FROM pg_policy WHERE polrelid = $1::bigint::oid ORDER BY polname",
                &[&relid],
            )
            .await
            .map_err(map_error)?;
        for row in policies {
            let oid: i64 = row.get(0);
            let name: String = row.get(1);
            objects.push(
                CatalogObject::new(
                    pg_id("policy", oid),
                    ObjectKind::DriverSpecific("policy".into()),
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    Some(parent.clone()),
                )
                .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
            );
        }
        Ok(objects)
    }

    async fn relation_name(&self, oid: i64) -> Result<(String, String, String), DriverError> {
        let row = self
            .client
            .query_one(
                "SELECT current_database()::text, n.nspname::text, c.relname::text
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE c.oid = $1::bigint::oid",
                &[&oid],
            )
            .await
            .map_err(map_error)?;
        Ok((row.get(0), row.get(1), row.get(2)))
    }

    async fn schema_name(&self, oid: i64) -> Result<(String, String), DriverError> {
        let row = self
            .client
            .query_one(
                "SELECT current_database()::text, nspname::text FROM pg_namespace WHERE oid = $1::bigint::oid",
                &[&oid],
            )
            .await
            .map_err(map_error)?;
        Ok((row.get(0), row.get(1)))
    }

    async fn depend_ids(&self, oid: i64, outgoing: bool) -> Result<Vec<ObjectId>, DriverError> {
        let sql = if outgoing {
            "SELECT DISTINCT d.refobjid::bigint,
                    CASE c.relname
                      WHEN 'pg_class' THEN COALESCE((
                        SELECT CASE relkind
                          WHEN 'r' THEN 'table' WHEN 'p' THEN 'table'
                          WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view'
                          WHEN 'S' THEN 'sequence' WHEN 'i' THEN 'index'
                          ELSE 'class' END
                        FROM pg_class obj WHERE obj.oid = d.refobjid), 'table')
                      WHEN 'pg_proc' THEN COALESCE((
                        SELECT CASE prokind WHEN 'p' THEN 'procedure' ELSE 'function' END
                        FROM pg_proc obj WHERE obj.oid = d.refobjid), 'function')
                      WHEN 'pg_namespace' THEN 'schema'
                      WHEN 'pg_constraint' THEN 'constraint'
                      WHEN 'pg_trigger' THEN 'trigger'
                      WHEN 'pg_type' THEN 'type'
                      WHEN 'pg_authid' THEN 'role'
                      ELSE NULL
                    END
             FROM pg_depend d
             JOIN pg_class c ON c.oid = d.refclassid
             WHERE d.deptype <> 'i'
               AND (d.objid = $1::bigint::oid
                    OR d.objid IN (SELECT r.oid FROM pg_rewrite r WHERE r.ev_class = $1::bigint::oid))"
        } else {
            "SELECT DISTINCT
                    CASE WHEN c.relname = 'pg_rewrite'
                         THEN (SELECT ev_class::bigint FROM pg_rewrite r WHERE r.oid = d.objid)
                         ELSE d.objid::bigint END,
                    CASE c.relname
                      WHEN 'pg_class' THEN COALESCE((
                        SELECT CASE relkind
                          WHEN 'r' THEN 'table' WHEN 'p' THEN 'table'
                          WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view'
                          WHEN 'S' THEN 'sequence' WHEN 'i' THEN 'index'
                          ELSE 'class' END
                        FROM pg_class obj WHERE obj.oid = d.objid), 'table')
                      WHEN 'pg_rewrite' THEN COALESCE((
                        SELECT CASE relkind
                          WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view'
                          ELSE 'table' END
                        FROM pg_rewrite r JOIN pg_class obj ON obj.oid = r.ev_class
                        WHERE r.oid = d.objid), 'view')
                      WHEN 'pg_proc' THEN COALESCE((
                        SELECT CASE prokind WHEN 'p' THEN 'procedure' ELSE 'function' END
                        FROM pg_proc obj WHERE obj.oid = d.objid), 'function')
                      WHEN 'pg_namespace' THEN 'schema'
                      WHEN 'pg_constraint' THEN 'constraint'
                      WHEN 'pg_trigger' THEN 'trigger'
                      WHEN 'pg_type' THEN 'type'
                      WHEN 'pg_authid' THEN 'role'
                      ELSE NULL
                    END
             FROM pg_depend d
             JOIN pg_class c ON c.oid = d.classid
             WHERE d.deptype <> 'i' AND d.refobjid = $1::bigint::oid"
        };
        let rows = self.client.query(sql, &[&oid]).await.map_err(map_error)?;
        let mut ids: Vec<ObjectId> = rows
            .into_iter()
            .filter_map(|row| {
                let dep_oid: i64 = row.get(0);
                let kind: Option<String> = row.get(1);
                kind.filter(|kind| !kind.is_empty())
                    .map(|kind| pg_id(&kind, dep_oid))
            })
            .collect();
        let fk_sql = if outgoing {
            "SELECT confrelid::bigint FROM pg_constraint
             WHERE conrelid = $1::bigint::oid AND contype = 'f' AND confrelid <> 0"
        } else {
            "SELECT conrelid::bigint FROM pg_constraint
             WHERE confrelid = $1::bigint::oid AND contype = 'f'"
        };
        let fk_rows = self
            .client
            .query(fk_sql, &[&oid])
            .await
            .map_err(map_error)?;
        for row in fk_rows {
            let relid: i64 = row.get(0);
            if let Some(id) = self.class_object_id(relid).await?
                && !ids.iter().any(|existing| existing == &id)
            {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    async fn class_object_id(&self, oid: i64) -> Result<Option<ObjectId>, DriverError> {
        let row = self
            .client
            .query_opt(
                "SELECT CASE relkind
                   WHEN 'r' THEN 'table' WHEN 'p' THEN 'table'
                   WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view'
                   WHEN 'S' THEN 'sequence' WHEN 'i' THEN 'index'
                   ELSE 'class' END
                 FROM pg_class WHERE oid = $1::bigint::oid",
                &[&oid],
            )
            .await
            .map_err(map_error)?;
        Ok(row.map(|row| {
            let kind: String = row.get(0);
            pg_id(&kind, oid)
        }))
    }
}

#[async_trait::async_trait]
impl CatalogReader for PostgresSession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        let Some(parent) = parent else {
            return Ok(CatalogList {
                objects: vec![self.current_catalog().await?],
                restrictions: vec![],
            });
        };
        let catalog = self.current_catalog().await?;
        let catalog_name = catalog.qualified_name.object().to_string();
        let Some((kind, key)) = parse_id(parent) else {
            return Ok(CatalogList::default());
        };
        match kind {
            "catalog" => {
                let mut objects = self
                    .list_schemas(parent, &catalog_name, options.include_system)
                    .await?;
                let mut restrictions = Vec::new();
                let (roles, restriction) = self.list_roles(parent, &catalog_name).await?;
                objects.extend(roles);
                if let Some(restriction) = restriction {
                    restrictions.push(restriction);
                }
                for (sql, kind) in [
                    (
                        "SELECT oid::bigint, extname::text FROM pg_extension ORDER BY extname",
                        "extension",
                    ),
                    (
                        "SELECT oid::bigint, fdwname::text FROM pg_foreign_data_wrapper ORDER BY fdwname",
                        "fdw",
                    ),
                    (
                        "SELECT oid::bigint, pubname::text FROM pg_publication ORDER BY pubname",
                        "publication",
                    ),
                ] {
                    let (extra, restriction) = self
                        .list_driver_objects(parent, &catalog_name, sql, kind)
                        .await?;
                    objects.extend(extra);
                    if let Some(restriction) = restriction {
                        restrictions.push(restriction);
                    }
                }
                Ok(CatalogList {
                    objects,
                    restrictions,
                })
            }
            "schema" => {
                let oid: i64 = key.parse().unwrap_or(0);
                let (catalog_name, schema) = self.schema_name(oid).await?;
                self.list_schema_children(parent, oid, &catalog_name, &schema)
                    .await
            }
            "table" | "view" | "materialized_view" | "partition" => {
                let oid: i64 = key.parse().unwrap_or(0);
                let (catalog_name, schema, relation) = self.relation_name(oid).await?;
                Ok(CatalogList {
                    objects: self
                        .list_relation_children(parent, oid, &catalog_name, &schema, &relation)
                        .await?,
                    restrictions: vec![],
                })
            }
            _ => Ok(CatalogList::default()),
        }
    }

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError> {
        let list = self
            .list_children(None, &CatalogListOptions::default())
            .await?;
        if let Some(found) = list.objects.into_iter().find(|object| object.id == *id) {
            return Ok(Some(found));
        }
        let Some((kind, key)) = parse_id(id) else {
            return Ok(None);
        };
        if kind == "column" {
            let (relid, _) = key.split_once('.').unwrap_or((key, "0"));
            let oid: i64 = relid.parse().unwrap_or(0);
            let children = self
                .list_children(Some(&pg_id("table", oid)), &CatalogListOptions::default())
                .await?;
            return Ok(children.objects.into_iter().find(|object| object.id == *id));
        }
        let oid: i64 = key.parse().unwrap_or(0);
        match kind {
            "schema" => {
                let (catalog, schema) = self.schema_name(oid).await?;
                let catalog_obj = self.current_catalog().await?;
                Ok(Some(
                    CatalogObject::new(
                        id.clone(),
                        ObjectKind::Schema,
                        QualifiedName::new(Some(catalog), Some(schema.clone()), schema),
                        Some(catalog_obj.id),
                    )
                    .with_attribute(oid_attr(oid).0, oid_attr(oid).1),
                ))
            }
            "table" | "view" | "materialized_view" | "sequence" | "index" | "partition" => {
                let (catalog, schema, name) = self.relation_name(oid).await?;
                Ok(Some(CatalogObject::new(
                    id.clone(),
                    relkind_to_kind(match kind {
                        "view" => "v",
                        "materialized_view" => "m",
                        "sequence" => "S",
                        "index" => "i",
                        _ => "r",
                    }),
                    QualifiedName::new(Some(catalog), Some(schema), name),
                    None,
                )))
            }
            _ => Ok(None),
        }
    }

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let sql = match kind {
            "table" | "partition" => {
                let oid: i64 = key.parse().unwrap_or(0);
                self.client
                    .query_one(
                        "SELECT 'CREATE TABLE ' || quote_ident(n.nspname) || '.' || quote_ident(c.relname) || E' (\\n' ||
                                coalesce(string_agg('  ' || quote_ident(a.attname) || ' ' || format_type(a.atttypid, a.atttypmod), E',\\n' ORDER BY a.attnum), '') ||
                                E'\\n);'
                         FROM pg_class c
                         JOIN pg_namespace n ON n.oid = c.relnamespace
                         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
                         WHERE c.oid = $1::bigint::oid
                         GROUP BY n.nspname, c.relname",
                        &[&oid],
                    )
                    .await
                    .map_err(map_error)?
                    .get::<_, String>(0)
            }
            "view" | "materialized_view" => {
                let oid: i64 = key.parse().unwrap_or(0);
                let def: String = self
                    .client
                    .query_one("SELECT pg_get_viewdef($1::bigint::oid, true)", &[&oid])
                    .await
                    .map_err(map_error)?
                    .get(0);
                let prefix = if kind == "view" {
                    "CREATE VIEW"
                } else {
                    "CREATE MATERIALIZED VIEW"
                };
                let (_, schema, name) = self.relation_name(oid).await?;
                format!("{prefix} {schema}.{name} AS {def}")
            }
            "function" | "procedure" => {
                let oid: i64 = key.parse().unwrap_or(0);
                self.client
                    .query_one("SELECT pg_get_functiondef($1::bigint::oid)", &[&oid])
                    .await
                    .map_err(map_error)?
                    .get(0)
            }
            "index" => {
                let oid: i64 = key.parse().unwrap_or(0);
                self.client
                    .query_one("SELECT pg_get_indexdef($1::bigint::oid)", &[&oid])
                    .await
                    .map_err(map_error)?
                    .get(0)
            }
            "trigger" => {
                let oid: i64 = key.parse().unwrap_or(0);
                self.client
                    .query_one("SELECT pg_get_triggerdef($1::bigint::oid)", &[&oid])
                    .await
                    .map_err(map_error)?
                    .get(0)
            }
            "constraint" => {
                let oid: i64 = key.parse().unwrap_or(0);
                self.client
                    .query_one("SELECT pg_get_constraintdef($1::bigint::oid)", &[&oid])
                    .await
                    .map_err(map_error)?
                    .get(0)
            }
            "sequence" => {
                let oid: i64 = key.parse().unwrap_or(0);
                let (_, schema, name) = self.relation_name(oid).await?;
                format!("CREATE SEQUENCE {schema}.{name}")
            }
            _ => {
                return Err(DriverError::unsupported(format!(
                    "ddl unavailable for {kind}"
                )));
            }
        };
        Ok(ObjectDdl {
            object_id: id.clone(),
            sql,
        })
    }

    async fn dependencies(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        let Some((_, key)) = parse_id(id) else {
            return Ok(Vec::new());
        };
        let oid: i64 = key
            .split('.')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        self.depend_ids(oid, true).await
    }

    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        let Some((_, key)) = parse_id(id) else {
            return Ok(Vec::new());
        };
        let oid: i64 = key
            .split('.')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        self.depend_ids(oid, false).await
    }
}
