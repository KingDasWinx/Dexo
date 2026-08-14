use crate::mcp::selector::{Effect, ObjectRef, SelectorRule};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    DenyHidden,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectPolicy {
    pub rules: Vec<SelectorRule>,
}

impl ObjectPolicy {
    pub fn new(rules: Vec<SelectorRule>) -> Self {
        Self { rules }
    }

    pub fn decide(&self, object: &ObjectRef) -> Decision {
        let mut matched: Vec<&SelectorRule> = self
            .rules
            .iter()
            .filter(|rule| rule.selector.matches(object))
            .collect();
        matched.sort_by_key(|rule| std::cmp::Reverse(rule.selector.specificity()));
        if matched.iter().any(|rule| rule.effect == Effect::Deny) {
            return Decision::DenyHidden;
        }
        if matched.iter().any(|rule| rule.effect == Effect::Allow) {
            Decision::Allow
        } else {
            Decision::DenyHidden
        }
    }

    pub fn hidden_error() -> &'static str {
        "not found"
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, ObjectPolicy};
    use crate::mcp::selector::{Effect, ObjectRef, SelectorRule};

    fn policy(rules: Vec<SelectorRule>) -> ObjectPolicy {
        ObjectPolicy::new(rules)
    }

    fn allow(pattern: &str) -> SelectorRule {
        SelectorRule::parse(Effect::Allow, pattern).unwrap()
    }

    fn deny(pattern: &str) -> SelectorRule {
        SelectorRule::parse(Effect::Deny, pattern).unwrap()
    }

    fn object(name: &str) -> ObjectRef {
        ObjectRef::parse(name)
    }

    #[test]
    fn table_deny_wins_over_schema_allow() {
        let policy = policy(vec![allow("db.public.*"), deny("db.public.secrets")]);
        assert_eq!(policy.decide(&object("db.public.users")), Decision::Allow);
        assert_eq!(
            policy.decide(&object("db.public.secrets")),
            Decision::DenyHidden
        );
    }

    #[test]
    fn specific_allow_cannot_broaden_parent_deny() {
        let policy = policy(vec![deny("db.*"), allow("db.public.users")]);
        assert_eq!(
            policy.decide(&object("db.public.users")),
            Decision::DenyHidden
        );
    }

    #[test]
    fn deny_and_missing_share_safe_error() {
        assert_eq!(ObjectPolicy::hidden_error(), "not found");
    }

    proptest::proptest! {
        #[test]
        fn adding_a_deny_cannot_increase_accessible_targets(
            deny_idx in 0usize..4usize,
        ) {
            let names = [
                "db.public.users",
                "db.public.orders",
                "db.public.secrets",
                "db.other.t",
            ];
            let base = policy(vec![allow("db.public.*")]);
            let before: Vec<_> = names
                .iter()
                .filter(|name| base.decide(&object(name)) == Decision::Allow)
                .copied()
                .collect();
            let with_deny = policy(vec![allow("db.public.*"), deny(names[deny_idx])]);
            let after: Vec<_> = names
                .iter()
                .filter(|name| with_deny.decide(&object(name)) == Decision::Allow)
                .copied()
                .collect();
            assert!(after.len() <= before.len());
            assert!(after.iter().all(|name| before.contains(name)));
        }
    }
}
