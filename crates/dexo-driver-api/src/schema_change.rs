use serde::{Deserialize, Serialize};

use crate::{ObjectKind, QualifiedName};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum LockLevel {
    None,
    Share,
    Exclusive,
    AccessExclusive,
}

impl LockLevel {
    pub fn is_sensitive(self) -> bool {
        self != Self::None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeRisk {
    pub destructive: bool,
    pub data_loss: bool,
    pub lock_level: LockLevel,
    pub reversible: bool,
}

impl ChangeRisk {
    pub const fn unknown_sql() -> Self {
        Self {
            destructive: true,
            data_loss: true,
            lock_level: LockLevel::AccessExclusive,
            reversible: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub name: QualifiedName,
    pub data_type: String,
    pub nullable: bool,
    pub default_sql: Option<String>,
    pub identity: Option<IdentitySpec>,
    pub auto_increment: bool,
    pub generated: Option<GeneratedSpec>,
    pub primary_key: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdentitySpec {
    pub always: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GeneratedSpec {
    pub expression: String,
    pub stored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TableShape {
    Table,
    Enum {
        labels: Vec<String>,
    },
    Domain {
        base_type: String,
        check: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PartitionSpec {
    pub method: String,
    pub columns: Vec<QualifiedName>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TableDef {
    pub shape: TableShape,
    pub columns: Vec<ColumnSpec>,
    pub constraints: Vec<ConstraintSpec>,
    pub partition: Option<PartitionSpec>,
    pub engine: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConstraintSpec {
    pub name: QualifiedName,
    pub kind: ConstraintKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConstraintKind {
    PrimaryKey { columns: Vec<QualifiedName> },
    Unique { columns: Vec<QualifiedName> },
    Check { expression: String },
    ForeignKey(ForeignKeySpec),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeySpec {
    pub columns: Vec<QualifiedName>,
    pub referenced_table: QualifiedName,
    pub referenced_columns: Vec<QualifiedName>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    pub table: QualifiedName,
    pub columns: Vec<QualifiedName>,
    pub unique: bool,
    pub concurrently: bool,
    pub method: Option<String>,
    pub include: Vec<QualifiedName>,
    pub predicate: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ViewDef {
    pub sql: String,
    pub materialized: bool,
    pub replace: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RoutineKind {
    Function,
    Procedure,
    Trigger,
    Event,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutineDef {
    pub kind: RoutineKind,
    pub arguments: String,
    pub language: String,
    pub body: String,
    pub returns: Option<String>,
    pub volatility: Option<String>,
    pub table: Option<QualifiedName>,
    pub timing: Option<String>,
    pub schedule: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDef {
    pub table: QualifiedName,
    pub command: String,
    pub using_sql: String,
    pub check_sql: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AlterOp {
    AddColumn(ColumnSpec),
    DropColumn { name: QualifiedName },
    AddIndex(IndexDef),
    DropIndex { name: QualifiedName },
    AddConstraint(ConstraintSpec),
    DropConstraint { name: QualifiedName },
    AddForeignKey(ForeignKeySpec),
    AddPolicy(PolicyDef),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrivilegeDef {
    pub principal: QualifiedName,
    pub privileges: Vec<String>,
    pub with_grant_option: bool,
    pub role_membership: bool,
    pub create_principal: bool,
    pub login: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrantRecord {
    pub principal: QualifiedName,
    pub target: QualifiedName,
    pub privileges: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SchemaChange {
    CreateTable {
        target: QualifiedName,
        def: TableDef,
    },
    AlterTable {
        target: QualifiedName,
        ops: Vec<AlterOp>,
    },
    CreateView {
        target: QualifiedName,
        def: ViewDef,
    },
    AlterRoutine {
        target: QualifiedName,
        def: RoutineDef,
    },
    CreateIndex {
        target: QualifiedName,
        def: IndexDef,
    },
    DropObject {
        target: QualifiedName,
        kind: ObjectKind,
    },
    RenameObject {
        target: QualifiedName,
        new_name: QualifiedName,
    },
    Grant {
        target: QualifiedName,
        def: PrivilegeDef,
    },
    Revoke {
        target: QualifiedName,
        def: PrivilegeDef,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct SchemaChangeError(pub String);

impl SchemaChange {
    pub fn target(&self) -> &QualifiedName {
        match self {
            Self::CreateTable { target, .. }
            | Self::AlterTable { target, .. }
            | Self::CreateView { target, .. }
            | Self::AlterRoutine { target, .. }
            | Self::CreateIndex { target, .. }
            | Self::DropObject { target, .. }
            | Self::RenameObject { target, .. }
            | Self::Grant { target, .. }
            | Self::Revoke { target, .. } => target,
        }
    }

    pub fn validate(&self) -> Result<(), SchemaChangeError> {
        require_name(self.target())?;
        match self {
            Self::CreateTable { def, .. } => {
                for column in &def.columns {
                    require_name(&column.name)?;
                }
                if let Some(partition) = &def.partition {
                    for column in &partition.columns {
                        require_name(column)?;
                    }
                }
            }
            Self::AlterTable { ops, .. } => {
                for op in ops {
                    validate_op(op)?;
                }
            }
            Self::CreateIndex { def, .. } => {
                require_name(&def.table)?;
                for column in &def.columns {
                    require_name(column)?;
                }
            }
            Self::RenameObject { new_name, .. } => require_name(new_name)?,
            Self::Grant { def, .. } | Self::Revoke { def, .. } => require_name(&def.principal)?,
            Self::CreateView { .. } | Self::AlterRoutine { .. } | Self::DropObject { .. } => {}
        }
        Ok(())
    }

    pub fn risk(&self) -> ChangeRisk {
        match self {
            Self::CreateTable { .. } | Self::CreateView { .. } => ChangeRisk {
                destructive: false,
                data_loss: false,
                lock_level: LockLevel::AccessExclusive,
                reversible: true,
            },
            Self::AlterTable { ops, .. } => ops.iter().map(op_risk).fold(
                ChangeRisk {
                    destructive: false,
                    data_loss: false,
                    lock_level: LockLevel::Share,
                    reversible: true,
                },
                merge_risk,
            ),
            Self::AlterRoutine { .. } => ChangeRisk {
                destructive: false,
                data_loss: false,
                lock_level: LockLevel::Exclusive,
                reversible: true,
            },
            Self::CreateIndex { def, .. } => index_risk(def),
            Self::DropObject { .. } => ChangeRisk {
                destructive: true,
                data_loss: true,
                lock_level: LockLevel::AccessExclusive,
                reversible: false,
            },
            Self::RenameObject { .. } => ChangeRisk {
                destructive: false,
                data_loss: false,
                lock_level: LockLevel::AccessExclusive,
                reversible: true,
            },
            Self::Grant { .. } | Self::Revoke { .. } => ChangeRisk {
                destructive: false,
                data_loss: false,
                lock_level: LockLevel::Share,
                reversible: true,
            },
        }
    }
}

fn require_name(name: &QualifiedName) -> Result<(), SchemaChangeError> {
    if name.object().trim().is_empty() {
        return Err(SchemaChangeError(
            "qualified target must be non-empty".into(),
        ));
    }
    Ok(())
}

fn validate_op(op: &AlterOp) -> Result<(), SchemaChangeError> {
    match op {
        AlterOp::AddColumn(column) => require_name(&column.name),
        AlterOp::DropColumn { name }
        | AlterOp::DropIndex { name }
        | AlterOp::DropConstraint { name } => require_name(name),
        AlterOp::AddIndex(index) => {
            require_name(&index.table)?;
            index.columns.iter().try_for_each(require_name)
        }
        AlterOp::AddConstraint(constraint) => require_name(&constraint.name),
        AlterOp::AddForeignKey(fk) => {
            require_name(&fk.referenced_table)?;
            fk.columns.iter().try_for_each(require_name)
        }
        AlterOp::AddPolicy(policy) => require_name(&policy.table),
    }
}

fn index_risk(_def: &IndexDef) -> ChangeRisk {
    ChangeRisk {
        destructive: false,
        data_loss: false,
        lock_level: LockLevel::Share,
        reversible: true,
    }
}

fn op_risk(op: &AlterOp) -> ChangeRisk {
    match op {
        AlterOp::AddIndex(def) => index_risk(def),
        AlterOp::AddColumn(_) | AlterOp::AddConstraint(_) | AlterOp::AddPolicy(_) => ChangeRisk {
            destructive: false,
            data_loss: false,
            lock_level: LockLevel::AccessExclusive,
            reversible: true,
        },
        AlterOp::AddForeignKey(_) => ChangeRisk {
            destructive: false,
            data_loss: false,
            lock_level: LockLevel::Share,
            reversible: true,
        },
        AlterOp::DropColumn { .. } => ChangeRisk {
            destructive: true,
            data_loss: true,
            lock_level: LockLevel::AccessExclusive,
            reversible: false,
        },
        AlterOp::DropIndex { .. } | AlterOp::DropConstraint { .. } => ChangeRisk {
            destructive: true,
            data_loss: false,
            lock_level: LockLevel::AccessExclusive,
            reversible: false,
        },
    }
}

fn merge_risk(a: ChangeRisk, b: ChangeRisk) -> ChangeRisk {
    ChangeRisk {
        destructive: a.destructive || b.destructive,
        data_loss: a.data_loss || b.data_loss,
        lock_level: a.lock_level.max(b.lock_level),
        reversible: a.reversible && b.reversible,
    }
}

pub fn classify_raw_sql(sql: &str) -> ChangeRisk {
    let trimmed = sql.trim_start();
    if trimmed.is_empty() {
        return ChangeRisk::unknown_sql();
    }
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("DROP ") {
        return ChangeRisk {
            destructive: true,
            data_loss: true,
            lock_level: LockLevel::AccessExclusive,
            reversible: false,
        };
    }
    if upper.contains("CREATE INDEX") || upper.contains("ADD INDEX") {
        return ChangeRisk {
            destructive: false,
            data_loss: false,
            lock_level: LockLevel::Share,
            reversible: true,
        };
    }
    if upper.starts_with("CREATE ") || upper.starts_with("GRANT ") || upper.starts_with("REVOKE ") {
        return ChangeRisk {
            destructive: false,
            data_loss: false,
            lock_level: LockLevel::AccessExclusive,
            reversible: true,
        };
    }
    ChangeRisk::unknown_sql()
}

#[cfg(test)]
mod tests {
    use super::{
        AlterOp, ChangeRisk, ColumnSpec, IndexDef, LockLevel, PrivilegeDef, RoutineDef,
        RoutineKind, SchemaChange, TableDef, TableShape, ViewDef, classify_raw_sql,
    };
    use crate::{ObjectKind, QualifiedName};

    fn q(catalog: &str, schema: &str, object: &str) -> QualifiedName {
        QualifiedName::new(Some(catalog), Some(schema), object)
    }

    fn ident(name: &str) -> QualifiedName {
        QualifiedName::new(None::<String>, None::<String>, name)
    }

    fn empty_table() -> TableDef {
        TableDef {
            shape: TableShape::Table,
            columns: vec![ColumnSpec {
                name: ident("id"),
                data_type: "bigint".into(),
                nullable: false,
                default_sql: None,
                identity: None,
                auto_increment: false,
                generated: None,
                primary_key: true,
            }],
            constraints: vec![],
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        }
    }

    fn add_index_change() -> SchemaChange {
        SchemaChange::AlterTable {
            target: q("prod", "public", "orders"),
            ops: vec![AlterOp::AddIndex(IndexDef {
                table: q("prod", "public", "orders"),
                columns: vec![ident("id")],
                unique: false,
                concurrently: false,
                method: Some("btree".into()),
                include: vec![],
                predicate: None,
            })],
        }
    }

    #[test]
    fn drop_object_is_destructive_irreversible_and_add_index_is_lock_sensitive_reversible() {
        let drop = SchemaChange::DropObject {
            target: q("prod", "public", "orders"),
            kind: ObjectKind::Table,
        };
        let drop_risk = drop.risk();
        assert!(drop_risk.destructive);
        assert!(!drop_risk.reversible);
        let index_risk = add_index_change().risk();
        assert!(index_risk.lock_level.is_sensitive());
        assert!(index_risk.reversible);
        assert!(!index_risk.destructive);
    }

    #[test]
    fn every_variant_has_risk_classification() {
        let changes = [
            SchemaChange::CreateTable {
                target: q("db", "public", "orders"),
                def: empty_table(),
            },
            SchemaChange::AlterTable {
                target: q("db", "public", "orders"),
                ops: vec![AlterOp::DropColumn { name: ident("qty") }],
            },
            add_index_change(),
            SchemaChange::CreateView {
                target: q("db", "public", "v"),
                def: ViewDef {
                    sql: "SELECT 1".into(),
                    materialized: false,
                    replace: false,
                },
            },
            SchemaChange::AlterRoutine {
                target: q("db", "public", "add1"),
                def: RoutineDef {
                    kind: RoutineKind::Function,
                    arguments: "n integer".into(),
                    language: "sql".into(),
                    body: "SELECT n + 1".into(),
                    returns: Some("integer".into()),
                    volatility: None,
                    table: None,
                    timing: None,
                    schedule: None,
                },
            },
            SchemaChange::CreateIndex {
                target: ident("orders_id_idx"),
                def: IndexDef {
                    table: q("db", "public", "orders"),
                    columns: vec![ident("id")],
                    unique: true,
                    concurrently: true,
                    method: Some("btree".into()),
                    include: vec![ident("qty")],
                    predicate: Some("id > 0".into()),
                },
            },
            SchemaChange::DropObject {
                target: q("db", "public", "orders"),
                kind: ObjectKind::Table,
            },
            SchemaChange::RenameObject {
                target: q("db", "public", "orders"),
                new_name: q("db", "public", "order_rows"),
            },
            SchemaChange::Grant {
                target: q("db", "public", "orders"),
                def: PrivilegeDef {
                    principal: ident("reporter"),
                    privileges: vec!["SELECT".into()],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: false,
                    login: false,
                },
            },
            SchemaChange::Revoke {
                target: q("db", "public", "orders"),
                def: PrivilegeDef {
                    principal: ident("reporter"),
                    privileges: vec!["SELECT".into()],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: false,
                    login: false,
                },
            },
        ];
        for change in &changes {
            change.validate().unwrap();
            let risk: ChangeRisk = change.risk();
            let _ = risk.lock_level;
        }
    }

    #[test]
    fn unknown_sql_is_treated_as_destructive() {
        let risk = classify_raw_sql("VACUUM FULL mystery()");
        assert!(risk.destructive);
        assert!(!risk.reversible);
        assert_eq!(risk.lock_level, LockLevel::AccessExclusive);
    }
}
