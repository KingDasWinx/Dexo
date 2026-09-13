use dexo_driver_api::{CatalogObject, ObjectId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectInspector {
    pub open: bool,
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
    pub fn open_loading(qualified: impl Into<String>) -> Self {
        Self {
            open: true,
            qualified_name: qualified.into(),
            ..Self::default()
        }
    }
}
