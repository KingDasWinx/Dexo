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
