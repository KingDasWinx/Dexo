use dexo_driver_api::{AdminAction, AdminConfirmKind, AdminPreview, LockLevel};

use crate::schema::security::{Confirmation, PolicyDecision};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdminPolicy {
    pub production: bool,
    pub read_only: bool,
}

pub fn production_policy() -> AdminPolicy {
    AdminPolicy {
        production: true,
        read_only: false,
    }
}

pub fn evaluate(action: &AdminAction, own_session: &str, policy: &AdminPolicy) -> PolicyDecision {
    if policy.read_only {
        return PolicyDecision {
            allowed: false,
            confirmation: Confirmation::None,
            reason: Some("admin mutations are disabled".into()),
        };
    }
    match action {
        AdminAction::CancelQuery { session_id } => {
            let own = session_id == own_session;
            PolicyDecision {
                allowed: true,
                confirmation: Confirmation::None,
                reason: Some(if own {
                    "confirm once to cancel own query".into()
                } else {
                    "confirm once to cancel query".into()
                }),
            }
        }
        AdminAction::TerminateSession { session_id } => PolicyDecision {
            allowed: true,
            confirmation: Confirmation::TypeTarget(session_id.clone()),
            reason: Some("type the session id to terminate".into()),
        },
        AdminAction::Vacuum { .. }
        | AdminAction::Analyze { .. }
        | AdminAction::Reindex { .. }
        | AdminAction::Optimize { .. } => PolicyDecision {
            allowed: true,
            confirmation: if policy.production {
                Confirmation::TypeTarget(preview_target(action))
            } else {
                Confirmation::None
            },
            reason: Some("preview exact command and lock risk before running".into()),
        },
    }
}

pub fn confirm_kind(action: &AdminAction, own_session: &str) -> AdminConfirmKind {
    match action {
        AdminAction::TerminateSession { .. } => AdminConfirmKind::TypeTarget,
        AdminAction::CancelQuery { session_id } if session_id != own_session => {
            AdminConfirmKind::Once
        }
        _ => AdminConfirmKind::Once,
    }
}

pub fn never_retry(_preview: &AdminPreview) -> bool {
    true
}

fn preview_target(action: &AdminAction) -> String {
    match action {
        AdminAction::Vacuum { target }
        | AdminAction::Analyze { target }
        | AdminAction::Reindex { target }
        | AdminAction::Optimize { target } => target.display_unquoted(),
        AdminAction::CancelQuery { session_id } | AdminAction::TerminateSession { session_id } => {
            session_id.clone()
        }
    }
}

pub fn lock_label(level: LockLevel) -> &'static str {
    match level {
        LockLevel::None => "none",
        LockLevel::Share => "share",
        LockLevel::Exclusive => "exclusive",
        LockLevel::AccessExclusive => "access exclusive",
    }
}

#[cfg(test)]
mod tests {
    use super::{confirm_kind, evaluate, never_retry, production_policy};
    use crate::schema::security::Confirmation;
    use dexo_driver_api::{AdminAction, AdminConfirmKind, AdminPreview, LockLevel, QualifiedName};

    fn table() -> QualifiedName {
        QualifiedName::new(None::<String>, Some("public"), "items")
    }

    #[test]
    fn production_cancel_own_is_once_terminate_types_target() {
        let policy = production_policy();
        let cancel = AdminAction::CancelQuery {
            session_id: "42".into(),
        };
        let decision = evaluate(&cancel, "42", &policy);
        assert!(decision.allowed);
        assert_eq!(decision.confirmation, Confirmation::None);
        assert_eq!(confirm_kind(&cancel, "42"), AdminConfirmKind::Once);
        let terminate = AdminAction::TerminateSession {
            session_id: "99".into(),
        };
        let decision = evaluate(&terminate, "42", &policy);
        assert_eq!(decision.confirmation, Confirmation::TypeTarget("99".into()));
        assert_eq!(confirm_kind(&terminate, "42"), AdminConfirmKind::TypeTarget);
    }

    #[test]
    fn maintenance_preview_exposes_command_and_lock_and_never_retries() {
        let vacuum = AdminAction::Vacuum { target: table() };
        let preview = AdminPreview {
            command: r#"VACUUM "public"."items""#.into(),
            lock_risk: LockLevel::Share,
            confirmation: AdminConfirmKind::Once,
        };
        assert!(preview.command.contains("VACUUM"));
        assert_eq!(preview.lock_risk, LockLevel::Share);
        assert!(never_retry(&preview));
        let decision = evaluate(&vacuum, "1", &production_policy());
        assert_eq!(
            decision.confirmation,
            Confirmation::TypeTarget("public.items".into())
        );
    }
}
