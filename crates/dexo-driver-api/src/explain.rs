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
}

impl ExplainRequest {
    pub fn estimated(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            analyze: false,
        }
    }

    pub fn analyzed(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            analyze: true,
        }
    }
}

#[async_trait::async_trait]
pub trait ExplainProvider: Send + Sync {
    async fn explain(&self, request: ExplainRequest) -> Result<ExplainPlan, DriverError>;
}
