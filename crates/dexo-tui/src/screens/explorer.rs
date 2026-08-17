use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind};

use crate::runtime::OperationId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeState {
    Collapsed,
    Loading(OperationId),
    Expanded,
    Error { message: String, retryable: bool },
    Stale,
    Restricted,
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

pub fn opens_table_data(kind: &ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
    )
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
    pub fn can_expand(&self) -> bool {
        if !self.children.is_empty() {
            return true;
        }
        matches!(
            self.kind,
            ObjectKind::Catalog
                | ObjectKind::Schema
                | ObjectKind::Table
                | ObjectKind::View
                | ObjectKind::MaterializedView
        )
    }

    pub fn from_object(object: CatalogObject) -> Self {
        let label = object_label(&object);
        Self {
            id: object.id,
            label,
            kind: object.kind,
            qualified: object.qualified_name.display_unquoted(),
            schema: object.qualified_name.schema().map(str::to_string),
            state: NodeState::Collapsed,
            expanded: false,
            favorite: object
                .attributes
                .get("favorite")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            children: Vec::new(),
            restriction: None,
            error: None,
        }
    }
}

fn object_label(object: &CatalogObject) -> String {
    let name = object.qualified_name.object();
    if object.kind == ObjectKind::Column {
        name.rsplit('.').next().unwrap_or(name).to_string()
    } else {
        name.to_string()
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
    pub include_system: bool,
    pub stale: bool,
    pub offset: usize,
}

impl ExplorerState {
    #[cfg(test)]
    pub fn fixture() -> Self {
        let schema = ExplorerNode {
            id: ObjectId::new("schema:public"),
            label: "public".into(),
            kind: ObjectKind::Schema,
            qualified: "local.public".into(),
            schema: Some("public".into()),
            state: NodeState::Collapsed,
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
            state: NodeState::Expanded,
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

    pub fn nodes(&self) -> &[ExplorerNode] {
        &self.roots
    }

    pub fn clear(&mut self) {
        self.roots.clear();
        self.selected = None;
        self.last_load = None;
        self.stale = false;
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
        Self::mark_stale_in(&mut self.roots);
    }

    fn mark_stale_in(nodes: &mut [ExplorerNode]) {
        for node in nodes {
            if matches!(node.state, NodeState::Expanded | NodeState::Collapsed) {
                node.state = NodeState::Stale;
            }
            Self::mark_stale_in(&mut node.children);
        }
    }

    pub fn collapse(&mut self, id: &ObjectId) -> bool {
        Self::collapse_in(&mut self.roots, id)
    }

    fn collapse_in(nodes: &mut [ExplorerNode], id: &ObjectId) -> bool {
        for node in nodes {
            if node.id == *id {
                if !node.expanded {
                    return false;
                }
                node.expanded = false;
                return true;
            }
            if Self::collapse_in(&mut node.children, id) {
                return true;
            }
        }
        false
    }

    pub fn expand(&mut self, id: &ObjectId) -> bool {
        self.expand_with(id, OperationId::new())
    }

    pub fn expand_with(&mut self, id: &ObjectId, operation: OperationId) -> bool {
        Self::expand_in(
            &mut self.roots,
            id,
            operation,
            &mut self.last_load,
            &mut self.selected,
        )
    }

    fn expand_in(
        nodes: &mut [ExplorerNode],
        id: &ObjectId,
        operation: OperationId,
        last_load: &mut Option<ObjectId>,
        selected: &mut Option<ObjectId>,
    ) -> bool {
        for node in nodes {
            if node.id == *id {
                node.expanded = true;
                *selected = Some(id.clone());
                if matches!(
                    node.state,
                    NodeState::Collapsed
                        | NodeState::Stale
                        | NodeState::Error {
                            retryable: true,
                            ..
                        }
                ) {
                    node.state = NodeState::Loading(operation);
                    *last_load = Some(id.clone());
                    return true;
                }
                return false;
            }
            if Self::expand_in(&mut node.children, id, operation, last_load, selected) {
                return true;
            }
        }
        false
    }

    pub fn apply_children(&mut self, parent: &ObjectId, page: CatalogList) {
        Self::apply_in(&mut self.roots, parent, page);
    }

    pub fn replace_roots(&mut self, page: CatalogList) {
        self.roots = page
            .objects
            .into_iter()
            .map(ExplorerNode::from_object)
            .collect();
        for restriction in page.restrictions {
            self.roots.push(restriction_node(restriction));
        }
        self.stale = false;
        self.offline = false;
    }

    pub fn set_error(&mut self, id: &ObjectId, message: String, retryable: bool) {
        Self::set_error_in(&mut self.roots, id, message, retryable);
    }

    fn set_error_in(nodes: &mut [ExplorerNode], id: &ObjectId, message: String, retryable: bool) {
        for node in nodes {
            if node.id == *id {
                node.state = NodeState::Error { message, retryable };
                return;
            }
            Self::set_error_in(&mut node.children, id, message.clone(), retryable);
        }
    }

    fn apply_in(nodes: &mut [ExplorerNode], parent: &ObjectId, page: CatalogList) {
        for node in nodes {
            if node.id == *parent {
                node.children = group_catalog_children(&node.id, &node.kind, page.objects);
                for restriction in page.restrictions {
                    node.children.push(restriction_node(restriction));
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
                    NodeState::Expanded
                };
                node.expanded = true;
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
        if !self.include_system && is_system_node(node) {
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
        if !self.search.is_empty()
            && !node
                .label
                .to_ascii_lowercase()
                .contains(&self.search.to_ascii_lowercase())
            && !node
                .qualified
                .to_ascii_lowercase()
                .contains(&self.search.to_ascii_lowercase())
        {
            return false;
        }
        true
    }

    pub fn flatten(&self) -> Vec<CatalogObject> {
        let mut out = Vec::new();
        fn walk(nodes: &[ExplorerNode], out: &mut Vec<CatalogObject>) {
            for node in nodes {
                if matches!(&node.kind, ObjectKind::DriverSpecific(kind) if kind == "folder") {
                    walk(&node.children, out);
                    continue;
                }
                let mut object = CatalogObject::new(
                    node.id.clone(),
                    node.kind.clone(),
                    dexo_app::parse_qualified(&node.qualified),
                    None,
                );
                if node.favorite {
                    object
                        .attributes
                        .insert("favorite".into(), serde_json::json!(true));
                }
                out.push(object);
                walk(&node.children, out);
            }
        }
        walk(&self.roots, &mut out);
        out
    }

    pub fn apply_favorites(&mut self, ids: &[String]) {
        fn walk(nodes: &mut [ExplorerNode], ids: &[String]) {
            for node in nodes {
                node.favorite = ids.iter().any(|id| id == node.id.as_str());
                walk(&mut node.children, ids);
            }
        }
        walk(&mut self.roots, ids);
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

    pub fn move_selection(&mut self, delta: i32) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|id| ids.iter().position(|candidate| candidate == id))
            .unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize;
        self.selected = Some(ids[next].clone());
    }

    pub fn selected_index(&self) -> usize {
        let ids = self.visible_ids();
        self.selected
            .as_ref()
            .and_then(|id| ids.iter().position(|candidate| candidate == id))
            .unwrap_or(0)
    }

    pub fn sync_scroll(&mut self, rows: usize) {
        self.offset = crate::palette::scroll_to_selection(
            self.selected_index(),
            self.offset,
            self.visible_ids().len(),
            rows,
        );
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

fn group_catalog_children(
    parent: &ObjectId,
    kind: &ObjectKind,
    objects: Vec<CatalogObject>,
) -> Vec<ExplorerNode> {
    match kind {
        ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView => group_by_kind(
            parent,
            objects,
            &[
                (bucket_column, "Columns"),
                (bucket_index, "Indexes"),
                (bucket_constraint, "Constraints"),
                (bucket_trigger, "Triggers"),
                (bucket_rls, "RLS"),
                (bucket_rule, "Rules"),
                (bucket_partition, "Partitions"),
            ],
        ),
        ObjectKind::Schema => group_schema_children(parent, objects),
        _ => objects.into_iter().map(ExplorerNode::from_object).collect(),
    }
}

fn group_schema_children(parent: &ObjectId, objects: Vec<CatalogObject>) -> Vec<ExplorerNode> {
    let mut tables = Vec::new();
    let mut rest = Vec::new();
    for object in objects {
        if matches!(
            object.kind,
            ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
        ) {
            tables.push(ExplorerNode::from_object(object));
        } else {
            rest.push(object);
        }
    }
    let mut children = tables;
    children.extend(group_by_kind(
        parent,
        rest,
        &[
            (bucket_function, "Functions"),
            (bucket_procedure, "Procedures"),
            (bucket_sequence, "Sequences"),
            (bucket_type, "Types"),
        ],
    ));
    children
}

type KindBucket<'a> = (fn(&ObjectKind) -> bool, &'a str);

fn group_by_kind(
    parent: &ObjectId,
    objects: Vec<CatalogObject>,
    buckets: &[KindBucket<'_>],
) -> Vec<ExplorerNode> {
    let mut leftover = objects;
    let mut folders = Vec::new();
    for (pred, label) in buckets {
        let mut taken = Vec::new();
        leftover.retain(|object| {
            if pred(&object.kind) {
                taken.push(ExplorerNode::from_object(object.clone()));
                false
            } else {
                true
            }
        });
        if let Some(folder) = folder_node(parent, label, taken) {
            folders.push(folder);
        }
    }
    folders
        .into_iter()
        .chain(leftover.into_iter().map(ExplorerNode::from_object))
        .collect()
}

fn folder_node(
    parent: &ObjectId,
    label: &str,
    children: Vec<ExplorerNode>,
) -> Option<ExplorerNode> {
    if children.is_empty() {
        return None;
    }
    Some(ExplorerNode {
        id: ObjectId::new(format!("folder:{}:{label}", parent.as_str())),
        label: label.into(),
        kind: ObjectKind::DriverSpecific("folder".into()),
        qualified: label.into(),
        schema: None,
        state: NodeState::Expanded,
        expanded: false,
        favorite: false,
        children,
        restriction: None,
        error: None,
    })
}

fn bucket_column(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Column)
}
fn bucket_index(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Index)
}
fn bucket_constraint(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Constraint)
}
fn bucket_trigger(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Trigger)
}
fn bucket_rls(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::DriverSpecific(name) if name == "policy")
}
fn bucket_rule(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::DriverSpecific(name) if name == "rule")
}
fn bucket_partition(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::DriverSpecific(name) if name == "partition")
}
fn bucket_function(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Function)
}
fn bucket_procedure(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Procedure)
}
fn bucket_sequence(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Sequence)
}
fn bucket_type(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::DriverSpecific(name) if name == "enum" || name == "domain")
}

fn restriction_node(restriction: dexo_driver_api::CatalogRestriction) -> ExplorerNode {
    ExplorerNode {
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
    }
}

fn is_system_node(node: &ExplorerNode) -> bool {
    let name = node.label.as_str();
    name == "pg_catalog"
        || name == "information_schema"
        || name == "mysql"
        || name == "performance_schema"
        || name == "sys"
        || name.starts_with("pg_")
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
        assert!(matches!(schema.state, NodeState::Loading(_)));
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

    #[test]
    fn enter_hint_only_on_tables() {
        use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

        let mut explorer = ExplorerState::default();
        explorer.replace_roots(CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("table:orders"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "orders"),
                None,
            )],
            restrictions: vec![],
        });
        explorer.select(ObjectId::new("table:orders"));
        let lines = crate::widgets::object_tree::render_lines(&explorer);
        assert!(
            lines
                .iter()
                .any(|line| line.contains('>') && line.contains("orders")),
            "{lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("actions:") || line.contains("selected:")),
            "{lines:?}"
        );
        assert!(super::opens_table_data(&ObjectKind::View));
        assert!(!super::opens_table_data(&ObjectKind::Schema));
    }

    #[test]
    fn apply_children_groups_table_and_schema() {
        let mut explorer = ExplorerState::default();
        explorer.replace_roots(CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("schema:public"),
                ObjectKind::Schema,
                QualifiedName::new(Some("db"), Some("public"), "public"),
                None,
            )],
            restrictions: vec![],
        });
        explorer.apply_children(
            &ObjectId::new("schema:public"),
            CatalogList {
                objects: vec![
                    CatalogObject::new(
                        ObjectId::new("table:users"),
                        ObjectKind::Table,
                        QualifiedName::new(Some("db"), Some("public"), "users"),
                        Some(ObjectId::new("schema:public")),
                    ),
                    CatalogObject::new(
                        ObjectId::new("fn:armor"),
                        ObjectKind::Function,
                        QualifiedName::new(Some("db"), Some("public"), "armor"),
                        Some(ObjectId::new("schema:public")),
                    ),
                ],
                restrictions: vec![],
            },
        );
        {
            let schema = &explorer.roots[0];
            assert!(schema.children.iter().any(|n| n.label == "users"));
            let functions = schema
                .children
                .iter()
                .find(|n| n.label == "Functions")
                .expect("functions folder");
            assert!(functions.children.iter().any(|n| n.label == "armor"));
            assert!(!schema.children.iter().any(|n| n.label == "armor"));
        }

        explorer.apply_children(
            &ObjectId::new("table:users"),
            CatalogList {
                objects: vec![
                    CatalogObject::new(
                        ObjectId::new("col:id"),
                        ObjectKind::Column,
                        QualifiedName::new(Some("db"), Some("public"), "users.id"),
                        Some(ObjectId::new("table:users")),
                    ),
                    CatalogObject::new(
                        ObjectId::new("idx:users_pk"),
                        ObjectKind::Index,
                        QualifiedName::new(Some("db"), Some("public"), "users_pk"),
                        Some(ObjectId::new("table:users")),
                    ),
                ],
                restrictions: vec![],
            },
        );
        let table = explorer.roots[0]
            .children
            .iter()
            .find(|n| n.label == "users")
            .unwrap();
        assert!(table.children.iter().any(|n| n.label == "Columns"));
        assert!(table.children.iter().any(|n| n.label == "Indexes"));
        assert!(!table.children.iter().any(|n| n.kind == ObjectKind::Column));
        let columns = table
            .children
            .iter()
            .find(|n| n.label == "Columns")
            .unwrap();
        assert!(columns.children.iter().any(|n| n.label == "id"));
        assert!(!columns.children.iter().any(|n| n.label == "users.id"));
    }
}
