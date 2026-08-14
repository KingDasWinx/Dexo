use dexo_driver_api::CatalogObject;

const NOISE_KEYS: &[&str] = &[
    "driver.postgres.oid",
    "driver.postgres.attnum",
    "driver.mysql.oid",
];

pub fn normalize(mut objects: Vec<CatalogObject>, _driver: &str) -> Vec<CatalogObject> {
    for object in &mut objects {
        for key in NOISE_KEYS {
            object.attributes.remove(*key);
        }
    }
    objects.sort_by(|a, b| {
        a.kind.as_str().cmp(b.kind.as_str()).then_with(|| {
            a.qualified_name
                .display_unquoted()
                .cmp(&b.qualified_name.display_unquoted())
        })
    });
    objects
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn table(name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn ordering_noise_removed_native_differences_kept() {
        let mut a = table("orders").with_attribute("driver.postgres.oid", serde_json::json!(1));
        a = a.with_attribute("driver.postgres.policy", serde_json::json!("orders_all"));
        let b = table("users").with_attribute("driver.postgres.oid", serde_json::json!(2));
        let first = normalize(vec![b.clone(), a.clone()], "postgres");
        let second = normalize(first.clone(), "postgres");
        assert_eq!(first, second);
        assert_eq!(first[0].qualified_name.object(), "orders");
        assert!(!first[0].attributes.contains_key("driver.postgres.oid"));
        assert_eq!(
            first[0].attributes.get("driver.postgres.policy"),
            Some(&serde_json::json!("orders_all"))
        );
        let mysql = table("orders")
            .with_attribute("driver.mysql.collation", serde_json::json!("utf8mb4_bin"));
        let kept = normalize(vec![mysql], "mysql");
        assert_eq!(
            kept[0].attributes.get("driver.mysql.collation"),
            Some(&serde_json::json!("utf8mb4_bin"))
        );
    }
}
