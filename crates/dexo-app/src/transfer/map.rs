/// A column of the file and the table's column it goes into; `skip` leaves it out.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMapping {
    pub source: String,
    pub target: String,
    pub skip: bool,
}

/// `source=target` pairs, as `dexo import --mapping` takes them; `source=` leaves the
/// file's column out.
pub fn parse_mapping(items: &[String]) -> Result<Vec<ColumnMapping>, String> {
    items
        .iter()
        .map(|item| {
            let (source, target) = item
                .split_once('=')
                .ok_or_else(|| format!("--mapping {item}: write it as source=target"))?;
            let (source, target) = (source.trim(), target.trim());
            if source.is_empty() {
                return Err(format!(
                    "--mapping {item}: name the file's column before the ="
                ));
            }
            Ok(ColumnMapping {
                source: source.to_string(),
                target: target.to_string(),
                skip: target.is_empty(),
            })
        })
        .collect()
}

/// The columns the file's rows go into, and the file's column that feeds each: its own
/// name unless a mapping gives another, a skipped one left out. A mapping names the
/// file's column as the file spells it, or in another case when that is the only match;
/// one the file does not have is an error naming the file's columns.
pub fn map_columns(
    columns: &[String],
    mapping: &[ColumnMapping],
) -> Result<(Vec<String>, Vec<usize>), String> {
    let mut by_column: Vec<Option<&ColumnMapping>> = vec![None; columns.len()];
    for item in mapping {
        let index = columns
            .iter()
            .position(|name| *name == item.source)
            .or_else(|| {
                columns
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(&item.source))
            })
            .ok_or_else(|| {
                format!(
                    "--mapping names {}, which the file does not have. The file's columns are: {}.",
                    item.source,
                    columns.join(", ")
                )
            })?;
        by_column[index] = Some(item);
    }
    let mut targets = Vec::new();
    let mut sources = Vec::new();
    for (index, (name, item)) in columns.iter().zip(by_column).enumerate() {
        match item {
            Some(item) if item.skip => continue,
            Some(item) => targets.push(item.target.clone()),
            None => targets.push(name.clone()),
        }
        sources.push(index);
    }
    Ok((targets, sources))
}

#[cfg(test)]
mod tests {
    use super::{map_columns, parse_mapping};

    fn columns() -> Vec<String> {
        ["id", "Email", "note"]
            .iter()
            .map(|name| name.to_string())
            .collect()
    }

    /// A mapping is by name, whatever order it is given in: `email=user_email` sends the
    /// file's Email to user_email, not the first column.
    #[test]
    fn a_mapping_renames_its_own_column_in_any_order() {
        let mapping = parse_mapping(&["email=user_email".into(), "id=user_id".into()]).unwrap();
        let (targets, sources) = map_columns(&columns(), &mapping).unwrap();
        assert_eq!(targets, ["user_id", "user_email", "note"]);
        assert_eq!(sources, [0, 1, 2]);
    }

    #[test]
    fn source_with_nothing_after_the_equals_is_left_out() {
        let mapping = parse_mapping(&["note=".into()]).unwrap();
        let (targets, sources) = map_columns(&columns(), &mapping).unwrap();
        assert_eq!(targets, ["id", "Email"]);
        assert_eq!(sources, [0, 1]);
    }

    #[test]
    fn a_column_the_file_does_not_have_is_named_with_the_files() {
        let mapping = parse_mapping(&["mail=user_email".into()]).unwrap();
        assert_eq!(
            map_columns(&columns(), &mapping).unwrap_err(),
            "--mapping names mail, which the file does not have. The file's columns are: id, Email, note."
        );
        assert!(parse_mapping(&["user_email".into()]).is_err());
        assert!(parse_mapping(&["=user_email".into()]).is_err());
    }
}
