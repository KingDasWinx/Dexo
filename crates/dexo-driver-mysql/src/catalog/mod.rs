use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, CatalogRestriction, DriverError,
    DriverErrorCategory, ObjectDdl, ObjectId, ObjectKind, QualifiedName,
};
use mysql_async::prelude::Queryable;

use crate::error::{is_permission, map_error};
use crate::session::MysqlSession;

const SYSTEM_SCHEMAS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

fn my_id(kind: &str, key: impl std::fmt::Display) -> ObjectId {
    ObjectId::new(format!("my:{kind}:{key}"))
}

fn parse_id(id: &ObjectId) -> Option<(&str, &str)> {
    id.as_str()
        .strip_prefix("my:")
        .and_then(|rest| rest.split_once(':'))
}

fn quote(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

fn is_system_schema(name: &str) -> bool {
    SYSTEM_SCHEMAS
        .iter()
        .any(|schema| schema.eq_ignore_ascii_case(name))
}

impl MysqlSession {
    async fn current_schema(&self) -> Result<String, DriverError> {
        let mut conn = self.conn.lock().await;
        let row: Option<(Option<String>,)> = conn
            .query_first("SELECT DATABASE()")
            .await
            .map_err(map_error)?;
        row.and_then(|row| row.0).ok_or_else(|| {
            DriverError::new(DriverErrorCategory::Configuration, "no database selected")
        })
    }

    async fn exec_rows<T>(
        &self,
        sql: &str,
        params: impl Into<mysql_async::Params> + Send,
    ) -> Result<Vec<T>, DriverError>
    where
        T: mysql_async::prelude::FromRow + Send + 'static,
    {
        let mut conn = self.conn.lock().await;
        conn.exec(sql, params).await.map_err(map_error)
    }

    async fn try_exec_rows<T>(
        &self,
        sql: &str,
        params: impl Into<mysql_async::Params> + Send,
    ) -> Result<Result<Vec<T>, String>, DriverError>
    where
        T: mysql_async::prelude::FromRow + Send + 'static,
    {
        let mut conn = self.conn.lock().await;
        match conn.exec(sql, params).await {
            Ok(rows) => Ok(Ok(rows)),
            Err(error) if is_permission(&error) => Ok(Err("permission denied".into())),
            Err(error) => Err(map_error(error)),
        }
    }
}

#[async_trait::async_trait]
impl CatalogReader for MysqlSession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        let current = self.current_schema().await?;
        let Some(parent) = parent else {
            let object = CatalogObject::new(
                my_id("catalog", &current),
                ObjectKind::Catalog,
                QualifiedName::new(Some(current.clone()), None::<String>, current.clone()),
                None,
            );
            let charset: Vec<mysql_async::Row> = self
                .exec_rows(
                    "SELECT SCHEMA_NAME, DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME
                     FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
                    (current.clone(),),
                )
                .await
                .unwrap_or_default();
            let object = if let Some(row) = charset.into_iter().next() {
                object
                    .with_attribute(
                        "driver.mysql.charset",
                        serde_json::json!(cell_string(&row, 1)),
                    )
                    .with_attribute(
                        "driver.mysql.collation",
                        serde_json::json!(cell_string(&row, 2)),
                    )
            } else {
                object
            };
            return Ok(CatalogList {
                objects: vec![object],
                restrictions: vec![],
            });
        };
        let Some((kind, key)) = parse_id(parent) else {
            return Ok(CatalogList::default());
        };
        match kind {
            "catalog" => self.list_catalog_children(parent, key, options).await,
            "table" | "view" => self.list_table_children(parent, key).await,
            _ => Ok(CatalogList::default()),
        }
    }

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Ok(None);
        };
        if kind == "catalog" {
            let list = self
                .list_children(None, &CatalogListOptions::default())
                .await?;
            return Ok(list.objects.into_iter().find(|object| object.id == *id));
        }
        let parent = match kind {
            "table" | "view" | "function" | "procedure" | "event" => {
                key.split('/').next().map(|schema| my_id("catalog", schema))
            }
            "column" | "index" | "constraint" | "trigger" | "partition" => {
                let mut parts = key.split('/');
                match (parts.next(), parts.next()) {
                    (Some(schema), Some(table)) => {
                        Some(my_id("table", format!("{schema}/{table}")))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(parent) = parent {
            let list = self
                .list_children(Some(&parent), &CatalogListOptions::default())
                .await?;
            return Ok(list.objects.into_iter().find(|object| object.id == *id));
        }
        Ok(None)
    }

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let sql = match kind {
            "table" | "view" => {
                let (schema, name) = split2(key);
                self.show_create("TABLE", schema, name).await?
            }
            "function" => {
                let (schema, name) = split2(key);
                self.show_create("FUNCTION", schema, name).await?
            }
            "procedure" => {
                let (schema, name) = split2(key);
                self.show_create("PROCEDURE", schema, name).await?
            }
            "trigger" => {
                let (schema, table, name) = split3(key);
                let _ = (schema, table);
                self.show_create_ident("TRIGGER", name).await?
            }
            "event" => {
                let (schema, name) = split2(key);
                self.show_create("EVENT", schema, name).await?
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
        self.relation_graph(id, true).await
    }

    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        self.relation_graph(id, false).await
    }

    async fn foreign_keys(
        &self,
        table: &QualifiedName,
    ) -> Result<Vec<dexo_driver_api::ForeignKeyRef>, DriverError> {
        let schema = table
            .schema()
            .or(table.catalog())
            .unwrap_or_default()
            .to_string();
        let name = table.object().to_string();
        let rows: Vec<(String, String, String, String, String, String, String)> = self
            .exec_rows(
                "SELECT k.CONSTRAINT_NAME, k.TABLE_SCHEMA, k.TABLE_NAME, k.COLUMN_NAME,
                        k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME
                 FROM information_schema.KEY_COLUMN_USAGE k
                 WHERE k.REFERENCED_TABLE_NAME IS NOT NULL
                   AND ((k.TABLE_SCHEMA = COALESCE(NULLIF(?, ''), DATABASE()) AND k.TABLE_NAME = ?)
                        OR (k.REFERENCED_TABLE_SCHEMA = COALESCE(NULLIF(?, ''), DATABASE())
                            AND k.REFERENCED_TABLE_NAME = ?))
                 ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
                (schema.clone(), name.clone(), schema, name),
            )
            .await?;
        // One row per column: a key's rows are consecutive, its columns in order. Each
        // end is named `database.table` as the app names a table opened from the
        // sidebar -- the database as the schema -- so the tab already open for it is
        // found instead of a second one opened.
        let mut keys: Vec<dexo_driver_api::ForeignKeyRef> = Vec::new();
        for (constraint, from_schema, from_table, from_column, to_schema, to_table, to_column) in
            rows
        {
            let from = QualifiedName::new(None::<String>, Some(from_schema), from_table);
            match keys.last_mut() {
                Some(key) if key.name == constraint && key.from == from => {
                    key.from_columns.push(from_column);
                    key.to_columns.push(to_column);
                }
                _ => keys.push(dexo_driver_api::ForeignKeyRef {
                    name: constraint,
                    from,
                    from_columns: vec![from_column],
                    to: QualifiedName::new(None::<String>, Some(to_schema), to_table),
                    to_columns: vec![to_column],
                }),
            }
        }
        Ok(keys)
    }

    async fn databases(&self) -> Result<Vec<String>, DriverError> {
        self.exec_rows(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY 1",
            (),
        )
        .await
    }

    async fn relations_named(
        &self,
        schema: Option<&str>,
        name: &str,
    ) -> Result<Option<Vec<CatalogObject>>, DriverError> {
        // A name without a database is the current database's, as a statement reads it.
        let rows: Vec<(String, String, String)> = self
            .exec_rows(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES
                 WHERE LOWER(TABLE_NAME) = LOWER(?)
                   AND LOWER(TABLE_SCHEMA) = LOWER(COALESCE(?, DATABASE()))
                 ORDER BY TABLE_SCHEMA, TABLE_NAME",
                (name.to_string(), schema.map(str::to_string)),
            )
            .await?;
        Ok(Some(
            rows.into_iter()
                .map(|(schema, name, table_type)| {
                    let is_view = table_type.eq_ignore_ascii_case("VIEW")
                        || table_type.eq_ignore_ascii_case("SYSTEM VIEW");
                    let (key, kind) = if is_view {
                        ("view", ObjectKind::View)
                    } else {
                        ("table", ObjectKind::Table)
                    };
                    CatalogObject::new(
                        my_id(key, format!("{schema}/{name}")),
                        kind,
                        QualifiedName::new(Some(schema.clone()), None::<String>, name),
                        Some(my_id("catalog", &schema)),
                    )
                })
                .collect(),
        ))
    }
}

impl MysqlSession {
    async fn list_catalog_children(
        &self,
        parent: &ObjectId,
        schema: &str,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        if !options.include_system && is_system_schema(schema) {
            return Ok(CatalogList::default());
        }
        let mut objects = Vec::new();
        let mut restrictions = Vec::new();
        let tables: Vec<mysql_async::Row> = self
            .exec_rows(
                "SELECT TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_COLLATION, TABLE_COMMENT
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = ?",
                (schema.to_string(),),
            )
            .await?;
        for row in tables {
            let name = cell_string(&row, 0);
            let table_type = cell_string(&row, 1);
            let engine = cell_opt(&row, 2);
            let collation = cell_opt(&row, 3);
            let comment = cell_string(&row, 4);
            let is_view = table_type.eq_ignore_ascii_case("VIEW")
                || table_type.eq_ignore_ascii_case("SYSTEM VIEW");
            let kind = if is_view {
                ObjectKind::View
            } else {
                ObjectKind::Table
            };
            let key = if is_view { "view" } else { "table" };
            let mut object = CatalogObject::new(
                my_id(key, format!("{schema}/{name}")),
                kind,
                QualifiedName::new(Some(schema), None::<String>, name),
                Some(parent.clone()),
            );
            if let Some(engine) = engine {
                object = object.with_attribute("driver.mysql.engine", serde_json::json!(engine));
            }
            if let Some(collation) = collation {
                object =
                    object.with_attribute("driver.mysql.collation", serde_json::json!(collation));
            }
            // A view's comment is the word VIEW; only a table's says anything.
            if !is_view && !comment.trim().is_empty() {
                object = object.with_attribute("comment", serde_json::json!(comment));
            }
            objects.push(object);
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ?",
                (schema.to_string(),),
            )
            .await?
        {
            Ok(routines) => {
                for row in routines {
                    let name = cell_string(&row, 0);
                    let routine_type = cell_string(&row, 1);
                    let (key, kind) = if routine_type.eq_ignore_ascii_case("PROCEDURE") {
                        ("procedure", ObjectKind::Procedure)
                    } else {
                        ("function", ObjectKind::Function)
                    };
                    objects.push(CatalogObject::new(
                        my_id(key, format!("{schema}/{name}")),
                        kind,
                        QualifiedName::new(Some(schema), None::<String>, name),
                        Some(parent.clone()),
                    ));
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.routines".into(),
                reason,
            }),
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA = ?",
                (schema.to_string(),),
            )
            .await?
        {
            Ok(events) => {
                for row in events {
                    let name = cell_string(&row, 0);
                    objects.push(CatalogObject::new(
                        my_id("event", format!("{schema}/{name}")),
                        ObjectKind::DriverSpecific("event".into()),
                        QualifiedName::new(Some(schema), None::<String>, name),
                        Some(parent.clone()),
                    ));
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.events".into(),
                reason,
            }),
        }

        match self
            .try_exec_rows::<mysql_async::Row>("SELECT User, Host FROM mysql.user", ())
            .await?
        {
            Ok(users) => {
                for row in users {
                    let user = cell_string(&row, 0);
                    let host = cell_string(&row, 1);
                    objects.push(
                        CatalogObject::new(
                            my_id("user", format!("{host}/{user}")),
                            ObjectKind::User,
                            QualifiedName::new(Some(schema), None::<String>, user),
                            Some(parent.clone()),
                        )
                        .with_attribute("driver.mysql.host", serde_json::json!(host)),
                    );
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.users".into(),
                reason,
            }),
        }

        // MariaDB keeps roles as users flagged `is_role`; it has no role_edges.
        let roles_sql = if self.is_mariadb() {
            "SELECT User FROM mysql.user WHERE is_role = 'Y'"
        } else {
            "SELECT from_user FROM mysql.role_edges"
        };
        match self
            .try_exec_rows::<mysql_async::Row>(roles_sql, ())
            .await?
        {
            Ok(roles) => {
                for row in roles {
                    let role = cell_string(&row, 0);
                    objects.push(CatalogObject::new(
                        my_id("role", &role),
                        ObjectKind::Role,
                        QualifiedName::new(Some(schema), None::<String>, role),
                        Some(parent.clone()),
                    ));
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.roles".into(),
                reason,
            }),
        }

        Ok(CatalogList {
            objects,
            restrictions,
        })
    }

    async fn list_table_children(
        &self,
        parent: &ObjectId,
        key: &str,
    ) -> Result<CatalogList, DriverError> {
        let (schema, table) = split2(key);
        let mut objects = Vec::new();
        let mut restrictions = Vec::new();
        let columns: Vec<mysql_async::Row> = self
            .exec_rows(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, GENERATION_EXPRESSION, EXTRA, COLLATION_NAME,
                        COLUMN_COMMENT
                 FROM information_schema.COLUMNS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                 ORDER BY ORDINAL_POSITION",
                (schema.to_string(), table.to_string()),
            )
            .await?;
        for row in columns {
            let name = cell_string(&row, 0);
            let data_type = cell_string(&row, 1);
            let nullable = cell_string(&row, 2);
            let generated = cell_opt(&row, 3);
            let extra = cell_string(&row, 4);
            let collation = cell_opt(&row, 5);
            let comment = cell_string(&row, 6);
            let mut object = CatalogObject::new(
                my_id("column", format!("{schema}/{table}/{name}")),
                ObjectKind::Column,
                QualifiedName::new(Some(schema), None::<String>, format!("{table}.{name}")),
                Some(parent.clone()),
            )
            .with_attribute("type", serde_json::json!(data_type))
            .with_attribute("driver.mysql.nullable", serde_json::json!(nullable))
            .with_attribute("driver.mysql.extra", serde_json::json!(extra.clone()));
            if let Some(collation) = collation {
                object =
                    object.with_attribute("driver.mysql.collation", serde_json::json!(collation));
            }
            if let Some(generated) = generated.filter(|value| !value.is_empty()) {
                object = object.with_attribute(
                    "driver.mysql.generation_expression",
                    serde_json::json!(generated),
                );
            }
            if !comment.trim().is_empty() {
                object = object.with_attribute("comment", serde_json::json!(comment));
            }
            objects.push(object);
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT DISTINCT INDEX_NAME FROM information_schema.STATISTICS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (schema.to_string(), table.to_string()),
            )
            .await?
        {
            Ok(indexes) => {
                for row in indexes {
                    let name = cell_string(&row, 0);
                    objects.push(CatalogObject::new(
                        my_id("index", format!("{schema}/{table}/{name}")),
                        ObjectKind::Index,
                        QualifiedName::new(Some(schema), None::<String>, name),
                        Some(parent.clone()),
                    ));
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.indexes".into(),
                reason,
            }),
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (schema.to_string(), table.to_string()),
            )
            .await?
        {
            Ok(constraints) => {
                type FkParts = (Vec<String>, Vec<String>, Option<String>, Option<String>);
                let mut fk_map: std::collections::BTreeMap<String, FkParts> =
                    std::collections::BTreeMap::new();
                if let Ok(Ok(fks)) = self
                    .try_exec_rows::<mysql_async::Row>(
                        "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA,
                                REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
                         FROM information_schema.KEY_COLUMN_USAGE
                         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                           AND REFERENCED_TABLE_NAME IS NOT NULL
                         ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
                        (schema.to_string(), table.to_string()),
                    )
                    .await
                {
                    for row in fks {
                        let name = cell_string(&row, 0);
                        let entry =
                            fk_map
                                .entry(name)
                                .or_insert((Vec::new(), Vec::new(), None, None));
                        entry.0.push(cell_string(&row, 1));
                        entry.1.push(cell_string(&row, 4));
                        entry.2 = Some(cell_string(&row, 2));
                        entry.3 = Some(cell_string(&row, 3));
                    }
                }
                for row in constraints {
                    let name = cell_string(&row, 0);
                    let constraint_type = cell_string(&row, 1);
                    let mut object = CatalogObject::new(
                        my_id("constraint", format!("{schema}/{table}/{name}")),
                        ObjectKind::Constraint,
                        QualifiedName::new(Some(schema), None::<String>, name.clone()),
                        Some(parent.clone()),
                    )
                    .with_attribute(
                        "driver.mysql.constraint_type",
                        serde_json::json!(constraint_type),
                    );
                    if let Some((local, referenced, fk_schema, fk_table)) = fk_map.get(&name) {
                        object = object
                            .with_attribute("fk_local", serde_json::json!(local))
                            .with_attribute("fk_referenced", serde_json::json!(referenced))
                            .with_attribute("fk_schema", serde_json::json!(fk_schema))
                            .with_attribute("fk_table", serde_json::json!(fk_table))
                            .with_attribute("fk_catalog", serde_json::json!(schema));
                    }
                    objects.push(object);
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.constraints".into(),
                reason,
            }),
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS
                 WHERE EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ?",
                (schema.to_string(), table.to_string()),
            )
            .await?
        {
            Ok(triggers) => {
                for row in triggers {
                    let name = cell_string(&row, 0);
                    objects.push(CatalogObject::new(
                        my_id("trigger", format!("{schema}/{table}/{name}")),
                        ObjectKind::Trigger,
                        QualifiedName::new(Some(schema), None::<String>, name),
                        Some(parent.clone()),
                    ));
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.triggers".into(),
                reason,
            }),
        }

        match self
            .try_exec_rows::<mysql_async::Row>(
                "SELECT PARTITION_NAME, PARTITION_METHOD FROM information_schema.PARTITIONS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND PARTITION_NAME IS NOT NULL",
                (schema.to_string(), table.to_string()),
            )
            .await?
        {
            Ok(partitions) => {
                for row in partitions {
                    let name = cell_string(&row, 0);
                    let method = cell_opt(&row, 1);
                    let mut object = CatalogObject::new(
                        my_id("partition", format!("{schema}/{table}/{name}")),
                        ObjectKind::DriverSpecific("partition".into()),
                        QualifiedName::new(Some(schema), None::<String>, name),
                        Some(parent.clone()),
                    );
                    if let Some(method) = method {
                        object = object.with_attribute(
                            "driver.mysql.partition_method",
                            serde_json::json!(method),
                        );
                    }
                    objects.push(object);
                }
            }
            Err(reason) => restrictions.push(CatalogRestriction {
                parent: Some(parent.clone()),
                capability: "mysql.partitions".into(),
                reason,
            }),
        }

        Ok(CatalogList {
            objects,
            restrictions,
        })
    }

    async fn relation_graph(
        &self,
        id: &ObjectId,
        outgoing: bool,
    ) -> Result<Vec<ObjectId>, DriverError> {
        let Some((kind, key)) = parse_id(id) else {
            return Err(DriverError::unsupported("unknown catalog object"));
        };
        let mut ids = Vec::new();
        match kind {
            "table" | "view" => {
                let (schema, name) = split2(key);
                if outgoing && kind == "view" {
                    self.push_view_tables(&mut ids, schema, name).await?;
                }
                if outgoing {
                    self.push_fk_targets(&mut ids, schema, name).await?;
                } else {
                    self.push_fk_sources(&mut ids, schema, name).await?;
                    self.push_view_dependents(&mut ids, schema, name).await?;
                    self.push_trigger_dependents(&mut ids, schema, name).await?;
                }
            }
            "trigger" => {
                if outgoing {
                    let (schema, table, _) = split3(key);
                    push_unique(&mut ids, my_id("table", format!("{schema}/{table}")));
                }
            }
            "function" | "procedure" => {
                let (schema, name) = split2(key);
                if !outgoing {
                    self.push_routine_view_dependents(&mut ids, schema, name)
                        .await?;
                }
            }
            _ => {}
        }
        Ok(ids)
    }

    async fn require_rows(
        &self,
        sql: &str,
        params: impl Into<mysql_async::Params> + Send,
    ) -> Result<Vec<mysql_async::Row>, DriverError> {
        match self.try_exec_rows(sql, params).await? {
            Ok(rows) => Ok(rows),
            Err(reason) => Err(DriverError::new(DriverErrorCategory::Permission, reason)),
        }
    }

    async fn relation_id(&self, schema: &str, name: &str) -> Result<ObjectId, DriverError> {
        let rows = self
            .require_rows(
                "SELECT TABLE_TYPE FROM information_schema.TABLES
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        let table_type = rows
            .first()
            .map(|row| cell_string(row, 0))
            .unwrap_or_default();
        let is_view = table_type.eq_ignore_ascii_case("VIEW")
            || table_type.eq_ignore_ascii_case("SYSTEM VIEW");
        Ok(my_id(
            if is_view { "view" } else { "table" },
            format!("{schema}/{name}"),
        ))
    }

    async fn push_fk_targets(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let rows = self
            .require_rows(
                "SELECT DISTINCT REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME
                 FROM information_schema.KEY_COLUMN_USAGE
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        for row in rows {
            let ref_schema = cell_string(&row, 0);
            let ref_name = cell_string(&row, 1);
            let id = self.relation_id(&ref_schema, &ref_name).await?;
            push_unique(ids, id);
        }
        Ok(())
    }

    async fn push_fk_sources(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let rows = self
            .require_rows(
                "SELECT DISTINCT TABLE_SCHEMA, TABLE_NAME
                 FROM information_schema.KEY_COLUMN_USAGE
                 WHERE REFERENCED_TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        for row in rows {
            let src_schema = cell_string(&row, 0);
            let src_name = cell_string(&row, 1);
            let id = self.relation_id(&src_schema, &src_name).await?;
            push_unique(ids, id);
        }
        Ok(())
    }

    async fn push_view_tables(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        if self.is_mariadb() {
            // MariaDB has no VIEW_TABLE_USAGE; its stored definition names every table
            // as `schema`.`table`, so the definition is read instead.
            let rows = self
                .require_rows(
                    "SELECT VIEW_DEFINITION FROM information_schema.VIEWS
                     WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                    (schema.to_string(), name.to_string()),
                )
                .await?;
            let definition = rows
                .first()
                .map(|row| cell_string(row, 0))
                .unwrap_or_default();
            for (table_schema, table_name) in qualified_names(&definition) {
                let exists = self
                    .require_rows(
                        "SELECT 1 FROM information_schema.TABLES
                         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                        (table_schema.clone(), table_name.clone()),
                    )
                    .await?;
                if !exists.is_empty() {
                    let id = self.relation_id(&table_schema, &table_name).await?;
                    push_unique(ids, id);
                }
            }
            return Ok(());
        }
        let rows = self
            .require_rows(
                "SELECT DISTINCT TABLE_SCHEMA, TABLE_NAME
                 FROM information_schema.VIEW_TABLE_USAGE
                 WHERE VIEW_SCHEMA = ? AND VIEW_NAME = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        for row in rows {
            let table_schema = cell_string(&row, 0);
            let table_name = cell_string(&row, 1);
            let id = self.relation_id(&table_schema, &table_name).await?;
            push_unique(ids, id);
        }
        Ok(())
    }

    async fn push_view_dependents(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let rows = if self.is_mariadb() {
            // No VIEW_TABLE_USAGE on MariaDB: a view depends on the table its stored
            // definition names as `schema`.`table`.
            let needle = format!("`{schema}`.`{name}`");
            let pattern = format!("%{}%", like_escape(&needle));
            self.require_rows(
                "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.VIEWS
                 WHERE VIEW_DEFINITION LIKE ? ESCAPE '!'",
                (pattern,),
            )
            .await?
        } else {
            self.require_rows(
                "SELECT DISTINCT VIEW_SCHEMA, VIEW_NAME
                 FROM information_schema.VIEW_TABLE_USAGE
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?
        };
        for row in rows {
            let view_schema = cell_string(&row, 0);
            let view_name = cell_string(&row, 1);
            push_unique(ids, my_id("view", format!("{view_schema}/{view_name}")));
        }
        Ok(())
    }

    async fn push_trigger_dependents(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let rows = self
            .require_rows(
                "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS
                 WHERE EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        for row in rows {
            let trigger = cell_string(&row, 0);
            push_unique(ids, my_id("trigger", format!("{schema}/{name}/{trigger}")));
        }
        Ok(())
    }

    async fn push_routine_view_dependents(
        &self,
        ids: &mut Vec<ObjectId>,
        schema: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let rows = self
            .require_rows(
                "SELECT DISTINCT VIEW_SCHEMA, VIEW_NAME
                 FROM information_schema.VIEW_ROUTINE_USAGE
                 WHERE SPECIFIC_SCHEMA = ? AND SPECIFIC_NAME = ?",
                (schema.to_string(), name.to_string()),
            )
            .await?;
        for row in rows {
            let view_schema = cell_string(&row, 0);
            let view_name = cell_string(&row, 1);
            push_unique(ids, my_id("view", format!("{view_schema}/{view_name}")));
        }
        Ok(())
    }

    async fn show_create(
        &self,
        kind: &str,
        schema: &str,
        name: &str,
    ) -> Result<String, DriverError> {
        let sql = format!("SHOW CREATE {kind} {}.{}", quote(schema), quote(name));
        self.show_create_sql(&sql).await
    }

    async fn show_create_ident(&self, kind: &str, name: &str) -> Result<String, DriverError> {
        self.show_create_sql(&format!("SHOW CREATE {kind} {}", quote(name)))
            .await
    }

    async fn show_create_sql(&self, sql: &str) -> Result<String, DriverError> {
        let mut conn = self.conn.lock().await;
        let row: Option<mysql_async::Row> = conn.query_first(sql).await.map_err(map_error)?;
        let row = row.ok_or_else(|| DriverError::unsupported("show create returned no row"))?;
        row.get_opt::<String, _>(1)
            .and_then(Result::ok)
            .or_else(|| row.get_opt::<String, _>(2).and_then(Result::ok))
            .ok_or_else(|| DriverError::unsupported("show create missing ddl column"))
    }
}

fn cell_string(row: &mysql_async::Row, idx: usize) -> String {
    match row.as_ref(idx) {
        None | Some(mysql_async::Value::NULL) => String::new(),
        Some(mysql_async::Value::Bytes(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
        Some(mysql_async::Value::Int(value)) => value.to_string(),
        Some(mysql_async::Value::UInt(value)) => value.to_string(),
        Some(_) => String::new(),
    }
}

fn cell_opt(row: &mysql_async::Row, idx: usize) -> Option<String> {
    let value = cell_string(row, idx);
    if value.is_empty() { None } else { Some(value) }
}

fn split2(key: &str) -> (&str, &str) {
    key.split_once('/').unwrap_or((key, key))
}

fn split3(key: &str) -> (&str, &str, &str) {
    let mut parts = key.split('/');
    (
        parts.next().unwrap_or(key),
        parts.next().unwrap_or(key),
        parts.next().unwrap_or(key),
    )
}

fn push_unique(ids: &mut Vec<ObjectId>, id: ObjectId) {
    if !ids.iter().any(|existing| existing == &id) {
        ids.push(id);
    }
}

/// The `schema`.`table` pairs a MariaDB view definition names, in order, once each. A
/// column reference `schema`.`table`.`column` names its table the same way.
fn qualified_names(definition: &str) -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> = Vec::new();
    let mut rest = definition;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else { break };
        let first = &after[..end];
        let tail = &after[end + 1..];
        if let Some(next) = tail.strip_prefix(".`")
            && let Some(close) = next.find('`')
        {
            let pair = (first.to_string(), next[..close].to_string());
            if !names.contains(&pair) {
                names.push(pair);
            }
        }
        rest = tail;
    }
    names
}

/// `text` for a LIKE pattern escaped with `!`.
fn like_escape(text: &str) -> String {
    text.replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}
