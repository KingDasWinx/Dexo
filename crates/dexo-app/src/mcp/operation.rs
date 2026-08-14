use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OperationState {
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SideEffect {
    Committed,
    RolledBack,
    PartiallyCommitted,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub profile: String,
    pub session: String,
    pub operation_id: String,
    pub tool: String,
    pub payload_hash: String,
    pub state: OperationState,
    pub side_effect: SideEffect,
    pub result: String,
}

pub fn payload_hash(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

pub fn replay_or_conflict(
    existing: &OperationRecord,
    payload_hash: &str,
) -> Result<OperationRecord, AppError> {
    if existing.payload_hash != payload_hash {
        return Err(AppError::new(
            ErrorCategory::Conflict,
            "operation_id was reused with a different payload",
        ));
    }
    if existing.state == OperationState::Unknown {
        return Err(AppError::new(
            ErrorCategory::Conflict,
            "unknown outcome must not be retried",
        ));
    }
    Ok(existing.clone())
}

#[cfg(test)]
mod tests {
    use super::{OperationRecord, OperationState, SideEffect, payload_hash, replay_or_conflict};
    use serde_json::json;

    fn record(hash: &str, state: OperationState) -> OperationRecord {
        OperationRecord {
            profile: "assistant".into(),
            session: "s".into(),
            operation_id: "op-1".into(),
            tool: "data_insert".into(),
            payload_hash: hash.into(),
            state,
            side_effect: SideEffect::Committed,
            result: "ok".into(),
        }
    }

    #[test]
    fn same_payload_replays_and_different_payload_conflicts() {
        let hash = payload_hash(&json!({"id": 7}));
        let first = record(&hash, OperationState::Succeeded);
        let replay = replay_or_conflict(&first, &hash).unwrap();
        assert_eq!(first, replay);
        assert!(replay_or_conflict(&first, &payload_hash(&json!({"id": 8}))).is_err());
        assert!(replay_or_conflict(&record(&hash, OperationState::Unknown), &hash).is_err());
    }
}
