use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeState {
    Unloaded,
    Loading,
    Loaded,
    Restricted,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerAction {
    Properties,
    Ddl,
    Data,
    Dependencies,
    Dependents,
    CopyName,
}

impl ExplorerAction {
    pub fn all() -> [ExplorerAction; 6] {
        [
            Self::Properties,
            Self::Ddl,
            Self::Data,
            Self::Dependencies,
            Self::Dependents,
            Self::CopyName,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Properties => "properties",
            Self::Ddl => "DDL",
            Self::Data => "data",
            Self::Dependencies => "dependencies",
            Self::Dependents => "dependents",
            Self::CopyName => "copy-name",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplorerNode {
    pub id: ObjectId,
    pub label: String,
    pub kind: ObjectKind,
    pub qualified: String,
    pub schema: Option<String>,
    pub state: NodeState,
    pub expanded: bool,
    pub favorite: bool,
    pub children: Vec<ExplorerNode>,
    pub restriction: Option<String>,
    pub error: Option<String>,
}

impl ExplorerNode {
    pub fn from_object(object: CatalogObject) -> Self {
        Self {
            id: object.id,
            label: object.qualified_name.object().to_string(),
            kind: object.kind,
            qualified: object.qualified_name.display_unquoted(),
            schema: object.qualified_name.schema().map(str::to_string),
            state: NodeState::Unloaded,
            expanded: false,
            favorite: false,
            children: Vec::new(),
            restriction: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExplorerState {
    pub roots: Vec<ExplorerNode>,
    pub selected: Option<ObjectId>,
    pub offline: bool,
    pub filter_name: String,
    pub filter_kind: Option<String>,
    pub filter_schema: Option<String>,
    pub favorites_only: bool,
    pub search: String,
    pub last_load: Option<ObjectId>,
    pub copied: Option<String>,
}

impl ExplorerState {
    pub fn fixture() -> Self {
        let schema = ExplorerNode {
            id: ObjectId::new("schema:public"),
            label: "public".into(),
            kind: ObjectKind::Schema,
            qualified: "local.public".into(),
            schema: Some("public".into()),
            state: NodeState::Unloaded,
            expanded: false,
            favorite: false,
            children: Vec::new(),
            restriction: None,
            error: None,
        };
        let mut root = ExplorerNode {
            id: ObjectId::new("catalog:local"),
            label: "local".into(),
            kind: ObjectKind::Catalog,
            qualified: "local".into(),
            schema: None,
            state: NodeState::Loaded,
            expanded: true,
            favorite: false,
            children: vec![schema],
            restriction: None,
            error: None,
        };
        root.children.push(ExplorerNode {
            id: ObjectId::new("restricted:users"),
            label: "mysql.users".into(),
            kind: ObjectKind::User,
            qualified: "local.mysql.users".into(),
            schema: None,
            state: NodeState::Restricted,
            expanded: false,
            favorite: false,
            children: Vec::new(),
            restriction: Some("permission denied".into()),
            error: None,
        });
        Self {
            roots: vec![root],
            selected: Some(ObjectId::new("schema:public")),
            offline: true,
            ..Self::default()
        }
    }

    pub fn expand(&mut self, id: &ObjectId) -> bool {
        Self::expand_in(&mut self.roots, id, &mut self.last_load, &mut self.selected)
    }

    fn expand_in(
        nodes: &mut [ExplorerNode],
        id: &ObjectId,
        last_load: &mut Option<ObjectId>,
        selected: &mut Option<ObjectId>,
    ) -> bool {
        for node in nodes {
            if node.id == *id {
                node.expanded = true;
                *selected = Some(id.clone());
                if node.state == NodeState::Unloaded {
                    node.state = NodeState::Loading;
                    *last_load = Some(id.clone());
                    return true;
                }
                return false;
            }
            if Self::expand_in(&mut node.children, id, last_load, selected) {
                return true;
            }
        }
        false
    }

    pub fn apply_children(&mut self, parent: &ObjectId, page: CatalogList) {
        Self::apply_in(&mut self.roots, parent, page);
    }

    fn apply_in(nodes: &mut [ExplorerNode], parent: &ObjectId, page: CatalogList) {
        for node in nodes {
            if node.id == *parent {
                node.children = page
                    .objects
                    .into_iter()
                    .map(ExplorerNode::from_object)
                    .collect();
                for restriction in page.restrictions {
                    node.children.push(ExplorerNode {
                        id: ObjectId::new(format!("restricted:{}", restriction.capability)),
                        label: restriction.capability,
                        kind: ObjectKind::DriverSpecific("restricted".into()),
                        qualified: String::new(),
                        schema: None,
                        state: NodeState::Restricted,
                        expanded: false,
                        favorite: false,
                        children: Vec::new(),
                        restriction: Some(restriction.reason),
                        error: None,
                    });
                }
                node.state = if node
                    .children
                    .iter()
                    .any(|child| child.state == NodeState::Restricted)
                    && node
                        .children
                        .iter()
                        .all(|child| child.state == NodeState::Restricted)
                {
                    NodeState::Restricted
                } else {
                    NodeState::Loaded
                };
                return;
            }
            Self::apply_in(&mut node.children, parent, page.clone());
        }
    }

    pub fn visible_ids(&self) -> Vec<ObjectId> {
        let mut out = Vec::new();
        self.collect_visible(&self.roots, &mut out);
        out
    }

    fn collect_visible(&self, nodes: &[ExplorerNode], out: &mut Vec<ObjectId>) {
        for node in nodes {
            if self.matches(node) {
                out.push(node.id.clone());
            }
            if node.expanded {
                self.collect_visible(&node.children, out);
            }
        }
    }

    pub fn matches(&self, node: &ExplorerNode) -> bool {
        if self.favorites_only && !node.favorite {
            return false;
        }
        if let Some(kind) = &self.filter_kind
            && node.kind.as_str() != kind
        {
            return false;
        }
        if let Some(schema) = &self.filter_schema
            && node.schema.as_deref() != Some(schema.as_str())
        {
            return false;
        }
        if !self.filter_name.is_empty()
            && !node
                .label
                .to_ascii_lowercase()
                .contains(&self.filter_name.to_ascii_lowercase())
        {
            return false;
        }
        true
    }

    pub fn toggle_favorite(&mut self, id: &ObjectId) {
        Self::toggle_in(&mut self.roots, id);
    }

    fn toggle_in(nodes: &mut [ExplorerNode], id: &ObjectId) -> bool {
        for node in nodes {
            if node.id == *id {
                node.favorite = !node.favorite;
                return true;
            }
            if Self::toggle_in(&mut node.children, id) {
                return true;
            }
        }
        false
    }

    pub fn select(&mut self, id: ObjectId) {
        self.selected = Some(id);
    }

    pub fn copy_selected_name(&mut self) {
        if let Some(id) = &self.selected
            && let Some(node) = Self::find(&self.roots, id)
        {
            self.copied = Some(node.qualified.clone());
        }
    }

    fn find<'a>(nodes: &'a [ExplorerNode], id: &ObjectId) -> Option<&'a ExplorerNode> {
        for node in nodes {
            if node.id == *id {
                return Some(node);
            }
            if let Some(found) = Self::find(&node.children, id) {
                return Some(found);
            }
        }
        None
    }

    pub fn selected_node(&self) -> Option<&ExplorerNode> {
        self.selected
            .as_ref()
            .and_then(|id| Self::find(&self.roots, id))
    }
}

#[cfg(test)]
mod tests {
    use super::{ExplorerAction, ExplorerState, NodeState};
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

    #[test]
    fn expand_loads_only_one_subtree() {
        let mut explorer = ExplorerState::fixture();
        assert!(explorer.expand(&ObjectId::new("schema:public")));
        assert_eq!(explorer.last_load, Some(ObjectId::new("schema:public")));
        let schema = explorer.roots[0]
            .children
            .iter()
            .find(|node| node.label == "public")
            .unwrap();
        assert_eq!(schema.state, NodeState::Loading);
        explorer.apply_children(
            &ObjectId::new("schema:public"),
            CatalogList {
                objects: vec![CatalogObject::new(
                    ObjectId::new("table:orders"),
                    ObjectKind::Table,
                    QualifiedName::new(Some("local"), Some("public"), "orders"),
                    Some(ObjectId::new("schema:public")),
                )],
                restrictions: vec![],
            },
        );
        assert_eq!(
            explorer.last_load.as_ref().unwrap().as_str(),
            "schema:public"
        );
        assert_eq!(explorer.roots[0].children[0].children.len(), 1);
    }

    #[test]
    fn filters_and_actions_are_available() {
        let mut explorer = ExplorerState::fixture();
        explorer.filter_name = "public".into();
        let ids = explorer.visible_ids();
        assert!(ids.iter().any(|id| id.as_str() == "schema:public"));
        explorer.filter_kind = Some("user".into());
        explorer.filter_name.clear();
        let ids = explorer.visible_ids();
        assert!(ids.iter().any(|id| id.as_str() == "restricted:users"));
        explorer.copy_selected_name();
        assert_eq!(explorer.copied.as_deref(), Some("local.public"));
        let labels: Vec<_> = ExplorerAction::all()
            .iter()
            .map(|action| action.label())
            .collect();
        assert_eq!(
            labels,
            [
                "properties",
                "DDL",
                "data",
                "dependencies",
                "dependents",
                "copy-name"
            ]
        );
    }
}
