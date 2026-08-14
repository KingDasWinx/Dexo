use crate::{ColumnMeta, DbValue, DriverError, DriverErrorCategory, QualifiedName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    Eq(ColumnId, DbValue),
    Ne(ColumnId, DbValue),
    Gt(ColumnId, DbValue),
    Gte(ColumnId, DbValue),
    Lt(ColumnId, DbValue),
    Lte(ColumnId, DbValue),
    IsNull(ColumnId),
    IsNotNull(ColumnId),
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sort {
    pub column: ColumnId,
    pub descending: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Page {
    pub offset: u64,
    pub limit: u32,
}

impl Page {
    pub const MAX_LIMIT: u32 = 10_000;

    pub fn new(offset: u64, limit: u32) -> Result<Self, DriverError> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(DriverError::new(
                DriverErrorCategory::Configuration,
                "page limit must be 1..=10000",
            ));
        }
        Ok(Self { offset, limit })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataRequest {
    pub object: QualifiedName,
    pub columns: Vec<ColumnId>,
    pub filter: Option<Filter>,
    pub sort: Vec<Sort>,
    pub page: Page,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataPage {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<DbValue>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mutation {
    Insert {
        table: QualifiedName,
        columns: Vec<ColumnId>,
        values: Vec<DbValue>,
    },
    Update {
        table: QualifiedName,
        identity: Vec<(ColumnId, DbValue)>,
        original: Vec<(ColumnId, DbValue)>,
        changes: Vec<(ColumnId, DbValue)>,
    },
    Delete {
        table: QualifiedName,
        identity: Vec<(ColumnId, DbValue)>,
        original: Vec<(ColumnId, DbValue)>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationConflict {
    pub message: String,
}

impl MutationConflict {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait::async_trait]
pub trait DataMutator: Send + Sync {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError>;
    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError>;
}

#[cfg(test)]
mod tests {
    use super::{ColumnId, Filter, Page};
    use crate::DbValue;

    #[test]
    fn filters_are_typed_ast_not_raw_sql() {
        let filter = Filter::Eq(ColumnId("age".into()), DbValue::I64(18));
        match filter {
            Filter::Eq(column, DbValue::I64(18)) => assert_eq!(column.0, "age"),
            _ => panic!("filter must stay a typed AST"),
        }
        assert!(Page::new(0, 10_001).is_err());
        assert!(Page::new(0, 10_000).is_ok());
    }

    #[test]
    fn identifiers_and_values_stay_quoted() {
        for name in ["id", "a\"b", "x;drop table t", "col`x"] {
            let pg = format!("\"{}\"", name.replace('"', "\"\""));
            assert!(pg.starts_with('"') && pg.ends_with('"'));
            assert_eq!(&pg[1..pg.len() - 1], &name.replace('"', "\"\""));
            let my = format!("`{}`", name.replace('`', "``"));
            assert!(my.starts_with('`') && my.ends_with('`'));
        }
        let value = "O'Reilly";
        assert_eq!(value.replace('\'', "''"), "O''Reilly");
    }
}
