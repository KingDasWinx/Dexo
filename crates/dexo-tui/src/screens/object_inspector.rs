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
    /// What each of them is, as `table orders`, where the catalog could say.
    pub names: std::collections::HashMap<ObjectId, String>,
    pub effective_privileges: Vec<String>,
    pub restrictions: Vec<String>,
    pub error: Option<String>,
    /// The note a person wrote on the object, for themselves and for agents.
    pub note: Option<String>,
    /// `n`: the note being written, and the focus of its Save/Cancel footer.
    pub editing_note: Option<(
        crate::widgets::text_input::TextInput,
        crate::widgets::form::FooterFocus,
    )>,
    /// The palette asked for the note editor before the object was read: it opens once
    /// the object and its note are.
    pub note_requested: bool,
}

impl ObjectInspector {
    /// The note as shown: the person's, else the database's comment, marked as such.
    pub fn shown_note(&self) -> Option<String> {
        self.note.clone().or_else(|| {
            self.object
                .as_ref()
                .and_then(|object| object.attributes.get("comment"))
                .and_then(serde_json::Value::as_str)
                .map(|comment| format!("{comment} (database comment)"))
        })
    }

    /// The key a note is kept under: the object's qualified name as the catalog spells it.
    pub fn note_key(&self) -> Option<String> {
        self.object
            .as_ref()
            .map(|object| object.qualified_name.display_unquoted())
    }
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
