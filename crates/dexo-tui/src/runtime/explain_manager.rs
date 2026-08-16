use dexo_sql::statement_at;
use std::sync::Mutex;

use dexo_driver_api::{ExplainRequest, Session};

pub struct ExplainManager {
    last_sql: Mutex<String>,
    analyze_confirmed: Mutex<bool>,
}

impl Default for ExplainManager {
    fn default() -> Self {
        Self {
            last_sql: Mutex::new(String::new()),
            analyze_confirmed: Mutex::new(false),
        }
    }
}

impl ExplainManager {
    pub async fn explain(
        &self,
        document: &str,
        cursor: usize,
        analyze: bool,
    ) -> Result<(), String> {
        if analyze && !*self.analyze_confirmed.lock().expect("confirm") {
            return Err("explain analyze requires confirmation".into());
        }
        let span = statement_at(document, cursor).ok_or_else(|| "no statement".to_string())?;
        let sql = document[span.byte_range.clone()]
            .trim()
            .trim_end_matches(';');
        *self.last_sql.lock().expect("sql") = sql.to_string();
        Ok(())
    }

    pub fn confirm_analyze(&self) {
        *self.analyze_confirmed.lock().expect("confirm") = true;
    }

    pub fn explain_sql(&self) -> String {
        self.last_sql.lock().expect("sql").clone()
    }
}

pub async fn run_live(
    session: std::sync::Arc<dyn Session>,
    document: &str,
    cursor: usize,
    analyze: bool,
    tx: tokio::sync::mpsc::Sender<crate::action::Action>,
) {
    let manager = ExplainManager::default();
    if analyze {
        manager.confirm_analyze();
    }
    match manager.explain(document, cursor, analyze).await {
        Ok(()) => {
            let sql = manager.explain_sql();
            let Some(provider) = session.explain() else {
                let _ = tx
                    .send(crate::action::Action::OperationFailed {
                        key: crate::runtime::OperationKey::new(
                            crate::runtime::OperationId::new(),
                            "",
                            "",
                            0,
                        ),
                        message: "explain unavailable".into(),
                    })
                    .await;
                return;
            };
            let request = if analyze {
                ExplainRequest::analyzed(sql)
            } else {
                ExplainRequest::estimated(sql)
            };
            match provider.explain(request).await {
                Ok(plan) => {
                    let _ = tx
                        .send(crate::action::Action::ExplainLoaded {
                            plan: Box::new(plan),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = tx
                        .send(crate::action::Action::OperationFailed {
                            key: crate::runtime::OperationKey::new(
                                crate::runtime::OperationId::new(),
                                "",
                                "",
                                0,
                            ),
                            message: error.to_string(),
                        })
                        .await;
                }
            }
        }
        Err(message) => {
            let _ = tx
                .send(crate::action::Action::OperationFailed {
                    key: crate::runtime::OperationKey::new(
                        crate::runtime::OperationId::new(),
                        "",
                        "",
                        0,
                    ),
                    message,
                })
                .await;
        }
    }
}
