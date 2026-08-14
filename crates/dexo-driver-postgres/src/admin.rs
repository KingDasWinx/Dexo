use dexo_driver_api::{
    AdminAction, AdminConfirmKind, AdminList, AdminOutcome, AdminPreview, AdministrationProvider,
    BlockingEdge, DriverError, DriverErrorCategory, LockInfo, LockLevel, Page, SessionInfo,
    SizeInfo, StatInfo, VariableInfo, VariableScope,
};

use crate::ddl::PgDialect;
use crate::error::{is_permission, map_error};
use crate::session::PostgresSession;

fn captured_at() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn parse_pid(id: &str) -> Result<i32, DriverError> {
    id.parse().map_err(|_| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "session id must be an integer",
        )
    })
}

fn restricted_list<T>(error: tokio_postgres::Error) -> Result<AdminList<T>, DriverError> {
    if is_permission(&error) {
        Ok(AdminList {
            items: Vec::new(),
            restriction: Some("permission denied for administration catalogs".into()),
            captured_at: captured_at(),
        })
    } else {
        Err(map_error(error))
    }
}

impl PostgresSession {
    async fn session_restriction(&self) -> Result<Option<String>, DriverError> {
        let row = self
            .client
            .query_one(
                "SELECT pg_has_role(current_user, 'pg_read_all_stats', 'USAGE')",
                &[],
            )
            .await;
        match row {
            Ok(row) => {
                let allowed: bool = row.get(0);
                Ok((!allowed).then_some(
                    "role cannot inspect other sessions (lacks pg_read_all_stats)".into(),
                ))
            }
            Err(error) if is_permission(&error) => {
                Ok(Some("permission denied for session inspection".into()))
            }
            Err(error) => Err(map_error(error)),
        }
    }
}

#[async_trait::async_trait]
impl AdministrationProvider for PostgresSession {
    async fn list_sessions(&self) -> Result<AdminList<SessionInfo>, DriverError> {
        let restriction = self.session_restriction().await?;
        let rows = match self
            .client
            .query(
                "SELECT pid::text, usename::text, datname::text, COALESCE(state, 'unknown'),
                        (EXTRACT(EPOCH FROM (now() - COALESCE(query_start, backend_start))) * 1000)::bigint,
                        NULLIF(btrim(query), '')
                 FROM pg_stat_activity
                 ORDER BY pid",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| SessionInfo {
                    id: row.get(0),
                    user: row.get(1),
                    database: row.get(2),
                    state: row.get(3),
                    duration_ms: row.get::<_, Option<i64>>(4).map(|ms| ms.max(0) as u64),
                    current_query: row.get(5),
                })
                .collect(),
            restriction,
            captured_at: captured_at(),
        })
    }

    async fn list_locks(&self) -> Result<AdminList<LockInfo>, DriverError> {
        let rows = match self
            .client
            .query(
                "SELECT locktype::text,
                        CASE WHEN relation IS NULL THEN NULL ELSE relation::regclass::text END,
                        mode::text, granted, pid::text
                 FROM pg_locks
                 WHERE pid IS NOT NULL",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| LockInfo {
                    lock_type: row.get(0),
                    relation: row.get(1),
                    mode: row.get(2),
                    granted: row.get(3),
                    session_id: row.get::<_, Option<String>>(4).unwrap_or_default(),
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn blocking_graph(&self) -> Result<AdminList<BlockingEdge>, DriverError> {
        let rows = match self
            .client
            .query(
                "SELECT blocking.pid::text, blocked.pid::text, blocked_locks.locktype::text,
                        CASE WHEN blocked_locks.relation IS NULL THEN NULL ELSE blocked_locks.relation::regclass::text END,
                        blocked_locks.mode::text, blocked_locks.granted, blocked.pid::text
                 FROM pg_catalog.pg_locks blocked_locks
                 JOIN pg_catalog.pg_stat_activity blocked ON blocked.pid = blocked_locks.pid
                 JOIN pg_catalog.pg_locks blocking_locks
                   ON blocking_locks.locktype = blocked_locks.locktype
                  AND blocking_locks.database IS NOT DISTINCT FROM blocked_locks.database
                  AND blocking_locks.relation IS NOT DISTINCT FROM blocked_locks.relation
                  AND blocking_locks.page IS NOT DISTINCT FROM blocked_locks.page
                  AND blocking_locks.tuple IS NOT DISTINCT FROM blocked_locks.tuple
                  AND blocking_locks.virtualxid IS NOT DISTINCT FROM blocked_locks.virtualxid
                  AND blocking_locks.transactionid IS NOT DISTINCT FROM blocked_locks.transactionid
                  AND blocking_locks.classid IS NOT DISTINCT FROM blocked_locks.classid
                  AND blocking_locks.objid IS NOT DISTINCT FROM blocked_locks.objid
                  AND blocking_locks.objsubid IS NOT DISTINCT FROM blocked_locks.objsubid
                  AND blocking_locks.pid IS DISTINCT FROM blocked_locks.pid
                 JOIN pg_catalog.pg_stat_activity blocking ON blocking.pid = blocking_locks.pid
                 WHERE NOT blocked_locks.granted AND blocking_locks.granted",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| BlockingEdge {
                    blocker: row.get(0),
                    blocked: row.get(1),
                    lock: LockInfo {
                        lock_type: row.get(2),
                        relation: row.get(3),
                        mode: row.get(4),
                        granted: row.get(5),
                        session_id: row.get(6),
                    },
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn sizes(&self, page: Page) -> Result<AdminList<SizeInfo>, DriverError> {
        let limit = page.limit as i64;
        let offset = page.offset as i64;
        let rows = match self
            .client
            .query(
                "SELECT n.nspname || '.' || c.relname,
                        pg_size_pretty(pg_total_relation_size(c.oid)),
                        pg_total_relation_size(c.oid)::bigint
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE c.relkind IN ('r','m','i','p')
                   AND n.nspname NOT IN ('pg_catalog','information_schema','pg_toast')
                 ORDER BY 3 DESC
                 LIMIT $1 OFFSET $2",
                &[&limit, &offset],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| SizeInfo {
                    object: row.get(0),
                    native_size: row.get(1),
                    bytes: row
                        .get::<_, Option<i64>>(2)
                        .and_then(|value| if value < 0 { None } else { Some(value as u64) }),
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn statistics(&self) -> Result<AdminList<StatInfo>, DriverError> {
        let captured = captured_at();
        let rows = match self
            .client
            .query(
                "SELECT relname::text, n_live_tup::text FROM pg_stat_user_tables",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| StatInfo {
                    name: row.get(0),
                    value: row.get(1),
                    captured_at: captured.clone(),
                })
                .collect(),
            restriction: None,
            captured_at: captured,
        })
    }

    async fn variables(&self) -> Result<AdminList<VariableInfo>, DriverError> {
        let rows = match self
            .client
            .query(
                "SELECT name, setting, 'session' FROM pg_settings
                 UNION ALL
                 SELECT name, COALESCE(boot_val, setting), 'server' FROM pg_settings
                 ORDER BY 1, 3",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => return restricted_list(error),
        };
        Ok(AdminList {
            items: rows
                .iter()
                .map(|row| {
                    let source: String = row.get(2);
                    VariableInfo {
                        name: row.get(0),
                        value: row.get(1),
                        scope: if source == "session" {
                            VariableScope::Session
                        } else {
                            VariableScope::Server
                        },
                    }
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    fn preview(&self, action: &AdminAction) -> Result<AdminPreview, DriverError> {
        preview_postgres(action)
    }

    async fn execute_action(&self, action: AdminAction) -> Result<AdminOutcome, DriverError> {
        match &action {
            AdminAction::CancelQuery { session_id } => {
                signal_backend(&self.client, session_id, false).await
            }
            AdminAction::TerminateSession { session_id } => {
                signal_backend(&self.client, session_id, true).await
            }
            AdminAction::Vacuum { .. }
            | AdminAction::Analyze { .. }
            | AdminAction::Reindex { .. } => {
                let preview = self.preview(&action)?;
                self.client
                    .batch_execute(&preview.command)
                    .await
                    .map_err(map_error)?;
                Ok(AdminOutcome {
                    ok: true,
                    idempotent_noop: false,
                    message: "maintenance completed".into(),
                })
            }
            AdminAction::Optimize { .. } => Err(DriverError::unsupported(
                "OPTIMIZE is not a PostgreSQL command",
            )),
        }
    }
}

pub fn preview_postgres(action: &AdminAction) -> Result<AdminPreview, DriverError> {
    match action {
        AdminAction::CancelQuery { session_id } => Ok(AdminPreview {
            command: format!("SELECT pg_cancel_backend({})", parse_pid(session_id)?),
            lock_risk: LockLevel::None,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::TerminateSession { session_id } => Ok(AdminPreview {
            command: format!("SELECT pg_terminate_backend({})", parse_pid(session_id)?),
            lock_risk: LockLevel::None,
            confirmation: AdminConfirmKind::TypeTarget,
        }),
        AdminAction::Vacuum { target } => Ok(AdminPreview {
            command: format!("VACUUM {}", PgDialect::quote_qualified(target)),
            lock_risk: LockLevel::Share,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::Analyze { target } => Ok(AdminPreview {
            command: format!("ANALYZE {}", PgDialect::quote_qualified(target)),
            lock_risk: LockLevel::Share,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::Reindex { target } => Ok(AdminPreview {
            command: format!("REINDEX TABLE {}", PgDialect::quote_qualified(target)),
            lock_risk: LockLevel::AccessExclusive,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::Optimize { .. } => Err(DriverError::unsupported(
            "OPTIMIZE is not a PostgreSQL command",
        )),
    }
}

async fn signal_backend(
    client: &tokio_postgres::Client,
    session_id: &str,
    terminate: bool,
) -> Result<AdminOutcome, DriverError> {
    let pid = parse_pid(session_id)?;
    let sql = if terminate {
        "SELECT pg_terminate_backend($1)"
    } else {
        "SELECT pg_cancel_backend($1)"
    };
    let sent: bool = client
        .query_one(sql, &[&pid])
        .await
        .map_err(map_error)?
        .get(0);
    Ok(AdminOutcome {
        ok: true,
        idempotent_noop: !sent,
        message: if sent {
            "signal sent".into()
        } else {
            "target already finished".into()
        },
    })
}
