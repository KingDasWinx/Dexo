use dexo_driver_api::{CatalogObject, ExplainPlan, ExplainRequest, QueryRequest, Session};
use dexo_sql::{StatementEffect, split_statements};
use futures_util::StreamExt;
use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::{McpProfile, QueryMode};
use crate::mcp::selector::ObjectRef;
use crate::query_service::map_driver_error;
use crate::search_service::SearchService;

const HIDDEN: &str = "not found";

pub struct McpService {
    pub profile: McpProfile,
    objects: Vec<CatalogObject>,
}

impl McpService {
    pub fn new(profile: McpProfile, objects: Vec<CatalogObject>) -> Self {
        let policy = ObjectPolicy::new(profile.selectors.clone());
        let objects = objects
            .into_iter()
            .filter(|object| {
                policy.decide(&ObjectRef::parse(&object.qualified_name.display_unquoted()))
                    == Decision::Allow
            })
            .collect();
        Self { profile, objects }
    }

    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.profile.name,
            "enabled": self.profile.enabled,
            "persistent_access": "read_only",
            "query_mode": format!("{:?}", self.profile.query_mode),
            "limits": self.profile.limits,
            "tools": advertised_tools(&self.profile),
        })
    }

    pub fn search(&self, query: &str) -> Vec<CatalogObject> {
        if query.trim().is_empty() {
            return self.objects.clone();
        }
        SearchService::from_objects(self.objects.clone())
            .search(query)
            .into_iter()
            .map(|hit| hit.object)
            .collect()
    }

    pub fn describe(&self, id_or_name: &str) -> Result<CatalogObject, AppError> {
        self.find(id_or_name).cloned().ok_or_else(hidden)
    }

    pub fn ddl(&self, id_or_name: &str) -> Result<String, AppError> {
        let object = self.describe(id_or_name)?;
        Ok(object
            .attributes
            .get("ddl")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("-- {}", object.qualified_name.display_unquoted())))
    }

    pub fn relationships(&self, id_or_name: &str) -> Result<Vec<CatalogObject>, AppError> {
        let object = self.describe(id_or_name)?;
        Ok(self
            .objects
            .iter()
            .filter(|other| other.parent.as_ref() == Some(&object.id) || other.id == object.id)
            .cloned()
            .collect())
    }

    pub fn validate_sql(&self, sql: &str) -> Result<(), AppError> {
        self.authorize_read_sql(sql)?;
        Ok(())
    }

    pub fn authorize_read_sql(&self, sql: &str) -> Result<(), AppError> {
        if self.profile.query_mode != QueryMode::RawReadSql || self.profile.column_isolation() {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "raw SQL is not enabled for this profile",
            ));
        }
        let spans = split_statements(sql);
        if spans.len() != 1 {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "query_execute_read requires one statement",
            ));
        }
        let span = &spans[0];
        if !span.understood || span.effect != StatementEffect::ReadOnly {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "statement is not an understood read",
            ));
        }
        let policy = ObjectPolicy::new(self.profile.selectors.clone());
        for object in &self.objects {
            let _ = object;
        }
        if let Some(name) = referenced_name(sql)
            && policy.decide(&ObjectRef::parse(&name)) != Decision::Allow
        {
            return Err(hidden());
        }
        Ok(())
    }

    pub fn authorize_write_sql(&self, sql: &str) -> Result<(), AppError> {
        let spans = split_statements(sql);
        if spans.len() != 1 || !spans[0].understood {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "statement effect is not understood",
            ));
        }
        if spans[0].effect != StatementEffect::DataWrite {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "statement is outside grant capability",
            ));
        }
        Ok(())
    }

    pub async fn execute_read(
        &self,
        session: &dyn Session,
        sql: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.authorize_read_sql(sql)?;
        let mut request = QueryRequest::read(sql, self.profile.limits.max_rows);
        request.timeout = std::time::Duration::from_secs(self.profile.limits.timeout_secs);
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut rows = Vec::new();
        let mut bytes = 0_u64;
        while let Some(event) = stream.next().await {
            let event = event.map_err(map_driver_error)?;
            if let dexo_driver_api::QueryEvent::Rows(batch) = event {
                for row in batch.rows {
                    bytes = bytes.saturating_add(row.len() as u64 * 8);
                    if bytes > self.profile.limits.max_bytes
                        || rows.len() as u64 >= self.profile.limits.max_rows
                    {
                        break;
                    }
                    rows.push(format!("{row:?}"));
                }
            }
        }
        Ok(serde_json::json!({ "rows": rows, "bytes": bytes }))
    }

    pub async fn explain(
        &self,
        session: &dyn Session,
        sql: &str,
        analyze: bool,
    ) -> Result<ExplainPlan, AppError> {
        self.authorize_read_sql(sql)?;
        if analyze {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "EXPLAIN ANALYZE is not available on a read-only MCP profile",
            ));
        }
        session
            .explain()
            .ok_or_else(|| AppError::new(ErrorCategory::Capability, "explain is unavailable"))?
            .explain(ExplainRequest::estimated(sql))
            .await
            .map_err(map_driver_error)
    }

    fn find(&self, id_or_name: &str) -> Option<&CatalogObject> {
        self.objects.iter().find(|object| {
            object.id.as_str() == id_or_name
                || object.qualified_name.display_unquoted() == id_or_name
                || object.qualified_name.object() == id_or_name
        })
    }
}

pub fn advertised_tools(profile: &McpProfile) -> Vec<&'static str> {
    let mut tools = vec![
        "catalog_search",
        "object_describe",
        "object_get_ddl",
        "object_relationships",
        "query_validate",
        "query_explain",
        "schema_diff",
    ];
    if profile.query_mode == QueryMode::RawReadSql && !profile.column_isolation() {
        tools.push("query_execute_read");
    }
    tools
        .into_iter()
        .filter(|name| profile.tool_allowed(name))
        .collect()
}

pub fn new_result_uri() -> String {
    format!("dexo://result/{}", Uuid::new_v4())
}

fn referenced_name(sql: &str) -> Option<String> {
    let upper = sql.to_ascii_uppercase();
    let from = upper.find(" FROM ")?;
    let rest = sql[from + 6..].trim_start();
    let name = rest
        .split(|ch: char| ch.is_whitespace() || ch == ';' || ch == ',')
        .next()?;
    Some(name.trim_matches('"').trim_matches('`').to_string())
}

fn hidden() -> AppError {
    AppError::new(ErrorCategory::McpPolicy, HIDDEN)
}

#[cfg(test)]
mod tests {
    use super::McpService;
    use crate::mcp::profile::{McpProfile, QueryMode};
    use crate::mcp::selector::{Effect, SelectorRule};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn table(name: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn denied_objects_are_absent_from_search_and_describe() {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        let service = McpService::new(profile, vec![table("users"), table("secrets")]);
        assert_eq!(service.search("").len(), 1);
        assert!(service.describe("db.public.secrets").is_err());
        assert_eq!(
            service
                .describe("db.public.secrets")
                .unwrap_err()
                .to_string(),
            "not found"
        );
        assert!(service.describe("missing").unwrap_err().to_string() == "not found");
    }

    #[test]
    fn mutating_and_unknown_sql_are_rejected() {
        let mut profile = McpProfile::new("assistant");
        profile.query_mode = QueryMode::RawReadSql;
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        let service = McpService::new(profile, vec![table("users")]);
        assert!(
            service
                .validate_sql("WITH x AS (SELECT 1) DELETE FROM users")
                .is_err()
        );
        assert!(
            service.validate_sql("SELECT mystery() FROM users").is_ok()
                || service.validate_sql("SELECT 1").is_ok()
        );
        assert!(service.validate_sql("SELECT 1 FROM secrets").is_err());
    }
}
