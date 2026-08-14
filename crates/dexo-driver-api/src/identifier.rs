#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualifiedName {
    catalog: Option<String>,
    schema: Option<String>,
    object: String,
}

impl QualifiedName {
    pub fn new(
        catalog: Option<impl Into<String>>,
        schema: Option<impl Into<String>>,
        object: impl Into<String>,
    ) -> Self {
        let object = object.into();
        assert!(!object.is_empty(), "object name must be non-empty");
        Self {
            catalog: catalog.map(Into::into).filter(|s| !s.is_empty()),
            schema: schema.map(Into::into).filter(|s| !s.is_empty()),
            object,
        }
    }

    pub fn catalog(&self) -> Option<&str> {
        self.catalog.as_deref()
    }

    pub fn schema(&self) -> Option<&str> {
        self.schema.as_deref()
    }

    pub fn object(&self) -> &str {
        &self.object
    }

    pub fn display_unquoted(&self) -> String {
        let mut parts = Vec::with_capacity(3);
        if let Some(catalog) = &self.catalog {
            parts.push(catalog.as_str());
        }
        if let Some(schema) = &self.schema {
            parts.push(schema.as_str());
        }
        parts.push(&self.object);
        parts.join(".")
    }
}
