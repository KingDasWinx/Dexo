use std::fmt;

use dexo_driver_api::{CatalogObject, ObjectKind, QualifiedName};
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
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Star => f.write_str("*"),
            Self::Exact(name) => f.write_str(name),
        }
    }
}

/// A dotted pattern matched segment by segment against the front of an object's path.
/// `db.public.*` covers every object in schema `public` of catalog `db`, and `db.*`
/// covers the whole catalog. Postgres paths are `catalog.schema.object` and MySQL paths
/// `catalog.object`, so one rule shape serves both without guessing which slot a name
/// belongs in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    pub segments: Vec<Segment>,
}

impl Selector {
    pub fn parse(pattern: &str) -> Result<Self, AppError> {
        let segments = pattern
            .split('.')
            .map(Segment::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if segments.is_empty() || segments.len() > 3 {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "selector must have 1..=3 segments (catalog.schema.object)",
            ));
        }
        Ok(Self { segments })
    }

    pub fn matches(&self, object: &ObjectRef) -> bool {
        self.segments.len() <= object.path.len()
            && self
                .segments
                .iter()
                .zip(&object.path)
                .all(|(segment, part)| segment.matches(part))
    }

    pub fn specificity(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Exact(_)))
            .count()
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.segments.iter().map(ToString::to_string).collect();
        f.write_str(&parts.join("."))
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

impl fmt::Display for SelectorRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let effect = match self.effect {
            Effect::Allow => "allow",
            Effect::Deny => "deny",
        };
        write!(f, "{effect} {}", self.selector)
    }
}

/// An object's position in the catalog tree, outermost segment first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRef {
    pub path: Vec<String>,
}

impl ObjectRef {
    pub fn parse(qualified: &str) -> Self {
        Self {
            path: qualified
                .split('.')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn from_qualified(name: &QualifiedName) -> Self {
        let mut path: Vec<String> = name
            .catalog()
            .into_iter()
            .chain(name.schema())
            .map(str::to_string)
            .collect();
        path.push(name.object().to_string());
        Self { path }
    }

    /// Drivers repeat a node's own name in the object slot (a Postgres schema is
    /// `db.public.public`) and store columns as `table.column`; both are mapped to the
    /// path the node really occupies, so a column belongs to its table.
    pub fn from_catalog_object(object: &CatalogObject) -> Self {
        let name = &object.qualified_name;
        match &object.kind {
            ObjectKind::Catalog => Self {
                path: vec![name.object().to_string()],
            },
            ObjectKind::Schema => Self {
                path: name
                    .catalog()
                    .into_iter()
                    .map(str::to_string)
                    .chain([name.object().to_string()])
                    .collect(),
            },
            ObjectKind::Column => {
                let mut reference = Self::from_qualified(name);
                if let Some(last) = reference.path.last_mut()
                    && let Some((table, _)) = last.split_once('.')
                {
                    *last = table.to_string();
                }
                reference
            }
            _ => Self::from_qualified(name),
        }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, ObjectRef, Selector, SelectorRule};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn object(kind: ObjectKind, name: QualifiedName) -> CatalogObject {
        CatalogObject::new(ObjectId::new("x"), kind, name, None)
    }

    #[test]
    fn two_part_mysql_names_match_catalog_selectors() {
        let items =
            ObjectRef::from_qualified(&QualifiedName::new(Some("shop"), None::<String>, "items"));
        assert!(Selector::parse("shop.*").unwrap().matches(&items));
        assert!(Selector::parse("shop.items").unwrap().matches(&items));
        assert!(!Selector::parse("shop.public.*").unwrap().matches(&items));
    }

    #[test]
    fn a_column_belongs_to_its_table() {
        let column = object(
            ObjectKind::Column,
            QualifiedName::new(Some("db"), Some("public"), "secrets.email"),
        );
        assert_eq!(
            ObjectRef::from_catalog_object(&column).path,
            ["db", "public", "secrets"]
        );
    }

    #[test]
    fn container_nodes_do_not_repeat_their_name() {
        let schema = object(
            ObjectKind::Schema,
            QualifiedName::new(Some("db"), Some("public"), "public"),
        );
        let catalog = object(
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), None::<String>, "db"),
        );
        assert_eq!(
            ObjectRef::from_catalog_object(&schema).path,
            ["db", "public"]
        );
        assert_eq!(ObjectRef::from_catalog_object(&catalog).path, ["db"]);
    }

    #[test]
    fn column_and_partial_wildcard_selectors_are_rejected() {
        assert!(Selector::parse("db.public.t.col").is_err());
        assert!(Selector::parse("db.pub*").is_err());
        assert!(Selector::parse("").is_err());
    }

    #[test]
    fn rules_render_as_they_are_typed() {
        let rule = SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap();
        assert_eq!(rule.to_string(), "deny db.public.secrets");
    }
}
