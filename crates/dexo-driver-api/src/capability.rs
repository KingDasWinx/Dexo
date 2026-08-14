#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityState {
    pub capability: Capability,
    pub available: bool,
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
