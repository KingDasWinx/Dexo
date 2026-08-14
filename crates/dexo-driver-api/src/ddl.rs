use serde::{Deserialize, Serialize};

use crate::{DriverError, ObjectId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObjectDdl {
    pub object_id: ObjectId,
    pub sql: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DdlStatement {
    pub sql: String,
    pub implicit_commit: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DdlPlan {
    pub statements: Vec<DdlStatement>,
    pub rollback: Vec<String>,
    pub warnings: Vec<String>,
    pub transactional: bool,
}

impl DdlPlan {
    pub fn push(&mut self, sql: impl Into<String>, implicit_commit: bool) {
        self.statements.push(DdlStatement {
            sql: sql.into(),
            implicit_commit,
        });
        if implicit_commit {
            self.transactional = false;
        }
    }

    pub fn sqls(&self) -> impl Iterator<Item = &str> {
        self.statements
            .iter()
            .map(|statement| statement.sql.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DdlOutcome {
    RolledBack,
    Committed,
    PartiallyCommitted { committed: usize },
    Unknown,
}

#[async_trait::async_trait]
pub trait DdlExecutor: Send + Sync {
    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError>;
}

#[async_trait::async_trait]
pub trait SecurityAdmin: Send + Sync {
    async fn list_grants(
        &self,
        principal: Option<&crate::QualifiedName>,
    ) -> Result<Vec<crate::GrantRecord>, DriverError>;

    async fn effective_privileges(
        &self,
        principal: &crate::QualifiedName,
        object: &crate::QualifiedName,
    ) -> Result<Vec<String>, DriverError>;

    async fn set_password(
        &self,
        principal: &crate::QualifiedName,
        password: &secrecy::SecretString,
    ) -> Result<(), DriverError>;
}
