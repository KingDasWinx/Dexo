use serde::{Deserialize, Serialize};

use crate::mcp::grant::Grant;

pub const SECRET_SENTINEL: &str = "SUPER_SECRET_SENTINEL";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SqlAuditMode {
    None,
    Hash,
    Sanitized,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: i64,
    pub request: String,
    pub operation_id: Option<String>,
    pub profile: String,
    pub client: String,
    pub target: String,
    pub decision: String,
    pub grant_id: Option<String>,
    pub duration_ms: u64,
    pub rows: u64,
    pub bytes: u64,
    pub status: String,
    pub sql: Option<String>,
}

impl AuditEvent {
    /// A person gave a profile write access to a connection, or took it back: who could
    /// write, and from when, is part of the record of what was written.
    pub fn access(grant: &Grant, given: bool) -> Self {
        let kinds: Vec<&str> = grant
            .kinds()
            .into_iter()
            .map(|kind| kind.as_str())
            .collect();
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs() as i64),
            request: if given {
                "dexo give_access"
            } else {
                "dexo revoke_access"
            }
            .into(),
            operation_id: None,
            profile: grant.profile.clone(),
            client: "dexo".into(),
            target: grant.connection.clone(),
            decision: "allow".into(),
            grant_id: Some(grant.id.to_string()),
            duration_ms: 0,
            rows: 0,
            bytes: 0,
            status: format!(
                "{} {}",
                if given {
                    "may write"
                } else {
                    "may no longer write"
                },
                kinds.join(", ")
            ),
            sql: None,
        }
    }

    pub fn sanitize(mut self, mode: SqlAuditMode, sql: Option<&str>) -> Self {
        self.sql = match (mode, sql) {
            (SqlAuditMode::None, _) => None,
            (SqlAuditMode::Hash, Some(sql)) => {
                Some(crate::mcp::operation::payload_hash(&serde_json::json!(sql)))
            }
            (SqlAuditMode::Sanitized, Some(_)) => Some("?".into()),
            (_, None) => None,
        };
        self
    }

    pub fn export_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub fn contains_secret(text: &str) -> bool {
    text.contains(SECRET_SENTINEL)
}

/// The grants that went, then the grants that came, between two lists of live grants.
pub fn access_changes(before: &[Grant], after: &[Grant]) -> Vec<AuditEvent> {
    let gone = before
        .iter()
        .filter(|grant| !after.iter().any(|now| now.id == grant.id))
        .map(|grant| AuditEvent::access(grant, false));
    let came = after
        .iter()
        .filter(|grant| !before.iter().any(|then| then.id == grant.id))
        .map(|grant| AuditEvent::access(grant, true));
    gone.chain(came).collect()
}

#[cfg(test)]
mod tests {
    use super::{AuditEvent, SECRET_SENTINEL, SqlAuditMode, contains_secret};

    #[test]
    fn audit_omits_results_and_secret_sentinel() {
        let event = AuditEvent {
            timestamp: 1,
            request: "tools/call".into(),
            operation_id: Some("op-1".into()),
            profile: "assistant".into(),
            client: "test".into(),
            target: "db.public.items".into(),
            decision: "allow".into(),
            grant_id: None,
            duration_ms: 3,
            rows: 1,
            bytes: 8,
            status: "ok".into(),
            sql: None,
        }
        .sanitize(
            SqlAuditMode::Sanitized,
            Some(&format!("select '{SECRET_SENTINEL}'")),
        );
        let line = event.export_line();
        assert!(!contains_secret(&line));
        assert!(!line.contains("select "));
        assert_eq!(event.sql.as_deref(), Some("?"));
    }
}
