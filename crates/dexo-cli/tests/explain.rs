use clap::Parser;
use dexo_app::explain_service::{compare_plans, render_summary, render_table, render_tree};
use dexo_cli::args::{Args, SessionsCommand};
use dexo_driver_api::{ExplainPlan, PlanMetrics, PlanNode};

fn sample_plan() -> ExplainPlan {
    ExplainPlan {
        planning_ms: Some(0.04),
        execution_ms: Some(0.21),
        raw: r#"[{"Plan":{"Node Type":"Seq Scan","Relation Name":"items"}}]"#.into(),
        root: PlanNode {
            kind: "Seq Scan".into(),
            relation: Some("items".into()),
            estimates: PlanMetrics {
                cost: Some(22.5),
                rows: Some(10.0),
                width: None,
                time_ms: None,
            },
            actual: PlanMetrics {
                cost: None,
                rows: Some(1000.0),
                width: None,
                time_ms: Some(0.18),
            },
            loops: Some(1),
            children: Vec::new(),
            native: Default::default(),
        },
    }
}

#[test]
fn explain_and_sessions_args_parse() {
    let explain = Args::parse_from([
        "dexo",
        "explain",
        "--connection",
        "c",
        "--sql",
        "select 1",
        "--analyze",
        "--confirm",
    ]);
    assert!(matches!(
        explain.command,
        Some(dexo_cli::args::Command::Explain {
            analyze: true,
            confirm: true,
            ..
        })
    ));
    let list = Args::parse_from(["dexo", "sessions", "list", "--connection", "c"]);
    assert!(matches!(
        list.command,
        Some(dexo_cli::args::Command::Sessions {
            command: SessionsCommand::List { .. }
        })
    ));
    let cancel = Args::parse_from([
        "dexo",
        "sessions",
        "cancel",
        "--connection",
        "c",
        "--session",
        "42",
        "--confirm",
    ]);
    assert!(matches!(
        cancel.command,
        Some(dexo_cli::args::Command::Sessions {
            command: SessionsCommand::Cancel { confirm: true, .. }
        })
    ));
    let terminate = Args::parse_from([
        "dexo",
        "sessions",
        "terminate",
        "--connection",
        "c",
        "--session",
        "99",
        "--confirm-target",
        "99",
    ]);
    assert!(matches!(
        terminate.command,
        Some(dexo_cli::args::Command::Sessions {
            command: SessionsCommand::Terminate { .. }
        })
    ));
}

#[test]
fn explain_cli_goldens_tree_table_summary_and_compare() {
    let plan = sample_plan();
    let tree = render_tree(&plan);
    assert!(tree.contains("Seq Scan"));
    assert!(tree.contains("actual rows >> estimate (heuristic)"));
    let table = render_table(&plan);
    assert!(table.contains("Seq Scan\titems"));
    let summary = render_summary(&plan);
    assert!(summary.contains("scans=1"));
    let other = ExplainPlan {
        root: PlanNode {
            kind: "Index Scan".into(),
            relation: Some("items".into()),
            ..plan.root.clone()
        },
        ..plan.clone()
    };
    let deltas = compare_plans(&plan, &other);
    assert!(deltas.iter().any(|delta| matches!(
        delta,
        dexo_app::explain_service::NodeDelta::Changed { field, .. } if field == "kind/relation"
    )));
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains("Seq Scan"));
    assert!(json.contains("raw"));
}
