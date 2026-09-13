use dexo_driver_api::{CatalogObject, ObjectId};

/// Which facet of the selected object the overlay is showing. DDL is long and
/// Properties is short, so they are separate surfaces rather than one scroll.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InspectorFacet {
    #[default]
    Properties,
    Ddl,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectInspector {
    pub open: bool,
    pub facet: InspectorFacet,
    pub scroll: u16,
    pub qualified_name: String,
    pub object: Option<CatalogObject>,
    pub ddl: Option<String>,
    pub dependencies: Vec<ObjectId>,
    pub dependents: Vec<ObjectId>,
    pub effective_privileges: Vec<String>,
    pub restrictions: Vec<String>,
    pub error: Option<String>,
}

impl ObjectInspector {
    /// Resets the inspector for a new object without showing it. Opening the overlay is
    /// the caller's decision: loading metadata is something opening a table does on its
    /// own, and that must not put a modal over the grid.
    pub fn loading(qualified: impl Into<String>) -> Self {
        Self {
            qualified_name: qualified.into(),
            ..Self::default()
        }
    }
}
