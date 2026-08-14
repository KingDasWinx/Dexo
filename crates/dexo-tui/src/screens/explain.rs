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

#[derive(Clone, Debug, PartialEq)]
pub struct ExplainScreen {
    pub plan: Option<ExplainPlan>,
    pub compare: Vec<NodeDelta>,
    pub view: ExplainView,
    pub captured_at: String,
    pub paused: bool,
    pub analyze: bool,
    pub analyze_confirmed: bool,
    pub raw: String,
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
            analyze_confirmed: false,
            raw: String::new(),
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
            analyze_confirmed: true,
            plan: Some(plan),
            ..Self::default()
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "explain captured_at={} paused={} analyze={} confirmed={}",
            self.captured_at, self.paused, self.analyze, self.analyze_confirmed
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

    pub fn set_plan(&mut self, plan: ExplainPlan, previous: Option<&ExplainPlan>) {
        if let Some(previous) = previous {
            self.compare = compare_plans(previous, &plan);
        }
        self.raw = plan.raw.clone();
        self.plan = Some(plan);
    }
}
