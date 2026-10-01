use dexo_app::explain_service::{
    NodeDelta, compare_plans, render_summary, render_table, render_tree,
};
use dexo_driver_api::ExplainPlan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplainView {
    Tree,
    Table,
    Summary,
}

impl ExplainView {
    pub fn next(self) -> Self {
        match self {
            Self::Tree => Self::Table,
            Self::Table => Self::Summary,
            Self::Summary => Self::Tree,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplainScreen {
    pub plan: Option<ExplainPlan>,
    pub compare: Vec<NodeDelta>,
    pub view: ExplainView,
    pub captured_at: String,
    pub paused: bool,
    pub analyze: bool,
    pub raw: String,
    /// The statement `plan` is for. Each new plan used to be compared with whatever came
    /// before it, often a plan of some other query.
    pub sql: String,
}

impl Default for ExplainScreen {
    fn default() -> Self {
        Self {
            plan: None,
            compare: Vec::new(),
            view: ExplainView::Tree,
            captured_at: String::new(),
            paused: false,
            analyze: false,
            raw: String::new(),
            sql: String::new(),
        }
    }
}

impl ExplainScreen {
    pub fn fixture() -> Self {
        let plan = ExplainPlan {
            planning_ms: Some(0.04),
            execution_ms: Some(0.21),
            raw: r#"[{"Plan":{"Node Type":"Seq Scan","Relation Name":"items"}}]"#.into(),
            root: dexo_driver_api::PlanNode {
                kind: "Seq Scan".into(),
                relation: Some("items".into()),
                estimates: dexo_driver_api::PlanMetrics {
                    cost: Some(22.5),
                    rows: Some(10.0),
                    width: Some(36.0),
                    time_ms: None,
                },
                actual: dexo_driver_api::PlanMetrics {
                    cost: None,
                    rows: Some(1000.0),
                    width: None,
                    time_ms: Some(0.18),
                },
                loops: Some(1),
                children: Vec::new(),
                native: Default::default(),
            },
        };
        Self {
            raw: plan.raw.clone(),
            captured_at: "1710000000".into(),
            analyze: true,
            plan: Some(plan),
            ..Self::default()
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "explain captured_at={} paused={} analyze={}",
            self.captured_at, self.paused, self.analyze
        )];
        if let Some(plan) = &self.plan {
            let body = match self.view {
                ExplainView::Tree => render_tree(plan),
                ExplainView::Table => render_table(plan),
                ExplainView::Summary => render_summary(plan),
            };
            lines.extend(body.lines().map(str::to_string));
            if !self.compare.is_empty() {
                lines.push("compare:".into());
                for delta in &self.compare {
                    lines.push(format!("{delta:?}"));
                }
            }
            lines.push(format!("raw_bytes={}", self.raw.len()));
        } else {
            lines.push("no plan".into());
        }
        lines
    }

    /// A second plan of the same statement is compared with the first -- estimated
    /// against analyzed, or before and after an index -- and any other plan starts clean.
    pub fn set_plan(&mut self, plan: ExplainPlan, sql: String) {
        self.compare = match &self.plan {
            Some(previous) if self.sql == sql => compare_plans(previous, &plan),
            _ => Vec::new(),
        };
        self.raw = plan.raw.clone();
        self.plan = Some(plan);
        self.sql = sql;
    }

    pub fn clear(&mut self) {
        self.plan = None;
        self.compare.clear();
        self.raw.clear();
        self.sql.clear();
    }
}
