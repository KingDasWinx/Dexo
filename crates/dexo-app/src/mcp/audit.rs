use serde::{Deserialize, Serialize};

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
