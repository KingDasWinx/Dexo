use serde::{Deserialize, Serialize};

use crate::DriverError;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanMetrics {
    pub cost: Option<f64>,
    pub rows: Option<f64>,
    pub width: Option<f64>,
    pub time_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanNode {
    pub kind: String,
    pub relation: Option<String>,
    /// What the node does its work by -- its condition, sort or group key, the index it
    /// uses -- in the server's own words. Plans saved before it existed have none.
    #[serde(default)]
    pub detail: Option<String>,
    pub estimates: PlanMetrics,
    pub actual: PlanMetrics,
    pub loops: Option<u64>,
    pub children: Vec<PlanNode>,
    pub native: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplainPlan {
    pub planning_ms: Option<f64>,
    pub execution_ms: Option<f64>,
    pub root: PlanNode,
    pub raw: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainRequest {
    pub sql: String,
    pub analyze: bool,
    /// Indexes to plan with as if they were built -- `CREATE INDEX ON orders
    /// (customer_id)` -- for an estimated plan only. Postgres tries them with hypopg on
    /// this session alone and drops them after; the other drivers refuse a list.
    pub hypothetical_indexes: Vec<String>,
}

impl ExplainRequest {
    pub fn estimated(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            analyze: false,
            hypothetical_indexes: Vec::new(),
        }
    }

    pub fn analyzed(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            analyze: true,
            hypothetical_indexes: Vec::new(),
        }
    }

    /// The estimated plan of `sql` with `indexes` as if they were built.
    pub fn with_indexes(sql: impl Into<String>, indexes: Vec<String>) -> Self {
        Self {
            hypothetical_indexes: indexes,
            ..Self::estimated(sql)
        }
    }
}

/// What a driver without hypothetical indexes says to a request that has some.
pub fn hypothetical_unsupported() -> DriverError {
    DriverError::unsupported(
        "trying an index before building it needs Postgres with the hypopg extension",
    )
}

#[async_trait::async_trait]
pub trait ExplainProvider: Send + Sync {
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError>;
}
