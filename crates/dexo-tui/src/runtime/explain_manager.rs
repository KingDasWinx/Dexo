use dexo_sql::statement_at;
use std::sync::Mutex;

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
