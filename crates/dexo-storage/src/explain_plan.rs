use dexo_driver_api::ExplainPlan;
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, PartialEq)]
pub struct SavedExplainPlan {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub driver: String,
    pub server_version: String,
    pub sql_fingerprint: String,
    pub analyzed: bool,
    pub plan: ExplainPlan,
    pub captured_at: String,
}

pub struct ExplainPlanRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ExplainPlanRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, plan: &SavedExplainPlan) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO explain_plans(
                id, project_id, connection_id, name, driver, server_version,
                sql_fingerprint, analyzed, plan_json, captured_at
             ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                plan.id,
                plan.project_id,
                plan.name,
                plan.driver,
                plan.server_version,
                plan.sql_fingerprint,
                plan.analyzed as i64,
                serde_json::to_string(&plan.plan)?,
                plan.captured_at,
            ],
        )?;
        Ok(())
    }

    pub fn load(&self, project_id: &str, id: &str) -> anyhow::Result<Option<SavedExplainPlan>> {
        self.conn
            .query_row(
                "SELECT id, project_id, name, driver, server_version, sql_fingerprint, analyzed, plan_json, captured_at
                 FROM explain_plans WHERE project_id = ?1 AND id = ?2",
                params![project_id, id],
                |row| {
                    let json: String = row.get(7)?;
                    let plan = serde_json::from_str(&json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    let analyzed: i64 = row.get(6)?;
                    Ok(SavedExplainPlan {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        name: row.get(2)?,
                        driver: row.get(3)?,
                        server_version: row.get(4)?,
                        sql_fingerprint: row.get(5)?,
                        analyzed: analyzed != 0,
                        plan,
                        captured_at: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExplainPlanRepository, SavedExplainPlan};
    use dexo_driver_api::{ExplainPlan, PlanMetrics, PlanNode};
    use rusqlite::Connection;

    #[test]
    fn crud_is_isolated_by_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects(id, name, created_at) VALUES ('p1', 'a', datetime('now')), ('p2', 'b', datetime('now'))",
            [],
        )
        .unwrap();
        let repo = ExplainPlanRepository::new(&conn);
        let saved = SavedExplainPlan {
            id: "plan-1".into(),
            project_id: "p1".into(),
            name: "orders".into(),
            driver: "postgres".into(),
            server_version: "16".into(),
            sql_fingerprint: "abc".into(),
            analyzed: false,
            plan: ExplainPlan {
                planning_ms: None,
                execution_ms: None,
                raw: "{}".into(),
                root: PlanNode {
                    kind: "Result".into(),
                    relation: None,
                    estimates: PlanMetrics::default(),
                    actual: PlanMetrics::default(),
                    loops: None,
                    children: Vec::new(),
                    native: serde_json::json!({}),
                },
            },
            captured_at: "2026-08-15T00:00:00Z".into(),
        };
        repo.save(&saved).unwrap();
        assert!(repo.load("p1", "plan-1").unwrap().is_some());
        assert!(repo.load("p2", "plan-1").unwrap().is_none());
    }
}
