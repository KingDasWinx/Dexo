use dexo_app::catalog_service::parse_qualified;
use dexo_app::data::{ChangeSet, ColumnDef, RowIdentity, TableMeta, mutations_for};
use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::audit::{AuditEvent, SqlAuditMode};
use dexo_app::mcp::grant::WRITE_TOOLS;
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::operation::{OperationRecord, OperationState, SideEffect, payload_hash};
use dexo_app::mcp::selector::ObjectRef;
use dexo_app::mcp::{McpConnection, McpService};
use dexo_app::query_service::map_driver_error;
use dexo_driver_api::{
    AdminAction, DbValue, DdlOutcome, DdlPlan, Mutation, Session, classify_raw_sql,
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
    target: Option<(&McpConnection, &dyn Session)>,
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
    let Some((connection, session)) = target else {
        return Err(AppError::new(
            ErrorCategory::Capability,
            "no connection is open for this profile",
        ));
    };
    if !service.profile.tool_allowed(name) {
        return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
    }
    connection.accepts_writes()?;
    let sql = arguments
        .get("sql")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let value = Value::Object(arguments.clone());
    let operation_id = arguments
        .get("operation_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::new(ErrorCategory::Configuration, "operation_id is required"))?;
    let target_name = arguments
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let object = connection.qualify(&ObjectRef::parse(target_name).path);
    if let Some(existing) = ledger.lookup_operation(&service.profile.name, session_id, operation_id)
    {
        let replayed =
            dexo_app::mcp::operation::replay_or_conflict(&existing, &payload_hash(&value))?;
        audit(
            ledger,
            service,
            name,
            Some(operation_id),
            target_name,
            "replay",
            None,
            &replayed.result,
            now,
            None,
        );
        return Ok(replayed.result);
    }
    let targets = match name {
        "data_execute_sql" => service.data_write_targets(connection, sql)?,
        "schema_apply_ddl" => service.schema_write_targets(connection, sql)?,
        _ => vec![object],
    };
    let profile_policy = service.policy();
    let grants = ledger.active_grants(&service.profile.name, now);
    let grant = grants
        .iter()
        .find(|grant| {
            targets.iter().all(|target| {
                grant.authorizes(name, &connection.name, target, &profile_policy, now)
            })
        })
        .cloned()
        .ok_or_else(|| {
            audit(
                ledger,
                service,
                name,
                Some(operation_id),
                target_name,
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
            target_name,
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
    let outcome = execute(service, name, &value, session, connection, || {
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
        target_name,
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
    session: &dyn Session,
    connection: &McpConnection,
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
            if cancelled() {
                return Ok((SideEffect::RolledBack, "rolled_back".into()));
            }
            let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
            let affected = service.execute_write(session, connection, sql).await?;
            Ok((SideEffect::Committed, format!("{affected} rows affected")))
        }
        "schema_apply_ddl" => apply_ddl(value, session, connection, cancelled()).await,
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
    session: &dyn Session,
    mutations: &[Mutation],
) -> Result<(SideEffect, String), AppError> {
    session
        .data()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "data writer unavailable"))?
        .apply(mutations)
        .await
        .map_err(map_driver_error)?;
    Ok((SideEffect::Committed, "applied".into()))
}

/// Raw DDL from a client cannot be turned into a structured `SchemaChange`, so the risk
/// comes from the driver-api classifier and the typed confirmation is required from the
/// client, never filled in from the target it already sent. MySQL commits DDL implicitly;
/// that is a property of the connection, not something the client gets to claim.
async fn apply_ddl(
    value: &Value,
    session: &dyn Session,
    connection: &McpConnection,
    cancelled: bool,
) -> Result<(SideEffect, String), AppError> {
    let target = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
    let risk = classify_raw_sql(sql);
    if (risk.destructive || risk.data_loss)
        && value.get("confirm_target").and_then(Value::as_str) != Some(target)
    {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {target} as confirm_target to confirm"),
        ));
    }
    if cancelled {
        return Ok((SideEffect::RolledBack, "rolled_back".into()));
    }
    let mut plan = DdlPlan::default();
    plan.push(sql, connection.dialect == dexo_sql::Dialect::Mysql);
    let outcome = session
        .ddl()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "ddl unavailable"))?
        .apply_ddl(&plan)
        .await
        .map_err(map_driver_error)?;
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
    session: &dyn Session,
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
    use dexo_app::mcp::McpConnection;
    use dexo_app::mcp::McpService;
    use dexo_app::mcp::audit::{SECRET_SENTINEL, contains_secret};
    use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
    use dexo_app::mcp::ledger::{GrantLedger, MemoryGrantLedger};
    use dexo_app::mcp::profile::McpProfile;
    use dexo_app::mcp::selector::{Effect, SelectorRule};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use dexo_test_support::FakeSession;
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

    fn connection(name: &str) -> McpConnection {
        McpConnection {
            name: name.into(),
            driver: "postgres".into(),
            dialect: dexo_sql::Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: dexo_app::Environment::Local,
            read_only: false,
        }
    }

    fn service_with(allow: &str) -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, allow).unwrap()];
        McpService::new(profile, Vec::new())
    }

    fn grant_on(
        profile: &McpProfile,
        capability: GrantCapability,
        tool: &str,
        selector: &str,
    ) -> Grant {
        Grant::new(
            profile,
            "local",
            capability,
            vec![tool.into()],
            vec![SelectorRule::parse(Effect::Allow, selector).unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap()
    }

    async fn call_on(
        ledger: &MemoryGrantLedger,
        session: &FakeSession,
        connection_name: &str,
        tool: &str,
        payload: serde_json::Value,
    ) -> Result<String, dexo_app::AppError> {
        let connection = connection(connection_name);
        call_write_tool(
            &service(),
            ledger,
            Some((&connection, session as &dyn dexo_driver_api::Session)),
            "s",
            tool,
            payload.as_object().cloned().unwrap(),
            0,
        )
        .await
    }

    async fn call(
        ledger: &MemoryGrantLedger,
        tool: &str,
        payload: serde_json::Value,
    ) -> Result<String, dexo_app::AppError> {
        call_on(ledger, &FakeSession::default(), "local", tool, payload).await
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
        let session = FakeSession::default().with_keys(vec![dexo_driver_api::ColumnKeyInfo {
            name: "id".into(),
            primary_key: true,
            unique: true,
        }]);
        let payload = json!({"operation_id":"op-1","target":"db.public.items","values":{"id":7}});
        let first = call_on(&ledger, &session, "local", "data_insert", payload.clone())
            .await
            .unwrap();
        let replay = call_on(&ledger, &session, "local", "data_insert", payload)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert!(first.contains("Committed"), "{first}");
        assert_eq!(
            session
                .log()
                .iter()
                .filter(|entry| entry.starts_with("apply"))
                .count(),
            1
        );
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
    async fn writes_without_an_open_connection_fail_before_the_grant_is_spent() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let error = call_write_tool(
            &service(),
            &ledger,
            None,
            "s",
            "data_insert",
            json!({"operation_id":"op-x","target":"db.public.items","values":{"id":1}})
                .as_object()
                .cloned()
                .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("no connection"));
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }

    #[tokio::test]
    async fn a_grant_cannot_reach_what_the_profile_denies() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.*",
            ))
            .unwrap();
        let error = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-s","target":"db.public.secrets","values":{"id":1}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
    }

    #[tokio::test]
    async fn a_grant_is_bound_to_its_connection() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let error = call_on(
            &ledger,
            &FakeSession::default(),
            "other",
            "data_insert",
            json!({"operation_id":"op-c","target":"db.public.items","values":{"id":1}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
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
        let connection = connection("local");
        let session = FakeSession::default();
        let error = call_write_tool(
            &service,
            &ledger,
            Some((&connection, &session as &dyn dexo_driver_api::Session)),
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
        assert!(
            error.to_string().contains("not allowed for this tool"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn mysql_ddl_commit_is_not_claimed_reversed() {
        let service = service_with("db.*");
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant_on(
                &service.profile,
                GrantCapability::Ddl,
                "schema_apply_ddl",
                "db.items",
            ))
            .unwrap();
        let session = FakeSession::default();
        let mysql = McpConnection {
            dialect: dexo_sql::Dialect::Mysql,
            default_schema: None,
            ..connection("local")
        };
        let result = call_write_tool(
            &service,
            &ledger,
            Some((&mysql, &session as &dyn dexo_driver_api::Session)),
            "s",
            "schema_apply_ddl",
            json!({
                "operation_id":"op-mysql",
                "target":"db.items",
                "sql":"DROP TABLE items",
                "confirm_target":"db.items"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap();
        assert!(
            result.contains("Committed") && !result.contains("RolledBack"),
            "{result}"
        );
        assert!(session.log().contains(&"ddl DROP TABLE items".to_string()));
    }

    #[tokio::test]
    async fn destructive_ddl_needs_the_target_typed_by_the_client() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::Ddl,
                "schema_apply_ddl",
                "db.public.items",
            ))
            .unwrap();
        let error = call(
            &ledger,
            "schema_apply_ddl",
            json!({"operation_id":"op-d","target":"db.public.items","sql":"DROP TABLE items"}),
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("type db.public.items"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn sql_writes_must_stay_inside_the_grant() {
        let mut profile = profile();
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        profile.tool_rules.push(dexo_app::mcp::profile::ToolRule {
            tool: "data_execute_sql".into(),
            allowed: true,
        });
        let service = McpService::new(profile.clone(), Vec::new());
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant_on(
                &profile,
                GrantCapability::DataWrite,
                "data_execute_sql",
                "db.public.items",
            ))
            .unwrap();
        let session = FakeSession::default();
        let error = call_write_tool(
            &service,
            &ledger,
            Some((
                &connection("local"),
                &session as &dyn dexo_driver_api::Session,
            )),
            "s",
            "data_execute_sql",
            json!({
                "operation_id":"op-sub",
                "target":"db.public.items",
                "sql":"update items set x = 1 where id in (select id from orders)"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
        assert!(session.log().is_empty());
    }

    #[tokio::test]
    async fn production_connections_refuse_writes_and_keep_the_grant() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(
                GrantCapability::DataWrite,
                "data_insert",
                "db.public.items",
            ))
            .unwrap();
        let production = McpConnection {
            environment: dexo_app::Environment::Production,
            ..connection("local")
        };
        let error = call_write_tool(
            &service(),
            &ledger,
            Some((
                &production,
                &FakeSession::default() as &dyn dexo_driver_api::Session,
            )),
            "s",
            "data_insert",
            json!({"operation_id":"op-p","target":"db.public.items","values":{"id":1}})
                .as_object()
                .cloned()
                .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("production"));
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }
}
