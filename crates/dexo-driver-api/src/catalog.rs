use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{DriverError, ObjectDdl, QualifiedName};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        assert!(!id.is_empty(), "object id must be non-empty");
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ObjectId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Catalog,
    Schema,
    Table,
    View,
    MaterializedView,
    Column,
    Index,
    Constraint,
    Sequence,
    Function,
    Procedure,
    Trigger,
    User,
    Role,
    DriverSpecific(String),
}

impl ObjectKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Catalog => "catalog",
            Self::Schema => "schema",
            Self::Table => "table",
            Self::View => "view",
            Self::MaterializedView => "materialized_view",
            Self::Column => "column",
            Self::Index => "index",
            Self::Constraint => "constraint",
            Self::Sequence => "sequence",
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Trigger => "trigger",
            Self::User => "user",
            Self::Role => "role",
            Self::DriverSpecific(kind) => kind.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogObject {
    pub id: ObjectId,
    pub kind: ObjectKind,
    pub qualified_name: QualifiedName,
    pub parent: Option<ObjectId>,
    pub attributes: BTreeMap<String, serde_json::Value>,
}

impl CatalogObject {
    pub fn new(
        id: ObjectId,
        kind: ObjectKind,
        qualified_name: QualifiedName,
        parent: Option<ObjectId>,
    ) -> Self {
        Self {
            id,
            kind,
            qualified_name,
            parent,
            attributes: BTreeMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogListOptions {
    pub include_system: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogRestriction {
    pub parent: Option<ObjectId>,
    pub capability: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogList {
    pub objects: Vec<CatalogObject>,
    pub restrictions: Vec<CatalogRestriction>,
}

#[async_trait::async_trait]
pub trait CatalogReader: Send + Sync {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError>;

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError>;

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError>;

    async fn dependencies(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError>;

    async fn dependents(&self, id: &ObjectId) -> Result<Vec<ObjectId>, DriverError>;
}

#[cfg(test)]
mod tests {
    use super::{CatalogObject, ObjectId, ObjectKind};
    use crate::QualifiedName;

    #[test]
    fn catalog_object_round_trip() {
        let object = CatalogObject::new(
            ObjectId::new("pg:table:16384"),
            ObjectKind::Table,
            QualifiedName::new(Some("dexo"), Some("public"), "orders"),
            Some(ObjectId::new("pg:schema:2200")),
        )
        .with_attribute("driver.postgres.partition_key", serde_json::json!("id"));
        let json = serde_json::to_string(&object).unwrap();
        let restored: CatalogObject = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, object);
        assert_eq!(
            restored.attributes.get("driver.postgres.partition_key"),
            Some(&serde_json::json!("id"))
        );
        assert_ne!(
            restored.id.as_str(),
            restored.qualified_name.display_unquoted()
        );
    }
}
