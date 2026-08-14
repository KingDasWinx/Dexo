use dexo_driver_api::{ColumnId, DbValue, Filter};

pub fn assert_typed_filter() -> Filter {
    Filter::Eq(ColumnId("age".into()), DbValue::I64(18))
}

#[cfg(test)]
mod tests {
    use super::assert_typed_filter;
    use dexo_driver_api::{ColumnId, DbValue, Filter};

    #[test]
    fn filters_are_typed_ast_nodes_not_raw_sql() {
        assert!(matches!(
            assert_typed_filter(),
            Filter::Eq(ColumnId(name), DbValue::I64(18)) if name == "age"
        ));
    }
}
