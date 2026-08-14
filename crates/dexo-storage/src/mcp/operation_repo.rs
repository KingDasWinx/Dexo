use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::operation::{OperationRecord, OperationState, SideEffect, replay_or_conflict};
use rusqlite::{Connection, OptionalExtension, params};

pub fn reserve(conn: &Connection, record: OperationRecord) -> Result<OperationRecord, AppError> {
    let existing = conn
        .query_row(
            "SELECT profile_name, session_id, operation_id, tool, payload_hash, state, side_effect, result
             FROM mcp_operations
             WHERE profile_name = ?1 AND session_id = ?2 AND operation_id = ?3",
            params![record.profile, record.session, record.operation_id],
            row_to_record,
        )
        .optional()
        .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?;
    if let Some(existing) = existing {
        return replay_or_conflict(&existing, &record.payload_hash);
    }
    conn.execute(
        "INSERT INTO mcp_operations (
            profile_name, session_id, operation_id, tool, payload_hash, state, side_effect, result, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%s','now'))",
        params![
            record.profile,
            record.session,
            record.operation_id,
            record.tool,
            record.payload_hash,
            state_name(record.state),
            effect_name(record.side_effect),
            record.result,
        ],
    )
    .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?;
    Ok(record)
}

pub fn lookup(
    conn: &Connection,
    profile: &str,
    session: &str,
    operation_id: &str,
) -> anyhow::Result<Option<OperationRecord>> {
    conn.query_row(
        "SELECT profile_name, session_id, operation_id, tool, payload_hash, state, side_effect, result
         FROM mcp_operations
         WHERE profile_name = ?1 AND session_id = ?2 AND operation_id = ?3",
        params![profile, session, operation_id],
        row_to_record,
    )
    .optional()
    .map_err(Into::into)
}

pub fn finish(
    conn: &Connection,
    profile: &str,
    session: &str,
    operation_id: &str,
    state: OperationState,
    side_effect: SideEffect,
    result: String,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_operations SET state = ?1, side_effect = ?2, result = ?3
         WHERE profile_name = ?4 AND session_id = ?5 AND operation_id = ?6",
        params![
            state_name(state),
            effect_name(side_effect),
            result,
            profile,
            session,
            operation_id
        ],
    )?;
    Ok(())
}

fn state_name(state: OperationState) -> &'static str {
    match state {
        OperationState::Running => "running",
        OperationState::Succeeded => "succeeded",
        OperationState::Failed => "failed",
        OperationState::Unknown => "unknown",
    }
}

fn effect_name(effect: SideEffect) -> &'static str {
    match effect {
        SideEffect::Committed => "committed",
        SideEffect::RolledBack => "rolled_back",
        SideEffect::PartiallyCommitted => "partial",
        SideEffect::Unknown => "unknown",
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRecord> {
    Ok(OperationRecord {
        profile: row.get(0)?,
        session: row.get(1)?,
        operation_id: row.get(2)?,
        tool: row.get(3)?,
        payload_hash: row.get(4)?,
        state: match row.get::<_, String>(5)?.as_str() {
            "succeeded" => OperationState::Succeeded,
            "failed" => OperationState::Failed,
            "unknown" => OperationState::Unknown,
            _ => OperationState::Running,
        },
        side_effect: match row.get::<_, String>(6)?.as_str() {
            "committed" => SideEffect::Committed,
            "rolled_back" => SideEffect::RolledBack,
            "partial" => SideEffect::PartiallyCommitted,
            _ => SideEffect::Unknown,
        },
        result: row.get(7)?,
    })
}
