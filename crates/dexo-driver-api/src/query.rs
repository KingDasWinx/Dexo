use std::{pin::Pin, time::Duration};

use futures_core::Stream;
use uuid::Uuid;

use crate::{DbValue, DriverError};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QueryId(pub Uuid);

#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub id: QueryId,
    pub sql: String,
    pub parameters: Vec<DbValue>,
    pub row_limit: u64,
    pub timeout: Duration,
    pub mutating: bool,
}

impl QueryRequest {
    pub fn read(sql: impl Into<String>, row_limit: u64) -> Self {
        Self {
            id: QueryId(Uuid::new_v4()),
            sql: sql.into(),
            parameters: vec![],
            row_limit,
            timeout: Duration::from_secs(30),
            mutating: false,
        }
    }

    pub fn write(sql: impl Into<String>) -> Self {
        Self {
            mutating: true,
            ..Self::read(sql, 0)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowBatch {
    pub rows: Vec<Vec<DbValue>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueryEvent {
    ResultSetStarted { index: usize },
    Columns(Vec<ColumnMeta>),
    Rows(RowBatch),
    Notice { message: String },
    ResultSetFinished { index: usize, rows_affected: Option<u64> },
    Finished { rows_affected: Option<u64> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    Notice {
        severity: Option<String>,
        message: String,
    },
}

pub type SessionEventStream = std::pin::Pin<Box<dyn Stream<Item = SessionEvent> + Send>>;

pub type QueryStream = Pin<Box<dyn Stream<Item = Result<QueryEvent, DriverError>> + Send>>;
