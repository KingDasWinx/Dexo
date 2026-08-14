use dexo_app::catalog_service::parse_qualified;
use dexo_app::data::{ChangeSet, ColumnDef, RowIdentity, TableMeta, mutations_for};
use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::McpService;
use dexo_app::mcp::audit::{AuditEvent, SqlAuditMode};
use dexo_app::mcp::grant::WRITE_TOOLS;
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::operation::{OperationRecord, OperationState, SideEffect, payload_hash};
use dexo_app::mcp::selector::ObjectRef;
use dexo_app::query_service::map_driver_error;
use dexo_app::schema::apply::{ApplyRequest, apply_change};
use dexo_app::schema::change::drop_table;
use dexo_app::schema::security::production_policy;
use dexo_driver_api::{
    AdminAction, DbValue, DdlOutcome, DdlPlan, Mutation, ObjectKind, QueryRequest, SchemaChange,
    Session,
};
use serde_json::{Map, Value};

pub fn write_tool_names(ledger: &dyn GrantLedger, profile: &str, now: i64) -> Vec<String> {
    let mut tools = Vec::new();
    for grant in ledger.active_grants(profile, now) {
        for tool in grant.tools {
            if WRITE_TOOLS.contains(&tool.as_str()) && !tools.contains(&tool) {
                tools.push(tool);
            }
        }
    }
    tools
}

pub fn is_grant_management(name: &str) -> bool {
    matches!(name, "grant_create" | "grant_revoke" | "grant_list")
}

pub async fn call_write_tool(
    service: &McpService,
    ledger: &dyn GrantLedger,
    session: Option<&dyn Session>,
    session_id: &str,
    name: &str,
    arguments: Map<String, Value>,
    now: i64,
) -> Result<String, AppError> {
    if is_grant_management(name) {
        audit(
            ledger,
            service,
            name,
            None,
            "",
            "deny",
            None,
            "not found",
            now,
            arguments.get("sql").and_then(Value::as_str),
        );
        return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
    }
    let value = Value::Object(arguments.clone());
    let operation_id = arguments
        .get("operation_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::new(ErrorCategory::Configuration, "operation_id is required"))?;
    let target = arguments
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let object = ObjectRef::parse(target);
    if let Some(existing) = ledger.lookup_operation(&service.profile.name, session_id, operation_id)
    {
        let replayed =
            dexo_app::mcp::operation::replay_or_conflict(&existing, &payload_hash(&value))?;
        audit(
            ledger,
            service,
            name,
            Some(operation_id),
            target,
            "replay",
            None,
            &replayed.result,
            now,
            None,
        );
        return Ok(replayed.result);
    }
    let grants = ledger.active_grants(&service.profile.name, now);
    let grant = grants
        .iter()
        .find(|grant| grant.authorizes(name, &object, now))
        .cloned()
        .ok_or_else(|| {
            audit(
                ledger,
                service,
                name,
                Some(operation_id),
                target,
                "deny",
                None,
                "not found",
                now,
                arguments.get("sql").and_then(Value::as_str),
            );
            AppError::new(ErrorCategory::McpPolicy, "not found")
        })?;
    let record = OperationRecord {
        profile: service.profile.name.clone(),
        session: session_id.into(),
        operation_id: operation_id.into(),
        tool: name.into(),
        payload_hash: payload_hash(&value),
        state: OperationState::Running,
        side_effect: SideEffect::Unknown,
        result: String::new(),
    };
    let reserved = ledger.reserve_operation(record)?;
    if reserved.state != OperationState::Running {
        audit(
            ledger,
            service,
            name,
            Some(operation_id),
            target,
            "replay",
            Some(&grant.id.to_string()),
            &reserved.result,
            now,
            None,
        );
        return Ok(reserved.result);
    }
    if ledger.consume(grant.id, now).is_err() {
        let result = format_outcome(OperationState::Failed, SideEffect::RolledBack, "revoked");
        ledger.finish_operation(
            &service.profile.name,
            session_id,
            operation_id,
            OperationState::Failed,
            SideEffect::RolledBack,
            result.clone(),
        )?;
        return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
    }
    if ledger.is_revoked(grant.id) {
        let result = format_outcome(OperationState::Failed, SideEffect::RolledBack, "revoked");
        ledger.finish_operation(
            &service.profile.name,
            session_id,
            operation_id,
            OperationState::Failed,
            SideEffect::RolledBack,
            result.clone(),
        )?;
        return Ok(result);
    }
    let outcome = execute(service, name, &value, session, || {
        ledger.is_revoked(grant.id)
    })
    .await;
    let (state, side_effect, text) = match &outcome {
        Ok((effect, text)) => (OperationState::Succeeded, *effect, text.clone()),
        Err(error) => (
            OperationState::Failed,
            SideEffect::RolledBack,
            error.to_string(),
        ),
    };
    let result = format_outcome(state, side_effect, &text);
    ledger.finish_operation(
        &service.profile.name,
        session_id,
        operation_id,
        state,
        side_effect,
        result.clone(),
    )?;
    audit(
        ledger,
        service,
        name,
        Some(operation_id),
        target,
        "allow",
        Some(&grant.id.to_string()),
        &result,
        now,
        arguments.get("sql").and_then(Value::as_str),
    );
    match outcome {
        Ok(_) => Ok(result),
        Err(error) => Err(error),
    }
}

fn format_outcome(state: OperationState, side_effect: SideEffect, text: &str) -> String {
    format!("{state:?} {side_effect:?} {text}")
}

#[allow(clippy::too_many_arguments)]
fn audit(
    ledger: &dyn GrantLedger,
    service: &McpService,
    tool: &str,
    operation_id: Option<&str>,
    target: &str,
    decision: &str,
    grant_id: Option<&str>,
    status: &str,
    now: i64,
    sql: Option<&str>,
) {
    ledger.record_audit(
        AuditEvent {
            timestamp: now,
            request: format!("tools/call {tool}"),
            operation_id: operation_id.map(str::to_string),
            profile: service.profile.name.clone(),
            client: "mcp".into(),
            target: target.into(),
            decision: decision.into(),
            grant_id: grant_id.map(str::to_string),
            duration_ms: 0,
            rows: 0,
            bytes: 0,
            status: status.into(),
            sql: None,
        }
        .sanitize(SqlAuditMode::Hash, sql),
    );
}

async fn execute(
    service: &McpService,
    name: &str,
    value: &Value,
    session: Option<&dyn Session>,
    cancelled: impl Fn() -> bool,
) -> Result<(SideEffect, String), AppError> {
    match name {
        "data_insert" | "data_update" | "data_delete" => {
            let mutations = data_mutations(name, value)?;
            if cancelled() {
                return Ok((SideEffect::RolledBack, "rolled_back".into()));
            }
            apply_mutations(session, &mutations).await
        }
        "data_execute_sql" => {
            let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
            service.authorize_write_sql(sql)?;
            if cancelled() {
                return Ok((SideEffect::RolledBack, "rolled_back".into()));
            }
            let Some(session) = session else {
                return Ok((SideEffect::Unknown, "session required".into()));
            };
            let _ = session
                .execute(QueryRequest::write(sql))
                .await
                .map_err(map_driver_error)?;
            Ok((SideEffect::Committed, "sql applied".into()))
        }
        "schema_apply_ddl" => apply_ddl(value, session, cancelled()).await,
        "admin_cancel_query" | "admin_terminate_session" => {
            if cancelled() {
                return Ok((SideEffect::RolledBack, "rolled_back".into()));
            }
            apply_admin(name, value, session).await
        }
        _ => Err(AppError::new(ErrorCategory::McpPolicy, "not found")),
    }
}

fn data_mutations(name: &str, value: &Value) -> Result<Vec<Mutation>, AppError> {
    let target = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let table = parse_qualified(target);
    let values = object_pairs(value.get("values"));
    let identity = object_pairs(value.get("identity"));
    let columns: Vec<ColumnDef> = if values.is_empty() {
        identity
            .iter()
            .map(|(name, _)| ColumnDef {
                name: name.clone(),
                primary_key: true,
                unique: true,
                nullable: false,
            })
            .collect()
    } else {
        values
            .iter()
            .enumerate()
            .map(|(index, (name, _))| ColumnDef {
                name: name.clone(),
                primary_key: index == 0,
                unique: index == 0,
                nullable: false,
            })
            .collect()
    };
    let meta = TableMeta { columns };
    let mut changes = ChangeSet::for_table(&meta);
    match name {
        "data_insert" => changes.insert(values),
        "data_update" => {
            let row = RowIdentity {
                columns: identity.iter().map(|(name, _)| name.clone()).collect(),
                values: identity.iter().map(|(_, value)| value.clone()).collect(),
            };
            changes.update(row, identity.clone(), values);
        }
        "data_delete" => {
            let row = RowIdentity {
                columns: identity.iter().map(|(name, _)| name.clone()).collect(),
                values: identity.iter().map(|(_, value)| value.clone()).collect(),
            };
            changes.delete(row, identity);
        }
        _ => {}
    }
    mutations_for(table, &changes)
        .map_err(|error| AppError::new(ErrorCategory::Configuration, error.to_string()))
}

async fn apply_mutations(
    session: Option<&dyn Session>,
    mutations: &[Mutation],
) -> Result<(SideEffect, String), AppError> {
    let Some(session) = session else {
        return Ok((SideEffect::Unknown, "session required".into()));
    };
    session
        .data()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "data writer unavailable"))?
        .apply(mutations)
        .await
        .map_err(map_driver_error)?;
    Ok((SideEffect::Committed, "applied".into()))
}

async fn apply_ddl(
    value: &Value,
    session: Option<&dyn Session>,
    cancelled: bool,
) -> Result<(SideEffect, String), AppError> {
    let target = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
    let implicit = value
        .get("implicit_commit")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| sql.to_ascii_uppercase().contains("DROP "));
    let change = if sql.trim_start().to_ascii_uppercase().starts_with("DROP") {
        drop_table(target)
    } else {
        SchemaChange::DropObject {
            target: parse_qualified(target),
            kind: ObjectKind::Table,
        }
    };
    let mut plan = DdlPlan::default();
    plan.push(sql, implicit);
    let Some(session) = session else {
        if cancelled && !implicit {
            return Ok((SideEffect::RolledBack, "rolled_back".into()));
        }
        return Ok((
            if implicit {
                SideEffect::Committed
            } else {
                SideEffect::Unknown
            },
            "session required".into(),
        ));
    };
    let executor = session
        .ddl()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "ddl unavailable"))?;
    let confirm = value.get("confirm_target").and_then(Value::as_str);
    let outcome = apply_change(
        executor,
        ApplyRequest {
            change: &change,
            plan: &plan,
            policy: &production_policy(),
            typed_confirmation: confirm.or(Some(target)),
            cancelled: cancelled && !implicit,
        },
    )
    .await?;
    Ok(map_ddl_outcome(outcome))
}

fn map_ddl_outcome(outcome: DdlOutcome) -> (SideEffect, String) {
    match outcome {
        DdlOutcome::Committed => (SideEffect::Committed, "committed".into()),
        DdlOutcome::RolledBack => (SideEffect::RolledBack, "rolled_back".into()),
        DdlOutcome::PartiallyCommitted { committed } => (
            SideEffect::PartiallyCommitted,
            format!("partial {committed}"),
        ),
        DdlOutcome::Unknown => (SideEffect::Unknown, "unknown".into()),
    }
}

async fn apply_admin(
    name: &str,
    value: &Value,
    session: Option<&dyn Session>,
) -> Result<(SideEffect, String), AppError> {
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let action = if name == "admin_terminate_session" {
        AdminAction::TerminateSession {
            session_id: session_id.clone(),
        }
    } else {
        AdminAction::CancelQuery {
            session_id: session_id.clone(),
        }
    };
    let decision = dexo_app::admin_service::evaluate(
        &action,
        "",
        &dexo_app::admin_service::production_policy(),
    );
    if !decision.allowed {
        return Err(AppError::new(ErrorCategory::Permission, "admin denied"));
    }
    if name == "admin_terminate_session"
        && value.get("confirm_target").and_then(Value::as_str) != Some(session_id.as_str())
    {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {session_id} to confirm"),
        ));
    }
    let Some(session) = session else {
        return Ok((SideEffect::Unknown, "session required".into()));
    };
    let outcome = session
        .admin()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "admin unavailable"))?
        .execute_action(action)
        .await
        .map_err(map_driver_error)?;
    Ok((
        if outcome.ok {
            SideEffect::Committed
        } else {
            SideEffect::Unknown
        },
        outcome.message,
    ))
}

fn object_pairs(value: Option<&Value>) -> Vec<(String, DbValue)> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, value)| (name.clone(), json_value(value)))
        .collect()
}

fn json_value(value: &Value) -> DbValue {
    match value {
        Value::Null => DbValue::Null,
        Value::Bool(flag) => DbValue::Bool(*flag),
        Value::Number(number) => number
            .as_i64()
            .map(DbValue::I64)
            .or_else(|| number.as_u64().map(DbValue::U64))
            .unwrap_or_else(|| DbValue::Text(number.to_string())),
        Value::String(text) => DbValue::Text(text.clone()),
        other => DbValue::Json(other.to_string()),
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{call_write_tool, is_grant_management, write_tool_names};
    use dexo_app::mcp::McpService;
    use dexo_app::mcp::audit::{SECRET_SENTINEL, contains_secret};
    use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
    use dexo_app::mcp::ledger::{GrantLedger, MemoryGrantLedger};
    use dexo_app::mcp::profile::McpProfile;
    use dexo_app::mcp::selector::{Effect, SelectorRule};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use serde_json::json;

    fn profile() -> McpProfile {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        profile
    }

    fn service() -> McpService {
        McpService::new(
            profile(),
            vec![CatalogObject::new(
                ObjectId::new("items"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "items"),
                None,
            )],
        )
    }

    fn grant(capability: GrantCapability, tool: &str, selector: &str) -> Grant {
        Grant::new(
            &profile(),
            "local",
            capability,
            vec![tool.into()],
            vec![SelectorRule::parse(Effect::Allow, selector).unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap()
    }

    async fn call(
        ledger: &MemoryGrantLedger,
        tool: &str,
        payload: serde_json::Value,
    ) -> Result<String, dexo_app::AppError> {
        call_write_tool(
            &service(),
            ledger,
            None,
            "s",
            tool,
            payload.as_object().cloned().unwrap(),
            0,
        )
        .await
    }

    #[test]
    fn catalog_has_no_grant_management_tools() {
        assert!(!is_grant_management("data_insert"));
        assert!(is_grant_management("grant_create"));
        let ledger = MemoryGrantLedger::default();
        assert!(write_tool_names(&ledger, "assistant", 0).is_empty());
    }

    #[tokio::test]
    async fn data_write_cannot_ddl_and_ddl_cannot_terminate() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let denied = call(
            &ledger,
            "schema_apply_ddl",
            json!({"operation_id":"op-ddl","target":"db.public.items","sql":"DROP TABLE items"}),
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("not found"));
        ledger
            .insert_grant(grant(
                GrantCapability::Ddl,
                "schema_apply_ddl",
                "db.public.items",
            ))
            .unwrap();
        let denied = call(
            &ledger,
            "admin_terminate_session",
            json!({"operation_id":"op-term","target":"db.public.items","session_id":"9"}),
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn same_operation_and_payload_executes_once() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let payload = json!({
            "operation_id":"op-1",
            "target":"db.public.items",
            "values":{"id":7}
        });
        let first = call(&ledger, "data_insert", payload.clone()).await.unwrap();
        let replay = call(&ledger, "data_insert", payload).await.unwrap();
        assert_eq!(first, replay);
        assert!(first.contains("Unknown"));
        let conflict = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-1","target":"db.public.items","values":{"id":8}}),
        )
        .await
        .unwrap_err();
        assert!(conflict.to_string().contains("different payload"));
    }

    #[tokio::test]
    async fn revoke_before_dispatch_hides_grant() {
        let ledger = MemoryGrantLedger::default();
        let grant = grant(GrantCapability::DataWrite, "data_insert", "db.public.items");
        let id = grant.id;
        ledger.insert_grant(grant).unwrap();
        ledger.revoke(id).unwrap();
        let error = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-r","target":"db.public.items","values":{"id":1}}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn mysql_ddl_commit_is_not_claimed_reversed() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::Ddl,
                "schema_apply_ddl",
                "db.public.items",
            ))
            .unwrap();
        let result = call(
            &ledger,
            "schema_apply_ddl",
            json!({
                "operation_id":"op-mysql",
                "target":"db.public.items",
                "sql":"DROP TABLE items",
                "implicit_commit":true,
                "confirm_target":"db.public.items"
            }),
        )
        .await
        .unwrap();
        assert!(result.contains("Committed"));
        assert!(!result.contains("RolledBack"));
    }

    #[tokio::test]
    async fn audit_omits_results_and_secret_sentinel() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let _ = call(
            &ledger,
            "data_insert",
            json!({
                "operation_id":"op-secret",
                "target":"db.public.items",
                "values":{"id":1},
                "sql": format!("select '{SECRET_SENTINEL}'")
            }),
        )
        .await;
        let _ = call(&ledger, "grant_create", json!({"operation_id":"op-g"})).await;
        let blob = ledger
            .audits()
            .into_iter()
            .map(|event| event.export_line())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!contains_secret(&blob));
        assert!(blob.contains("allow") || blob.contains("deny"));
        assert!(!blob.contains("select "));
    }

    #[tokio::test]
    async fn data_execute_sql_rejects_ddl() {
        let mut profile = profile();
        profile.tool_rules.push(dexo_app::mcp::profile::ToolRule {
            tool: "data_execute_sql".into(),
            allowed: true,
        });
        let grant = Grant::new(
            &profile,
            "local",
            GrantCapability::DataWrite,
            vec!["data_execute_sql".into()],
            vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap();
        let ledger = MemoryGrantLedger::default();
        ledger.insert_grant(grant).unwrap();
        let service = McpService::new(profile, Vec::new());
        let error = call_write_tool(
            &service,
            &ledger,
            None,
            "s",
            "data_execute_sql",
            json!({
                "operation_id":"op-sql",
                "target":"db.public.items",
                "sql":"DROP TABLE items"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("outside grant capability"));
    }
}
