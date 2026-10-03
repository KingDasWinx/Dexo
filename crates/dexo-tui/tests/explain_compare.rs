//! A plan compared with the last one says so, also when nothing differs.
use dexo_driver_api::{ExplainPlan, PlanMetrics, PlanNode};
use dexo_tui::screens::explain::{ExplainScreen, ExplainStyles};
use ratatui::style::Style;

fn plan(kind: &str) -> ExplainPlan {
    ExplainPlan {
        planning_ms: None,
        execution_ms: None,
        root: PlanNode {
            kind: kind.into(),
            relation: Some("orders".into()),
            detail: None,
            estimates: PlanMetrics::default(),
            actual: PlanMetrics::default(),
            loops: None,
            children: Vec::new(),
            native: serde_json::Value::Null,
        },
        raw: String::new(),
    }
}

fn headline(screen: &ExplainScreen) -> String {
    let styles = ExplainStyles {
        muted: Style::default(),
        warning: Style::default(),
        unicode: true,
    };
    screen.lines(120, &styles)[0].to_string()
}

#[test]
fn the_same_plan_twice_says_it_is_the_same() {
    let mut screen = ExplainScreen::default();
    screen.set_plan(plan("Seq Scan"), "select 1".into(), Vec::new());
    assert!(!headline(&screen).contains("same plan"));
    screen.set_plan(plan("Seq Scan"), "select 1".into(), Vec::new());
    assert!(headline(&screen).contains("same plan as the last one"));
    screen.set_plan(plan("Index Scan"), "select 1".into(), Vec::new());
    assert!(headline(&screen).contains("1 change since the last plan"));
}
