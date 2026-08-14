use dexo_driver_api::{ChangeRisk, SchemaChange};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Confirmation {
    None,
    TypeTarget(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub confirmation: Confirmation,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DdlPolicy {
    pub read_only: bool,
    pub typed_destructive: bool,
}

pub fn production_policy() -> DdlPolicy {
    DdlPolicy {
        read_only: false,
        typed_destructive: true,
    }
}

pub fn read_only_policy() -> DdlPolicy {
    DdlPolicy {
        read_only: true,
        typed_destructive: true,
    }
}

pub fn evaluate(change: &SchemaChange, policy: &DdlPolicy) -> PolicyDecision {
    if policy.read_only {
        return PolicyDecision {
            allowed: false,
            confirmation: Confirmation::None,
            reason: Some("connection is read-only".into()),
        };
    }
    let risk = change.risk();
    if policy.typed_destructive && is_destructive(&risk) {
        return PolicyDecision {
            allowed: true,
            confirmation: Confirmation::TypeTarget(change.target().display_unquoted()),
            reason: None,
        };
    }
    PolicyDecision {
        allowed: true,
        confirmation: Confirmation::None,
        reason: None,
    }
}

fn is_destructive(risk: &ChangeRisk) -> bool {
    risk.destructive || risk.data_loss
}

#[cfg(test)]
mod tests {
    use super::{Confirmation, evaluate, production_policy, read_only_policy};
    use crate::schema::change::drop_table;
    use dexo_driver_api::{PrivilegeDef, QualifiedName, SchemaChange};

    #[test]
    fn production_drop_requires_typed_target() {
        let decision = evaluate(&drop_table("prod.public.orders"), &production_policy());
        assert_eq!(
            decision.confirmation,
            Confirmation::TypeTarget("prod.public.orders".into())
        );
    }

    #[test]
    fn read_only_denies_ddl() {
        let decision = evaluate(&drop_table("db.public.t"), &read_only_policy());
        assert!(!decision.allowed);
        assert_eq!(decision.reason.as_deref(), Some("connection is read-only"));
    }

    #[test]
    fn grant_in_production_does_not_require_typed_target() {
        let change = SchemaChange::Grant {
            target: QualifiedName::new(Some("prod"), Some("public"), "orders"),
            def: PrivilegeDef {
                principal: QualifiedName::new(None::<String>, None::<String>, "reporter"),
                privileges: vec!["SELECT".into()],
                with_grant_option: false,
                role_membership: false,
                create_principal: false,
                login: false,
            },
        };
        let decision = evaluate(&change, &production_policy());
        assert!(decision.allowed);
        assert_eq!(decision.confirmation, Confirmation::None);
    }
}
