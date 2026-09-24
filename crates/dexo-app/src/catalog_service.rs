use std::collections::HashMap;

use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, DdlOutcome, ObjectDdl, ObjectId,
    ObjectKind, QualifiedName,
};
use dexo_sql::Catalog;
use dexo_sql::completion::{ForeignKey, FunctionInfo, TableInfo};

use crate::error::AppError;
use crate::query_service::map_driver_error;
use crate::schema::{CacheAction, invalidate_after_ddl};

pub struct CatalogService;

impl CatalogService {
    pub async fn list_children(
        reader: &dyn CatalogReader,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, AppError> {
        reader
            .list_children(parent, options)
            .await
            .map_err(map_driver_error)
    }

    pub async fn object(
        reader: &dyn CatalogReader,
        id: &ObjectId,
    ) -> Result<Option<CatalogObject>, AppError> {
        reader.object(id).await.map_err(map_driver_error)
    }

    pub async fn ddl(reader: &dyn CatalogReader, id: &ObjectId) -> Result<ObjectDdl, AppError> {
        reader.ddl(id).await.map_err(map_driver_error)
    }

    pub async fn find_by_qualified_name(
        reader: &dyn CatalogReader,
        qualified: &str,
        options: &CatalogListOptions,
    ) -> Result<Option<CatalogObject>, AppError> {
        let parsed = parse_qualified(qualified);
        let roots = Self::list_children(reader, None, options).await?;
        let Some(catalog) = roots.objects.into_iter().next() else {
            return Ok(None);
        };
        if matches_name(&catalog, qualified) {
            return Ok(Some(catalog));
        }
        let children = Self::list_children(reader, Some(&catalog.id), options).await?;
        if !children.restrictions.is_empty()
            && children.objects.is_empty()
            && matches_restricted(qualified, &children.restrictions)
        {
            return Err(AppError::new(
                crate::error::ErrorCategory::Permission,
                "object is restricted",
            ));
        }
        if let Some(object) = children
            .objects
            .iter()
            .find(|object| matches_name(object, qualified))
        {
            return Ok(Some(object.clone()));
        }
        let schema = children.objects.iter().find(|object| {
            object.kind == ObjectKind::Schema
                && parsed
                    .schema()
                    .is_some_and(|schema| object.qualified_name.object() == schema)
        });
        if let Some(schema) = schema {
            let schema_children = Self::list_children(reader, Some(&schema.id), options).await?;
            if let Some(object) = schema_children
                .objects
                .into_iter()
                .find(|object| matches_name(object, qualified))
            {
                return Ok(Some(object));
            }
        }
        Ok(None)
    }

    pub fn refresh_required_after_ddl(outcome: DdlOutcome, target: &QualifiedName) -> bool {
        invalidate_after_ddl(outcome, target) != CacheAction::Keep
    }
}

fn matches_name(object: &CatalogObject, qualified: &str) -> bool {
    object.qualified_name.display_unquoted() == qualified
        || object.qualified_name.object() == qualified
}

fn matches_restricted(
    qualified: &str,
    restrictions: &[dexo_driver_api::CatalogRestriction],
) -> bool {
    let hay = qualified.to_ascii_lowercase();
    restrictions.iter().any(|restriction| {
        hay.contains("mysql.user")
            || hay.contains(".user")
            || restriction.capability.contains("user")
            || restriction.capability.contains("role")
    })
}

pub struct SnapshotCatalog {
    objects: Vec<CatalogObject>,
    /// Columns grouped by the table that owns them, built once. Matching them with a
    /// nested scan per table made this O(tables x objects), on a path that runs for
    /// every character typed in the editor.
    columns: HashMap<ObjectId, Vec<String>>,
    /// Foreign keys by the qualified name of the table that declares them. Both drivers
    /// already attach them to `ObjectKind::Constraint`; nothing read them until now.
    foreign_keys: HashMap<String, Vec<ForeignKey>>,
    /// Tables, views and materialized views by lowercased name, as positions in
    /// `objects`: a statement's tables are looked up on every character typed.
    tables_by_name: HashMap<String, Vec<usize>>,
}

impl SnapshotCatalog {
    pub fn new(objects: Vec<CatalogObject>) -> Self {
        let mut columns: HashMap<ObjectId, Vec<String>> = HashMap::new();
        for object in &objects {
            if object.kind != ObjectKind::Column {
                continue;
            }
            let Some(parent) = object.parent.clone() else {
                continue;
            };
            let name = object.qualified_name.object();
            columns
                .entry(parent)
                .or_default()
                .push(name.rsplit('.').next().unwrap_or(name).to_string());
        }
        let by_id: HashMap<&ObjectId, &CatalogObject> =
            objects.iter().map(|object| (&object.id, object)).collect();
        let mut foreign_keys: HashMap<String, Vec<ForeignKey>> = HashMap::new();
        for object in &objects {
            if object.kind != ObjectKind::Constraint {
                continue;
            }
            let Some(key) = foreign_key(object) else {
                continue;
            };
            let Some(table) = object.parent.as_ref().and_then(|id| by_id.get(id)) else {
                continue;
            };
            foreign_keys
                .entry(table.qualified_name.display_unquoted())
                .or_default()
                .push(key);
        }
        drop(by_id);
        let mut tables_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, object) in objects.iter().enumerate() {
            if is_table(object) {
                tables_by_name
                    .entry(object.qualified_name.object().to_ascii_lowercase())
                    .or_default()
                    .push(index);
            }
        }
        Self {
            objects,
            columns,
            foreign_keys,
            tables_by_name,
        }
    }

    fn table_info(&self, object: &CatalogObject) -> TableInfo {
        TableInfo {
            qualified: object.qualified_name.display_unquoted(),
            schema: object.qualified_name.schema().unwrap_or("").to_string(),
            name: object.qualified_name.object().to_string(),
            favorite: object
                .attributes
                .get("favorite")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            recency: object
                .attributes
                .get("recency")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            columns: self.columns.get(&object.id).cloned().unwrap_or_default(),
        }
    }

    pub fn objects(&self) -> &[CatalogObject] {
        &self.objects
    }
}

impl Catalog for SnapshotCatalog {
    fn tables(&self) -> Vec<TableInfo> {
        self.objects
            .iter()
            .filter(|object| is_table(object))
            .map(|object| self.table_info(object))
            .collect()
    }

    fn table(&self, schema: Option<&str>, name: &str) -> Option<TableInfo> {
        let candidates = self.tables_by_name.get(&name.to_ascii_lowercase())?;
        let found = candidates
            .iter()
            .map(|&index| &self.objects[index])
            .find(|object| {
                schema.is_none_or(|schema| {
                    object
                        .qualified_name
                        .schema()
                        .is_some_and(|own| own.eq_ignore_ascii_case(schema))
                })
            })?;
        Some(self.table_info(found))
    }

    fn foreign_keys(&self, qualified: &str) -> Vec<ForeignKey> {
        self.foreign_keys
            .get(qualified)
            .cloned()
            .unwrap_or_default()
    }

    fn functions(&self) -> Vec<FunctionInfo> {
        self.objects
            .iter()
            .filter(|object| object.kind == ObjectKind::Function)
            .map(|object| FunctionInfo {
                name: object.qualified_name.object().to_string(),
                signature: format!("{}()", object.qualified_name.object()),
            })
            .collect()
    }
}

fn is_table(object: &CatalogObject) -> bool {
    matches!(
        object.kind,
        ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
    )
}

/// The foreign key a constraint object carries, if it is one. Both drivers write the
/// same five attributes; the referenced table arrives as its parts.
fn foreign_key(object: &CatalogObject) -> Option<ForeignKey> {
    let strings = |name: &str| -> Vec<String> {
        object
            .attributes
            .get(name)
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let text = |name: &str| -> Option<String> {
        object
            .attributes
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    let table = text("fk_table")?;
    let local_columns = strings("fk_local");
    let referenced_columns = strings("fk_referenced");
    if local_columns.is_empty() || local_columns.len() != referenced_columns.len() {
        return None;
    }
    let referenced = match text("fk_schema") {
        Some(schema) => format!("{schema}.{table}"),
        None => table,
    };
    Some(ForeignKey {
        local_columns,
        referenced,
        referenced_columns,
    })
}

pub fn parse_qualified(input: &str) -> QualifiedName {
    let parts: Vec<&str> = input.split('.').collect();
    match parts.as_slice() {
        [catalog, schema, object] => QualifiedName::new(Some(*catalog), Some(*schema), *object),
        [schema, object] => QualifiedName::new(None::<String>, Some(*schema), *object),
        [object] => QualifiedName::new(None::<String>, None::<String>, *object),
        _ => QualifiedName::new(None::<String>, None::<String>, input),
    }
}

#[cfg(test)]
mod tests {
    use super::SnapshotCatalog;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use dexo_sql::{Catalog, Dialect, complete, labels};

    #[test]
    fn offline_snapshot_powers_autocomplete() {
        let table = CatalogObject::new(
            ObjectId::new("t1"),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), "users"),
            None,
        );
        let column = CatalogObject::new(
            ObjectId::new("c1"),
            ObjectKind::Column,
            QualifiedName::new(Some("db"), Some("public"), "users.id"),
            Some(ObjectId::new("t1")),
        );
        let catalog = SnapshotCatalog::new(vec![table, column]);
        assert_eq!(catalog.tables()[0].columns, vec!["id"]);
        let items = complete(
            "select u. from public.users u",
            9,
            &catalog,
            Dialect::Postgres,
        );
        assert_eq!(labels(items), ["id"]);
    }

    #[test]
    fn first_statement_failure_does_not_require_refresh() {
        let target = QualifiedName::new(Some("db"), Some("public"), "orders");
        assert!(!super::CatalogService::refresh_required_after_ddl(
            dexo_driver_api::DdlOutcome::RolledBack,
            &target
        ));
        assert!(super::CatalogService::refresh_required_after_ddl(
            dexo_driver_api::DdlOutcome::Unknown,
            &target
        ));
    }
}
