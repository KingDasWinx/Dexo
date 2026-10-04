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
    /// SQL text typed into the workbench's WHERE and ORDER BY bars. Only the TUI sets
    /// it, after checking it reads; MCP builds requests from typed filters alone.
    pub clauses: RawClauses,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawClauses {
    pub where_sql: Option<String>,
    pub order_by: Option<String>,
}

impl RawClauses {
    /// The WHERE condition: the raw text, the typed filter, or both together.
    pub fn condition(&self, typed: Option<String>) -> Option<String> {
        let raw = self
            .where_sql
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|raw| {
                // A line comment at its end (`--`, or MySQL's `#`) ran on over the `)`
                // and everything after it on the line.
                if raw.contains("--") || raw.contains('#') {
                    format!("({raw}\n)")
                } else {
                    format!("({raw})")
                }
            });
        match (raw, typed) {
            (Some(raw), Some(typed)) => Some(format!("{raw} AND ({typed})")),
            (raw, None) => raw,
            (None, typed) => typed,
        }
    }

    /// The ORDER BY text, which takes the place of a typed sort.
    pub fn order(&self) -> Option<&str> {
        self.order_by
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
    }
}

impl DataRequest {
    pub fn validate(&self) -> Result<(), DriverError> {
        for column in self
            .columns
            .iter()
            .chain(self.sort.iter().map(|sort| &sort.column))
        {
            if column.0.is_empty() || column.0.contains('\0') {
                return Err(DriverError::new(
                    DriverErrorCategory::Configuration,
                    "invalid column",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataPage {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<DbValue>>,
    pub offset: u64,
    pub has_more: bool,
    pub estimated_total: Option<u64>,
}

impl DataPage {
    pub fn from_fetched(
        columns: Vec<ColumnMeta>,
        mut rows: Vec<Vec<DbValue>>,
        offset: u64,
        limit: u32,
    ) -> Self {
        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.truncate(limit as usize);
        }
        Self {
            columns,
            rows,
            offset,
            has_more,
            estimated_total: None,
        }
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteValueRef {
    pub object: QualifiedName,
    pub identity: Vec<(ColumnId, DbValue)>,
    pub column: ColumnId,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnKeyInfo {
    pub name: String,
    pub primary_key: bool,
    pub unique: bool,
}

#[async_trait::async_trait]
pub trait DataMutator: Send + Sync {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError>;
    async fn fetch_value(
        &self,
        value: &RemoteValueRef,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, DriverError>;
    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError>;
    async fn table_columns(
        &self,
        target: &QualifiedName,
    ) -> Result<Vec<ColumnKeyInfo>, DriverError>;

    /// The table's row count as the server's statistics have it, without counting:
    /// `None` when it keeps none.
    async fn estimate_rows(&self, _target: &QualifiedName) -> Result<Option<u64>, DriverError> {
        Ok(None)
    }
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

    /// A comment ending the WHERE text ends on its own line, before the `)` and the
    /// typed filter that follow it.
    #[test]
    fn a_trailing_comment_cannot_take_the_typed_filter() {
        let clauses = |text: &str| super::RawClauses {
            where_sql: Some(text.into()),
            order_by: None,
        };
        assert_eq!(
            clauses("total > 1").condition(Some("id = ?".into())),
            Some("(total > 1) AND (id = ?)".into())
        );
        assert_eq!(
            clauses("total > 1 -- big").condition(Some("id = ?".into())),
            Some("(total > 1 -- big\n) AND (id = ?)".into())
        );
        assert_eq!(
            clauses("total > 1 # big").condition(None),
            Some("(total > 1 # big\n)".into())
        );
        assert_eq!(clauses("  ").condition(None), None);
    }
}
