#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Catalog,
    Query,
    Cancel,
    Transactions,
    DataWrite,
    Ddl,
    Explain,
    ExplainAnalyze,
    Admin,
    Import,
    Export,
    /// A dump and restore of the whole database through the database's own tools.
    Backup,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityState {
    pub capability: Capability,
    pub available: bool,
    #[serde(default)]
    reason: Option<String>,
}

impl CapabilityState {
    pub fn available(capability: Capability) -> Self {
        Self {
            capability,
            available: true,
            reason: None,
        }
    }

    pub fn unavailable(capability: Capability, reason: impl Into<String>) -> Self {
        Self {
            capability,
            available: false,
            reason: Some(reason.into()),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}
