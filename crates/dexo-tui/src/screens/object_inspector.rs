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
    pub tab: InspectorTab,
    pub scroll: u16,
}

impl InspectorTab {
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Properties),
            1 => Some(Self::Ddl),
            2 => Some(Self::Dependencies),
            3 => Some(Self::Privileges),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Properties => Self::Ddl,
            Self::Ddl => Self::Dependencies,
            Self::Dependencies => Self::Privileges,
            Self::Privileges => Self::Properties,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Properties => "properties",
            Self::Ddl => "ddl",
            Self::Dependencies => "dependencies",
            Self::Privileges => "privileges",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InspectorTab {
    #[default]
    Properties,
    Ddl,
    Dependencies,
    Privileges,
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
