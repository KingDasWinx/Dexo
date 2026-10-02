//! The object inspector lists what an object depends on by name. It listed the catalog's own
//! ids -- `pg:schema:2200, pg:type:17094` -- which name nothing a person knows.
use dexo_driver_api::ObjectId;
use dexo_tui::{Model, render::render_to_string};

#[test]
fn dependencies_are_named_not_numbered() {
    let mut model = Model::default();
    model.apply_size(120, 36);
    model.inspector.open = true;
    model.inspector.qualified_name = "public.orders".into();
    model.inspector.dependencies = vec![
        ObjectId::new("pg:schema:2200"),
        ObjectId::new("pg:type:17094"),
    ];
    let screen = render_to_string(&model, 120, 36);
    assert!(!screen.contains("pg:schema:2200"), "{screen}");
    assert!(!screen.contains("17094"), "{screen}");
    assert!(screen.contains("deps: a schema, a type"), "{screen}");
}

/// A request that waits for minutes is on the status line, not only in a toast that is
/// gone in seconds.
#[test]
fn a_waiting_agent_write_stays_on_the_status_line() {
    let mut model = Model::default();
    model.apply_size(120, 36);
    model.mcp_audit.announced = vec![uuid::Uuid::from_u128(1)];
    let screen = render_to_string(&model, 120, 36);
    assert!(
        screen.contains("1 agent write waiting: Ctrl+Alt+A"),
        "{screen}"
    );
    model.mcp_audit.open = true;
    assert!(!render_to_string(&model, 120, 36).contains("agent write waiting"));
}
