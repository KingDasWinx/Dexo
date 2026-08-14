use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryDocumentDraft {
    pub id: String,
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryCheckpoint {
    pub documents: Vec<RecoveryDocumentDraft>,
    pub layout_json: Option<String>,
    pub transaction: String,
}

#[derive(Debug)]
pub struct CheckpointDebounce {
    last: Option<Instant>,
    interval: Duration,
}

impl CheckpointDebounce {
    pub fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
        }
    }

    pub fn due(&mut self) -> bool {
        match self.last {
            Some(last) if last.elapsed() < self.interval => false,
            _ => {
                self.last = Some(Instant::now());
                true
            }
        }
    }
}

pub fn sanitize_checkpoint(
    documents: Vec<RecoveryDocumentDraft>,
    layout_json: Option<String>,
    had_active_transaction: bool,
    secrets: &[&str],
) -> RecoveryCheckpoint {
    RecoveryCheckpoint {
        documents: documents
            .into_iter()
            .map(|mut doc| {
                doc.content = redact(&doc.content, secrets);
                doc.title = redact(&doc.title, secrets);
                doc
            })
            .collect(),
        layout_json: layout_json.map(|json| redact(&json, secrets)),
        transaction: if had_active_transaction {
            "unknown".into()
        } else {
            "idle".into()
        },
    }
}

pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

pub fn contains_session_handle(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("session_id=") || lower.contains("backend-pid")
}

#[cfg(test)]
mod tests {
    use super::{
        CheckpointDebounce, RecoveryDocumentDraft, contains_session_handle, sanitize_checkpoint,
    };
    use std::time::Duration;

    #[test]
    fn strips_secrets_and_never_keeps_active_tx() {
        let checkpoint = sanitize_checkpoint(
            vec![RecoveryDocumentDraft {
                id: "1".into(),
                title: "scratch".into(),
                content: "select 'SUPER_SECRET_SENTINEL'".into(),
            }],
            Some("{\"session_handle\":\"nope\",\"password\":\"SUPER_SECRET_SENTINEL\"}".into()),
            true,
            &["SUPER_SECRET_SENTINEL"],
        );
        assert_eq!(checkpoint.transaction, "unknown");
        assert!(!checkpoint.documents[0].content.contains("SUPER_SECRET_SENTINEL"));
        assert!(!checkpoint
            .layout_json
            .as_deref()
            .unwrap()
            .contains("SUPER_SECRET_SENTINEL"));
        assert!(!contains_session_handle(&checkpoint.documents[0].content));
    }

    #[test]
    fn debounce_skips_burst() {
        let mut debounce = CheckpointDebounce::new(Duration::from_secs(30));
        assert!(debounce.due());
        assert!(!debounce.due());
    }
}
