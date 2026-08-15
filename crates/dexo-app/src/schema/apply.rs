use dexo_driver_api::{
    DdlExecutor, DdlOutcome, DdlPlan, DriverErrorCategory, QualifiedName, SchemaChange,
};

use crate::error::{AppError, ErrorCategory};
use crate::query_service::map_driver_error;
use crate::schema::security::{Confirmation, DdlPolicy, evaluate};

pub struct ApplyRequest<'a> {
    pub change: &'a SchemaChange,
    pub plan: &'a DdlPlan,
    pub policy: &'a DdlPolicy,
    pub typed_confirmation: Option<&'a str>,
    pub cancelled: bool,
}

pub async fn apply_change(
    executor: &dyn DdlExecutor,
    request: ApplyRequest<'_>,
) -> Result<DdlOutcome, AppError> {
    let decision = evaluate(request.change, request.policy);
    if !decision.allowed {
        return Err(AppError::new(
            ErrorCategory::Permission,
            decision.reason.unwrap_or_else(|| "ddl denied".into()),
        ));
    }
    if let Confirmation::TypeTarget(expected) = decision.confirmation
        && request.typed_confirmation != Some(expected.as_str())
    {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {expected} to confirm"),
        ));
    }
    if request.cancelled {
        return Ok(DdlOutcome::RolledBack);
    }
    match executor.apply_ddl(request.plan).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            if error.category() == DriverErrorCategory::Cancelled {
                Ok(DdlOutcome::Unknown)
            } else {
                Err(map_driver_error(error))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAction {
    Keep,
    InvalidateSubtree,
    MarkUncertain,
}

pub fn invalidate_after_ddl(outcome: DdlOutcome, _target: &QualifiedName) -> CacheAction {
    match outcome {
        DdlOutcome::RolledBack => CacheAction::Keep,
        DdlOutcome::Committed => CacheAction::InvalidateSubtree,
        DdlOutcome::PartiallyCommitted { .. } | DdlOutcome::Unknown => CacheAction::MarkUncertain,
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplyRequest, CacheAction, apply_change, invalidate_after_ddl};
    use crate::schema::change::drop_table;
    use crate::schema::security::{production_policy, read_only_policy};
    use dexo_driver_api::{
        DdlExecutor, DdlOutcome, DdlPlan, DdlStatement, DriverError, QualifiedName,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeDdl {
        outcome: DdlOutcome,
        fail: Option<DriverError>,
        called: AtomicBool,
    }

    #[async_trait::async_trait]
    impl DdlExecutor for FakeDdl {
        fn plan_change(
            &self,
            change: &dexo_driver_api::SchemaChange,
        ) -> Result<DdlPlan, DriverError> {
            Ok(DdlPlan {
                risk: change.risk(),
                ..drop_plan()
            })
        }

        async fn apply_ddl(&self, _: &DdlPlan) -> Result<DdlOutcome, DriverError> {
            self.called.store(true, Ordering::SeqCst);
            if let Some(error) = &self.fail {
                return Err(DriverError::new(error.category(), error.to_string()));
            }
            Ok(self.outcome)
        }
    }

    fn drop_plan() -> DdlPlan {
        DdlPlan {
            statements: vec![DdlStatement {
                sql: "DROP TABLE orders".into(),
                implicit_commit: false,
            }],
            rollback: vec![],
            warnings: vec![],
            transactional: true,
            risk: dexo_driver_api::ChangeRisk {
                destructive: true,
                data_loss: true,
                lock_level: dexo_driver_api::LockLevel::AccessExclusive,
                reversible: false,
            },
        }
    }

    #[tokio::test]
    async fn production_drop_requires_exact_confirmation() {
        let change = drop_table("prod.public.orders");
        let plan = drop_plan();
        let fake = FakeDdl {
            outcome: DdlOutcome::Committed,
            fail: None,
            called: AtomicBool::new(false),
        };
        let err = apply_change(
            &fake,
            ApplyRequest {
                change: &change,
                plan: &plan,
                policy: &production_policy(),
                typed_confirmation: None,
                cancelled: false,
            },
        )
        .await
        .unwrap_err();
        assert!(!fake.called.load(Ordering::SeqCst));
        assert!(err.to_string().contains("prod.public.orders"));
        let outcome = apply_change(
            &fake,
            ApplyRequest {
                change: &change,
                plan: &plan,
                policy: &production_policy(),
                typed_confirmation: Some("prod.public.orders"),
                cancelled: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, DdlOutcome::Committed);
        assert!(fake.called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn read_only_and_cancel_do_not_execute() {
        let change = drop_table("db.public.t");
        let plan = drop_plan();
        let fake = FakeDdl {
            outcome: DdlOutcome::Committed,
            fail: None,
            called: AtomicBool::new(false),
        };
        assert!(
            apply_change(
                &fake,
                ApplyRequest {
                    change: &change,
                    plan: &plan,
                    policy: &read_only_policy(),
                    typed_confirmation: None,
                    cancelled: false,
                },
            )
            .await
            .is_err()
        );
        assert!(!fake.called.load(Ordering::SeqCst));
        let outcome = apply_change(
            &fake,
            ApplyRequest {
                change: &change,
                plan: &plan,
                policy: &production_policy(),
                typed_confirmation: Some("db.public.t"),
                cancelled: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, DdlOutcome::RolledBack);
        assert!(!fake.called.load(Ordering::SeqCst));
    }

    #[test]
    fn first_statement_failure_keeps_catalog_uncertain_marks_stale() {
        let target = QualifiedName::new(Some("db"), Some("public"), "orders");
        assert_eq!(
            invalidate_after_ddl(DdlOutcome::RolledBack, &target),
            CacheAction::Keep
        );
        assert_eq!(
            invalidate_after_ddl(DdlOutcome::Committed, &target),
            CacheAction::InvalidateSubtree
        );
        assert_eq!(
            invalidate_after_ddl(DdlOutcome::PartiallyCommitted { committed: 1 }, &target),
            CacheAction::MarkUncertain
        );
        assert_eq!(
            invalidate_after_ddl(DdlOutcome::Unknown, &target),
            CacheAction::MarkUncertain
        );
    }
}
