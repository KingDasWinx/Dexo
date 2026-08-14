use dexo_driver_api::{
    CatalogList, CatalogListOptions, CatalogObject, CatalogReader, DdlOutcome, ObjectDdl, ObjectId,
    ObjectKind, QualifiedName,
};
use dexo_sql::Catalog;
use dexo_sql::completion::{FunctionInfo, TableInfo};

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
}

impl SnapshotCatalog {
    pub fn new(objects: Vec<CatalogObject>) -> Self {
        Self { objects }
    }

    pub fn objects(&self) -> &[CatalogObject] {
        &self.objects
    }
}

impl Catalog for SnapshotCatalog {
    fn tables(&self) -> Vec<TableInfo> {
        self.objects
            .iter()
            .filter(|object| {
                matches!(
                    object.kind,
                    ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
                )
            })
            .map(|object| {
                let columns = self
                    .objects
                    .iter()
                    .filter(|child| {
                        child.kind == ObjectKind::Column
                            && child.parent.as_ref() == Some(&object.id)
                    })
                    .map(|child| {
                        child
                            .qualified_name
                            .object()
                            .rsplit('.')
                            .next()
                            .unwrap_or(child.qualified_name.object())
                            .to_string()
                    })
                    .collect();
                TableInfo {
                    qualified: object.qualified_name.display_unquoted(),
                    schema: object.qualified_name.schema().unwrap_or("").to_string(),
                    name: object.qualified_name.object().to_string(),
                    favorite: false,
                    recency: 0,
                    columns,
                }
            })
            .collect()
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
