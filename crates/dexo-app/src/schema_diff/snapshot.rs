use dexo_driver_api::CatalogObject;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub format_version: u32,
    pub driver: String,
    pub server_version: String,
    pub captured_at: String,
    pub scope: String,
    pub objects: Vec<CatalogObject>,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot digest mismatch")]
    Tampered,
    #[error("unsupported snapshot format {0}")]
    Unsupported(u32),
}

impl SchemaSnapshot {
    pub fn capture(
        driver: impl Into<String>,
        server_version: impl Into<String>,
        captured_at: impl Into<String>,
        scope: impl Into<String>,
        mut objects: Vec<CatalogObject>,
    ) -> Self {
        canonicalize(&mut objects);
        let mut snapshot = Self {
            format_version: FORMAT_VERSION,
            driver: driver.into(),
            server_version: server_version.into(),
            captured_at: captured_at.into(),
            scope: scope.into(),
            objects,
            digest: String::new(),
        };
        snapshot.digest = digest_of(&snapshot);
        snapshot
    }

    pub fn verify(&self) -> Result<(), SnapshotError> {
        if self.format_version != FORMAT_VERSION {
            return Err(SnapshotError::Unsupported(self.format_version));
        }
        if self.digest != digest_of(self) {
            return Err(SnapshotError::Tampered);
        }
        Ok(())
    }
}

fn canonicalize(objects: &mut [CatalogObject]) {
    objects.sort_by(|a, b| {
        a.kind
            .as_str()
            .cmp(b.kind.as_str())
            .then_with(|| {
                a.qualified_name
                    .display_unquoted()
                    .cmp(&b.qualified_name.display_unquoted())
            })
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
}

fn digest_of(snapshot: &SchemaSnapshot) -> String {
    let mut body = snapshot.clone();
    body.digest.clear();
    let json = serde_json::to_vec(&body).expect("snapshot json");
    let hash = Sha256::digest(json);
    hash.iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::SchemaSnapshot;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn obj(kind: ObjectKind, name: &str, attr: Option<(&str, serde_json::Value)>) -> CatalogObject {
        let mut object = CatalogObject::new(
            ObjectId::new(name),
            kind,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        );
        if let Some((key, value)) = attr {
            object = object.with_attribute(key, value);
        }
        object
    }

    #[test]
    fn round_trip_includes_driver_attributes_and_stable_digest() {
        let snapshot = SchemaSnapshot::capture(
            "postgres",
            "16.9",
            "2026-08-14T00:00:00Z",
            "db.public",
            vec![
                obj(
                    ObjectKind::Table,
                    "orders",
                    Some(("driver.postgres.partition_key", serde_json::json!("id"))),
                ),
                obj(ObjectKind::View, "v_orders", None),
            ],
        );
        assert_eq!(snapshot.format_version, 1);
        assert_eq!(snapshot.digest.len(), 64);
        snapshot.verify().unwrap();
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: SchemaSnapshot = serde_json::from_str(&json).unwrap();
        restored.verify().unwrap();
        assert_eq!(restored, snapshot);
        let again = SchemaSnapshot::capture(
            "postgres",
            "16.9",
            "2026-08-14T00:00:00Z",
            "db.public",
            vec![
                obj(ObjectKind::View, "v_orders", None),
                obj(
                    ObjectKind::Table,
                    "orders",
                    Some(("driver.postgres.partition_key", serde_json::json!("id"))),
                ),
            ],
        );
        assert_eq!(again.digest, snapshot.digest);
    }

    #[test]
    fn tampered_digest_is_rejected() {
        let mut snapshot = SchemaSnapshot::capture(
            "mysql",
            "8.4",
            "2026-08-14T00:00:00Z",
            "dexo",
            vec![obj(
                ObjectKind::Table,
                "t",
                Some(("driver.mysql.collation", serde_json::json!("utf8mb4_bin"))),
            )],
        );
        snapshot.objects[0].attributes.insert(
            "driver.mysql.collation".into(),
            serde_json::json!("utf8mb4_general_ci"),
        );
        assert_eq!(snapshot.verify(), Err(super::SnapshotError::Tampered));
    }
}
