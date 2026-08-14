use dexo_driver_api::{ObjectKind, SchemaChange};

use crate::catalog_service::parse_qualified;

pub fn drop_table(qualified: &str) -> SchemaChange {
    SchemaChange::DropObject {
        target: parse_qualified(qualified),
        kind: ObjectKind::Table,
    }
}

#[cfg(test)]
mod tests {
    use super::drop_table;

    #[test]
    fn drop_table_keeps_qualified_target() {
        let change = drop_table("prod.public.orders");
        assert_eq!(change.target().display_unquoted(), "prod.public.orders");
    }
}
