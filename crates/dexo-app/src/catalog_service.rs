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

    /// The object `qualified` names: in full, `shop.public.orders`, or by its last parts,
    /// `public.orders` or `orders`, in any database the connection lists (an attached
    /// one too) and any schema. A name of the right case wins over one that differs only
    /// in case.
    pub async fn find_by_qualified_name(
        reader: &dyn CatalogReader,
        qualified: &str,
        options: &CatalogListOptions,
    ) -> Result<Option<CatalogObject>, AppError> {
        let parsed = parse_qualified(qualified);
        let roots = Self::list_children(reader, None, options).await?;
        let mut seen: Vec<CatalogObject> = Vec::new();
        for (index, catalog) in roots.objects.into_iter().enumerate() {
            let children = Self::list_children(reader, Some(&catalog.id), options).await?;
            if index == 0
                && !children.restrictions.is_empty()
                && children.objects.is_empty()
                && matches_restricted(qualified, &children.restrictions)
            {
                return Err(AppError::new(
                    crate::error::ErrorCategory::Permission,
                    "object is restricted",
                ));
            }
            seen.push(catalog);
            for child in children.objects {
                let descend = child.kind == ObjectKind::Schema
                    && parsed.schema().is_none_or(|schema| {
                        child.qualified_name.object().eq_ignore_ascii_case(schema)
                    });
                if descend {
                    let objects = Self::list_children(reader, Some(&child.id), options).await?;
                    seen.extend(objects.objects);
                }
                seen.push(child);
            }
        }
        let exact = seen
            .iter()
            .position(|object| matches_name(object, qualified, true));
        let any_case = || {
            seen.iter()
                .position(|object| matches_name(object, qualified, false))
        };
        Ok(exact.or_else(any_case).map(|index| seen.swap_remove(index)))
    }

    pub fn refresh_required_after_ddl(outcome: DdlOutcome, target: &QualifiedName) -> bool {
        invalidate_after_ddl(outcome, target) != CacheAction::Keep
    }
}

/// The name in full, `shop.public.orders`, or its last parts, `public.orders` or
/// `orders`, compared part by part: `orders` is not `a.orders`, a table whose name has a
/// dot in it, which its whole name still finds.
fn matches_name(object: &CatalogObject, qualified: &str, exact_case: bool) -> bool {
    let same = |a: &str, b: &str| {
        if exact_case {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    };
    let name = &object.qualified_name;
    let have: Vec<&str> = name
        .catalog()
        .into_iter()
        .chain(name.schema())
        .chain([name.object()])
        .collect();
    let wanted: Vec<&str> = qualified.split('.').collect();
    same(name.object(), qualified)
        || same(&name.display_unquoted(), qualified)
        || wanted.len() <= have.len()
            && have[have.len() - wanted.len()..]
                .iter()
                .zip(&wanted)
                .all(|(have, wanted)| same(have, wanted))
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

/// What the diagnostics know of a catalog: whatever a FROM can name, and the columns,
/// by the schema they are in -- MySQL names its databases as catalogs, the others as
/// schemas.
pub fn known_objects(objects: &[CatalogObject]) -> dexo_sql::KnownObjects {
    let mut known = dexo_sql::KnownObjects::default();
    for object in objects {
        let name = &object.qualified_name;
        let schema = name.schema().or(name.catalog()).unwrap_or("");
        match &object.kind {
            // Sequences and partitions read like tables.
            ObjectKind::Table
            | ObjectKind::View
            | ObjectKind::MaterializedView
            | ObjectKind::Sequence => known.add_table(schema, name.object()),
            ObjectKind::DriverSpecific(kind) if kind == "partition" => {
                known.add_table(schema, name.object())
            }
            ObjectKind::Column => {
                if let Some((table, column)) = name.object().rsplit_once('.') {
                    known.add_column(schema, table, column);
                }
            }
            _ => {}
        }
    }
    known
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

/// What `object` is, where the catalog lists it as what it reads like: a Postgres
/// foreign table is listed as a table, but `DROP TABLE` refuses it and `\dt` should
/// say what it is.
pub(crate) fn exact_kind(object: &CatalogObject) -> dexo_driver_api::ObjectKind {
    match object
        .attributes
        .get("driver.postgres.relkind")
        .and_then(|relkind| relkind.as_str())
    {
        Some("f") => dexo_driver_api::ObjectKind::DriverSpecific("foreign_table".into()),
        _ => object.kind.clone(),
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

    /// `inspect --object t2` found nothing: only the first database was looked in, and
    /// a schema's tables only when the schema was named.
    #[tokio::test]
    async fn an_object_is_found_in_any_database_and_schema() {
        use dexo_driver_api::CatalogListOptions;
        let object = |id: &str, kind, name: QualifiedName, parent: Option<&str>| {
            CatalogObject::new(ObjectId::new(id), kind, name, parent.map(ObjectId::new))
        };
        let session =
            dexo_test_support::FakeSession::with_rows(&[], Vec::new()).with_catalog(vec![
                object(
                    "db1",
                    ObjectKind::Catalog,
                    QualifiedName::new(None::<String>, None::<String>, "shop"),
                    None,
                ),
                object(
                    "db2",
                    ObjectKind::Catalog,
                    QualifiedName::new(None::<String>, None::<String>, "other"),
                    None,
                ),
                object(
                    "s2",
                    ObjectKind::Schema,
                    QualifiedName::new(Some("other"), None::<String>, "main"),
                    Some("db2"),
                ),
                object(
                    "t2",
                    ObjectKind::Table,
                    QualifiedName::new(Some("other"), Some("main"), "T2"),
                    Some("s2"),
                ),
                object(
                    "t3",
                    ObjectKind::Table,
                    QualifiedName::new(Some("other"), Some("main"), "t2"),
                    Some("s2"),
                ),
            ]);
        let find = |name: &'static str| {
            let session = &session;
            async move {
                super::CatalogService::find_by_qualified_name(
                    session,
                    name,
                    &CatalogListOptions::default(),
                )
                .await
                .unwrap()
                .map(|found| found.id.as_str().to_string())
            }
        };
        assert_eq!(find("t2").await.as_deref(), Some("t3"));
        assert_eq!(find("T2").await.as_deref(), Some("t2"));
        assert!(find("MAIN.t3").await.is_none());
        assert!(find("MAIN.t2").await.is_some());
        assert_eq!(find("other.main.T2").await.as_deref(), Some("t2"));
        assert_eq!(find("nope").await, None);
    }

    #[test]
    fn an_object_is_found_by_its_last_name_parts() {
        let table = CatalogObject::new(
            ObjectId::new("t1"),
            ObjectKind::Table,
            QualifiedName::new(Some("shop"), Some("public"), "orders"),
            None,
        );
        for name in ["shop.public.orders", "public.orders", "orders"] {
            assert!(super::matches_name(&table, name, true), "{name}");
        }
        assert!(super::matches_name(&table, "Public.ORDERS", false));
        assert!(!super::matches_name(&table, "Public.ORDERS", true));
        for name in ["lic.orders", "hop.public.orders", "public"] {
            assert!(!super::matches_name(&table, name, false), "{name}");
        }
        let dotted = CatalogObject::new(
            ObjectId::new("t2"),
            ObjectKind::Table,
            QualifiedName::new(Some("shop"), Some("main"), "a.orders"),
            None,
        );
        assert!(!super::matches_name(&dotted, "orders", false));
        assert!(super::matches_name(&dotted, "a.orders", true));
    }

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
