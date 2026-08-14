use dexo_driver_api::{
    AdminAction, AdminConfirmKind, AdminList, AdminOutcome, AdminPreview, AdministrationProvider,
    BlockingEdge, DriverError, DriverErrorCategory, LockInfo, LockLevel, Page, SessionInfo,
    SizeInfo, StatInfo, VariableInfo, VariableScope,
};
use mysql_async::prelude::Queryable;

use crate::ddl::MysqlDialect;
use crate::error::{is_permission, map_error};
use crate::session::MysqlSession;

fn captured_at() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn parse_id(id: &str) -> Result<u32, DriverError> {
    id.parse().map_err(|_| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "session id must be an integer",
        )
    })
}

fn is_unknown_thread(error: &mysql_async::Error) -> bool {
    matches!(error, mysql_async::Error::Server(err) if err.code == 1094)
        || error
            .to_string()
            .to_ascii_lowercase()
            .contains("unknown thread")
}

impl MysqlSession {
    async fn process_restriction(&self) -> Result<Option<String>, DriverError> {
        let mut conn = self.conn.lock().await;
        let count: Option<(u64,)> = conn
            .query_first(
                "SELECT COUNT(*) FROM information_schema.USER_PRIVILEGES WHERE PRIVILEGE_TYPE = 'PROCESS'",
            )
            .await
            .map_err(map_error)?;
        Ok((count.map(|row| row.0).unwrap_or(0) == 0).then_some(
            "PROCESS privilege is missing; only the current session may be visible".into(),
        ))
    }
}

#[async_trait::async_trait]
impl AdministrationProvider for MysqlSession {
    async fn list_sessions(&self) -> Result<AdminList<SessionInfo>, DriverError> {
        let restriction = self.process_restriction().await?;
        let mut conn = self.conn.lock().await;
        let rows: Vec<(
            u32,
            Option<String>,
            Option<String>,
            String,
            i64,
            Option<String>,
        )> = match conn
            .query("SELECT ID, USER, DB, COMMAND, TIME, INFO FROM information_schema.PROCESSLIST")
            .await
        {
            Ok(rows) => rows,
            Err(error) if is_permission(&error) => {
                return Ok(AdminList {
                    items: Vec::new(),
                    restriction: Some("permission denied for process list".into()),
                    captured_at: captured_at(),
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        Ok(AdminList {
            items: rows
                .into_iter()
                .map(|(id, user, database, state, time_s, query)| SessionInfo {
                    id: id.to_string(),
                    user,
                    database,
                    state,
                    duration_ms: Some((time_s.max(0) as u64).saturating_mul(1000)),
                    current_query: query,
                })
                .collect(),
            restriction,
            captured_at: captured_at(),
        })
    }

    async fn list_locks(&self) -> Result<AdminList<LockInfo>, DriverError> {
        let mut conn = self.conn.lock().await;
        let rows: Vec<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<u64>,
        )> = match conn
            .query(
                "SELECT LOCK_TYPE, OBJECT_SCHEMA, OBJECT_NAME, LOCK_MODE, LOCK_STATUS, THREAD_ID
                 FROM performance_schema.data_locks",
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) if is_permission(&error) => {
                return Ok(AdminList {
                    items: Vec::new(),
                    restriction: Some("permission denied for performance_schema.data_locks".into()),
                    captured_at: captured_at(),
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        Ok(AdminList {
            items: rows
                .into_iter()
                .map(|(lock_type, schema, name, mode, status, thread)| LockInfo {
                    lock_type: lock_type.unwrap_or_else(|| "unknown".into()),
                    relation: match (schema, name) {
                        (Some(schema), Some(name)) => Some(format!("{schema}.{name}")),
                        (None, Some(name)) | (Some(name), None) => Some(name),
                        _ => None,
                    },
                    mode: mode.unwrap_or_else(|| "unknown".into()),
                    granted: status
                        .as_deref()
                        .is_none_or(|status| status.eq_ignore_ascii_case("GRANTED")),
                    session_id: thread.map(|id| id.to_string()).unwrap_or_default(),
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn blocking_graph(&self) -> Result<AdminList<BlockingEdge>, DriverError> {
        let mut conn = self.conn.lock().await;
        let rows: Vec<(
            Option<u64>,
            Option<u64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = match conn
            .query(
                "SELECT waiting.OWNER_THREAD_ID, blocking.OWNER_THREAD_ID, waiting.LOCK_TYPE,
                        CONCAT(waiting.OBJECT_SCHEMA, '.', waiting.OBJECT_NAME), waiting.LOCK_MODE,
                        waiting.LOCK_STATUS
                 FROM performance_schema.data_lock_waits w
                 JOIN performance_schema.data_locks waiting
                   ON waiting.ENGINE_LOCK_ID = w.REQUESTING_ENGINE_LOCK_ID
                 JOIN performance_schema.data_locks blocking
                   ON blocking.ENGINE_LOCK_ID = w.BLOCKING_ENGINE_LOCK_ID",
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) if is_permission(&error) => {
                return Ok(AdminList {
                    items: Vec::new(),
                    restriction: Some("permission denied for data_lock_waits".into()),
                    captured_at: captured_at(),
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        Ok(AdminList {
            items: rows
                .into_iter()
                .map(
                    |(blocked, blocker, lock_type, relation, mode, status)| BlockingEdge {
                        blocker: blocker.map(|id| id.to_string()).unwrap_or_default(),
                        blocked: blocked.map(|id| id.to_string()).unwrap_or_default(),
                        lock: LockInfo {
                            lock_type: lock_type.unwrap_or_else(|| "unknown".into()),
                            relation,
                            mode: mode.unwrap_or_else(|| "unknown".into()),
                            granted: status
                                .as_deref()
                                .is_none_or(|status| status.eq_ignore_ascii_case("GRANTED")),
                            session_id: blocked.map(|id| id.to_string()).unwrap_or_default(),
                        },
                    },
                )
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn sizes(&self, page: Page) -> Result<AdminList<SizeInfo>, DriverError> {
        let sql = format!(
            "SELECT CONCAT(table_schema, '.', table_name),
                    CASE WHEN data_length IS NULL OR index_length IS NULL THEN NULL
                         ELSE CONCAT(ROUND((data_length + index_length) / 1024 / 1024, 2), ' MiB') END,
                    (data_length + index_length)
             FROM information_schema.tables
             WHERE table_schema = DATABASE()
             ORDER BY (data_length + index_length) DESC
             LIMIT {} OFFSET {}",
            page.limit, page.offset
        );
        let mut conn = self.conn.lock().await;
        let rows: Vec<(String, Option<String>, Option<u64>)> = match conn.query(sql).await {
            Ok(rows) => rows,
            Err(error) if is_permission(&error) => {
                return Ok(AdminList {
                    items: Vec::new(),
                    restriction: Some("permission denied for table sizes".into()),
                    captured_at: captured_at(),
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        Ok(AdminList {
            items: rows
                .into_iter()
                .map(|(object, native_size, bytes)| SizeInfo {
                    object,
                    native_size,
                    bytes,
                })
                .collect(),
            restriction: None,
            captured_at: captured_at(),
        })
    }

    async fn statistics(&self) -> Result<AdminList<StatInfo>, DriverError> {
        let captured = captured_at();
        let mut conn = self.conn.lock().await;
        let rows: Vec<(String, Option<u64>, Option<String>)> = match conn
            .query(
                "SELECT table_name, table_rows, DATE_FORMAT(update_time, '%Y-%m-%dT%H:%i:%s')
                 FROM information_schema.tables
                 WHERE table_schema = DATABASE()",
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) if is_permission(&error) => {
                return Ok(AdminList {
                    items: Vec::new(),
                    restriction: Some("permission denied for table statistics".into()),
                    captured_at: captured,
                });
            }
            Err(error) => return Err(map_error(error)),
        };
        Ok(AdminList {
            items: rows
                .into_iter()
                .map(|(name, rows, updated)| StatInfo {
                    name,
                    value: rows.map(|count| count.to_string()),
                    captured_at: updated.unwrap_or_else(|| captured.clone()),
                })
                .collect(),
            restriction: None,
            captured_at: captured,
        })
    }

    async fn variables(&self) -> Result<AdminList<VariableInfo>, DriverError> {
        let mut conn = self.conn.lock().await;
        let session: Vec<(String, String)> = conn
            .query("SHOW SESSION VARIABLES")
            .await
            .map_err(map_error)?;
        let server: Vec<(String, String)> = conn
            .query("SHOW GLOBAL VARIABLES")
            .await
            .map_err(map_error)?;
        let mut items: Vec<VariableInfo> = session
            .into_iter()
            .map(|(name, value)| VariableInfo {
                name,
                value: Some(value),
                scope: VariableScope::Session,
            })
            .collect();
        items.extend(server.into_iter().map(|(name, value)| VariableInfo {
            name,
            value: Some(value),
            scope: VariableScope::Server,
        }));
        Ok(AdminList {
            items,
            restriction: None,
            captured_at: captured_at(),
        })
    }

    fn preview(&self, action: &AdminAction) -> Result<AdminPreview, DriverError> {
        preview_mysql(action)
    }

    async fn execute_action(&self, action: AdminAction) -> Result<AdminOutcome, DriverError> {
        let preview = self.preview(&action)?;
        let mut conn = self.conn.lock().await;
        match conn.query_drop(&preview.command).await {
            Ok(()) => Ok(AdminOutcome {
                ok: true,
                idempotent_noop: false,
                message: "action completed".into(),
            }),
            Err(error) if is_unknown_thread(&error) => Ok(AdminOutcome {
                ok: true,
                idempotent_noop: true,
                message: "target already finished".into(),
            }),
            Err(error) => Err(map_error(error)),
        }
    }
}

pub fn preview_mysql(action: &AdminAction) -> Result<AdminPreview, DriverError> {
    match action {
        AdminAction::CancelQuery { session_id } => Ok(AdminPreview {
            command: format!("KILL QUERY {}", parse_id(session_id)?),
            lock_risk: LockLevel::None,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::TerminateSession { session_id } => Ok(AdminPreview {
            command: format!("KILL CONNECTION {}", parse_id(session_id)?),
            lock_risk: LockLevel::None,
            confirmation: AdminConfirmKind::TypeTarget,
        }),
        AdminAction::Analyze { target } => Ok(AdminPreview {
            command: format!("ANALYZE TABLE {}", MysqlDialect::quote_qualified(target)),
            lock_risk: LockLevel::Share,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::Optimize { target } => Ok(AdminPreview {
            command: format!("OPTIMIZE TABLE {}", MysqlDialect::quote_qualified(target)),
            lock_risk: LockLevel::Exclusive,
            confirmation: AdminConfirmKind::Once,
        }),
        AdminAction::Vacuum { .. } => {
            Err(DriverError::unsupported("VACUUM is not a MySQL command"))
        }
        AdminAction::Reindex { .. } => {
            Err(DriverError::unsupported("REINDEX is not a MySQL command"))
        }
    }
}
