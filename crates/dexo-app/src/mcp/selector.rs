use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Exact(String),
    Star,
}

impl Segment {
    fn parse(part: &str) -> Result<Self, AppError> {
        if part == "*" {
            return Ok(Self::Star);
        }
        if part.is_empty() || part.contains('*') {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "selectors allow exact names or explicit * only",
            ));
        }
        Ok(Self::Exact(part.to_string()))
    }

    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Star => true,
            Self::Exact(name) => name == value,
        }
    }

    fn specificity(&self) -> u8 {
        match self {
            Self::Star => 0,
            Self::Exact(_) => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    pub catalog: Option<Segment>,
    pub schema: Option<Segment>,
    pub object: Option<Segment>,
    pub column: Option<Segment>,
}

impl Selector {
    pub fn parse(pattern: &str) -> Result<Self, AppError> {
        let parts: Vec<&str> = pattern.split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "selector must have 1..=4 segments",
            ));
        }
        let mut segs = parts
            .into_iter()
            .map(Segment::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let column = if segs.len() == 4 { segs.pop() } else { None };
        let object = if segs.len() == 3 { segs.pop() } else { None };
        let schema = if segs.len() == 2 { segs.pop() } else { None };
        let catalog = segs.pop();
        Ok(Self {
            catalog,
            schema,
            object,
            column,
        })
    }

    pub fn matches(&self, object: &ObjectRef) -> bool {
        self.catalog.as_ref().is_none_or(|seg| {
            object
                .catalog
                .as_deref()
                .is_some_and(|value| seg.matches(value))
        }) && self.schema.as_ref().is_none_or(|seg| {
            object
                .schema
                .as_deref()
                .is_some_and(|value| seg.matches(value))
        }) && self
            .object
            .as_ref()
            .is_none_or(|seg| seg.matches(&object.name))
            && self.column.as_ref().is_none_or(|seg| {
                object
                    .column
                    .as_deref()
                    .is_some_and(|value| seg.matches(value))
            })
    }

    pub fn specificity(&self) -> u8 {
        self.catalog.as_ref().map(Segment::specificity).unwrap_or(0)
            + self.schema.as_ref().map(Segment::specificity).unwrap_or(0)
            + self.object.as_ref().map(Segment::specificity).unwrap_or(0)
            + self.column.as_ref().map(Segment::specificity).unwrap_or(0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelectorRule {
    pub effect: Effect,
    pub selector: Selector,
}

impl SelectorRule {
    pub fn parse(effect: Effect, pattern: &str) -> Result<Self, AppError> {
        Ok(Self {
            effect,
            selector: Selector::parse(pattern)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRef {
    pub catalog: Option<String>,
    pub schema: Option<String>,
    pub name: String,
    pub column: Option<String>,
}

impl ObjectRef {
    pub fn parse(qualified: &str) -> Self {
        let parts: Vec<&str> = qualified.split('.').collect();
        match parts.as_slice() {
            [catalog, schema, name, column] => Self {
                catalog: Some((*catalog).into()),
                schema: Some((*schema).into()),
                name: (*name).into(),
                column: Some((*column).into()),
            },
            [catalog, schema, name] => Self {
                catalog: Some((*catalog).into()),
                schema: Some((*schema).into()),
                name: (*name).into(),
                column: None,
            },
            [schema, name] => Self {
                catalog: None,
                schema: Some((*schema).into()),
                name: (*name).into(),
                column: None,
            },
            [name] => Self {
                catalog: None,
                schema: None,
                name: (*name).into(),
                column: None,
            },
            _ => Self {
                catalog: None,
                schema: None,
                name: qualified.into(),
                column: None,
            },
        }
    }
}
