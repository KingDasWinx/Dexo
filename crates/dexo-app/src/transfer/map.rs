#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMapping {
    pub source: String,
    pub target: String,
    pub skip: bool,
}

pub fn map_columns(source: &[String], target: &[String]) -> Vec<ColumnMapping> {
    source
        .iter()
        .map(|name| {
            let target_name = target
                .iter()
                .find(|item| item.eq_ignore_ascii_case(name))
                .cloned()
                .unwrap_or_else(|| name.clone());
            ColumnMapping {
                source: name.clone(),
                target: target_name,
                skip: false,
            }
        })
        .collect()
}

pub fn remap_row(
    source_columns: &[String],
    mapping: &[ColumnMapping],
    row: Vec<dexo_driver_api::DbValue>,
) -> Vec<(String, dexo_driver_api::DbValue)> {
    mapping
        .iter()
        .filter(|item| !item.skip)
        .filter_map(|item| {
            let index = source_columns
                .iter()
                .position(|name| name == &item.source)?;
            Some((
                item.target.clone(),
                row.get(index)
                    .cloned()
                    .unwrap_or(dexo_driver_api::DbValue::Null),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::map_columns;

    #[test]
    fn maps_by_name_case_insensitive() {
        let mapped = map_columns(&["Id".into()], &["id".into(), "name".into()]);
        assert_eq!(mapped[0].target, "id");
    }
}
