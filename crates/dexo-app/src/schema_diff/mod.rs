pub mod diff;
pub mod graph;
pub mod normalize;
pub mod risk;
pub mod script;
pub mod snapshot;

use dexo_driver_api::{DdlPlan, SchemaChange};

pub use diff::{RenameMapping, SchemaDifference, diff};
pub use graph::{OrderedChange, infer_edges, order_changes};
pub use normalize::normalize;
pub use risk::classify_difference;
pub use script::{MigrationScript, generate_script, render_unquoted};
pub use snapshot::{DiffSource, SchemaSnapshot, SnapshotEnvelope, SnapshotError};

pub fn plan_migration(
    from: &SchemaSnapshot,
    to: &SchemaSnapshot,
    renames: &[RenameMapping],
    render: impl Fn(&SchemaChange) -> Result<DdlPlan, String>,
) -> (Vec<SchemaDifference>, Vec<OrderedChange>, MigrationScript) {
    let from_objects = normalize(from.objects.clone(), &from.driver);
    let to_objects = normalize(to.objects.clone(), &to.driver);
    let changes = diff(&from_objects, &to_objects, renames, None);
    let edges = infer_edges(&changes);
    let ordered = order_changes(changes.clone(), &edges);
    let script = generate_script(&ordered, render);
    (changes, ordered, script)
}
