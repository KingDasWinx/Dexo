use dexo_driver_api::DbValue;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub primary_key: bool,
    pub unique: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableMeta {
    pub columns: Vec<ColumnDef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditMode {
    ReadOnly,
    Editable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowIdentity {
    pub columns: Vec<String>,
    pub values: Vec<DbValue>,
}

impl RowIdentity {
    pub fn from_table(table: &TableMeta) -> Option<Vec<String>> {
        let pk: Vec<String> = table
            .columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| column.name.clone())
            .collect();
        if !pk.is_empty() {
            return Some(pk);
        }
        let unique: Vec<String> = table
            .columns
            .iter()
            .filter(|column| column.unique && !column.nullable)
            .map(|column| column.name.clone())
            .collect();
        if unique.is_empty() {
            None
        } else {
            Some(unique)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PendingChange {
    Insert {
        values: Vec<(String, DbValue)>,
    },
    Update {
        identity: RowIdentity,
        original: Vec<(String, DbValue)>,
        values: Vec<(String, DbValue)>,
    },
    Delete {
        identity: RowIdentity,
        original: Vec<(String, DbValue)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangeSet {
    mode: EditMode,
    identity_columns: Vec<String>,
    pending: Vec<PendingChange>,
    errors: Vec<String>,
}

impl ChangeSet {
    pub fn for_table(table: &TableMeta) -> Self {
        match RowIdentity::from_table(table) {
            Some(columns) => Self {
                mode: EditMode::Editable,
                identity_columns: columns,
                pending: Vec::new(),
                errors: Vec::new(),
            },
            None => Self {
                mode: EditMode::ReadOnly,
                identity_columns: Vec::new(),
                pending: Vec::new(),
                errors: Vec::new(),
            },
        }
    }

    pub fn mode(&self) -> EditMode {
        self.mode
    }

    pub fn pending(&self) -> &[PendingChange] {
        &self.pending
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn insert(&mut self, values: Vec<(String, DbValue)>) {
        if self.mode == EditMode::ReadOnly {
            self.errors.push("table is read-only".into());
            return;
        }
        self.pending.push(PendingChange::Insert { values });
    }

    pub fn update(
        &mut self,
        identity: RowIdentity,
        original: Vec<(String, DbValue)>,
        values: Vec<(String, DbValue)>,
    ) {
        if self.mode == EditMode::ReadOnly {
            self.errors.push("table is read-only".into());
            return;
        }
        self.pending.push(PendingChange::Update {
            identity,
            original,
            values,
        });
    }

    pub fn delete(&mut self, identity: RowIdentity, original: Vec<(String, DbValue)>) {
        if self.mode == EditMode::ReadOnly {
            self.errors.push("table is read-only".into());
            return;
        }
        self.pending
            .push(PendingChange::Delete { identity, original });
    }

    pub fn revert(&mut self, index: usize) {
        if index < self.pending.len() {
            self.pending.remove(index);
        }
    }

    pub fn discard(&mut self) {
        self.pending.clear();
        self.errors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{ChangeSet, ColumnDef, EditMode, RowIdentity, TableMeta};
    use dexo_driver_api::DbValue;

    fn table_without_keys() -> TableMeta {
        TableMeta {
            columns: vec![ColumnDef {
                name: "note".into(),
                primary_key: false,
                unique: false,
                nullable: true,
            }],
        }
    }

    fn table_with_pk() -> TableMeta {
        TableMeta {
            columns: vec![ColumnDef {
                name: "id".into(),
                primary_key: true,
                unique: true,
                nullable: false,
            }],
        }
    }

    #[test]
    fn table_without_unique_identity_is_read_only() {
        assert_eq!(RowIdentity::from_table(&table_without_keys()), None);
        assert_eq!(
            ChangeSet::for_table(&table_without_keys()).mode(),
            EditMode::ReadOnly
        );
    }

    #[test]
    fn add_edit_delete_revert_does_not_mutate_loaded_rows() {
        let mut changes = ChangeSet::for_table(&table_with_pk());
        let loaded = vec![("id".into(), DbValue::I64(1))];
        changes.insert(vec![("id".into(), DbValue::I64(2))]);
        changes.update(
            RowIdentity {
                columns: vec!["id".into()],
                values: vec![DbValue::I64(1)],
            },
            loaded.clone(),
            vec![("id".into(), DbValue::I64(3))],
        );
        changes.delete(
            RowIdentity {
                columns: vec!["id".into()],
                values: vec![DbValue::I64(1)],
            },
            loaded.clone(),
        );
        assert_eq!(changes.pending().len(), 3);
        changes.revert(2);
        assert_eq!(changes.pending().len(), 2);
        changes.discard();
        assert!(changes.pending().is_empty());
        assert_eq!(loaded, vec![("id".into(), DbValue::I64(1))]);
    }
}

#[cfg(all(test, not(miri)))]
mod proptests {
    use super::{ChangeSet, ColumnDef, RowIdentity, TableMeta};
    use dexo_driver_api::DbValue;

    proptest::proptest! {
        #[test]
        fn add_edit_delete_revert_sequences(
            inserts in 0..8usize,
            edits in 0..8usize,
            deletes in 0..8usize,
            reverts in 0..8usize,
        ) {
            let table = TableMeta {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    primary_key: true,
                    unique: true,
                    nullable: false,
                }],
            };
            let mut changes = ChangeSet::for_table(&table);
            for index in 0..inserts {
                changes.insert(vec![("id".into(), DbValue::I64(index as i64))]);
            }
            for index in 0..edits {
                changes.update(
                    RowIdentity {
                        columns: vec!["id".into()],
                        values: vec![DbValue::I64(index as i64)],
                    },
                    vec![("id".into(), DbValue::I64(index as i64))],
                    vec![("id".into(), DbValue::I64(index as i64 + 1))],
                );
            }
            for index in 0..deletes {
                changes.delete(
                    RowIdentity {
                        columns: vec!["id".into()],
                        values: vec![DbValue::I64(index as i64)],
                    },
                    vec![("id".into(), DbValue::I64(index as i64))],
                );
            }
            let before = changes.pending().len();
            for _ in 0..reverts {
                if !changes.pending().is_empty() {
                    changes.revert(0);
                }
            }
            assert_eq!(changes.pending().len(), before.saturating_sub(reverts.min(before)));
            let loaded: Vec<(String, DbValue)> = vec![("id".into(), DbValue::I64(0))];
            changes.discard();
            assert!(changes.pending().is_empty());
            assert_eq!(loaded, vec![("id".into(), DbValue::I64(0))]);
        }
    }
}
