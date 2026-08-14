use dexo_driver_api::{
    ChangeRisk, DdlPlan, GrantRecord, QualifiedName, SchemaChange, classify_raw_sql,
};
use secrecy::{ExposeSecret, SecretString};

use crate::schema::security::{Confirmation, DdlPolicy, evaluate};

#[derive(Clone, Debug, PartialEq)]
pub struct DdlPreview {
    pub plan: DdlPlan,
    pub risk: ChangeRisk,
    pub dependents: Vec<QualifiedName>,
    pub grants: Vec<GrantRecord>,
    pub confirmation: Confirmation,
    pub warnings: Vec<String>,
}

pub fn preview_change(
    change: &SchemaChange,
    plan: DdlPlan,
    dependents: Vec<QualifiedName>,
    grants: Vec<GrantRecord>,
    policy: &DdlPolicy,
) -> DdlPreview {
    let decision = evaluate(change, policy);
    let mut warnings = plan.warnings.clone();
    if !dependents.is_empty() {
        warnings.push(format!("{} known dependent(s) may break", dependents.len()));
    }
    DdlPreview {
        risk: change.risk(),
        dependents,
        grants,
        confirmation: decision.confirmation,
        warnings,
        plan,
    }
}

pub fn preview_raw_sql(sql: &str) -> ChangeRisk {
    classify_raw_sql(sql)
}

pub fn redact_secret(sql: &str, secret: &SecretString) -> String {
    sql.replace(secret.expose_secret(), "***")
}

#[cfg(test)]
mod tests {
    use super::{preview_change, preview_raw_sql, redact_secret};
    use crate::schema::change::drop_table;
    use crate::schema::security::production_policy;
    use dexo_driver_api::{DdlPlan, DdlStatement, GrantRecord, QualifiedName};
    use secrecy::{ExposeSecret, SecretString};

    #[test]
    fn preview_includes_dependents_grants_and_risk() {
        let change = drop_table("prod.public.orders");
        let preview = preview_change(
            &change,
            DdlPlan {
                statements: vec![DdlStatement {
                    sql: "DROP TABLE \"public\".\"orders\"".into(),
                    implicit_commit: false,
                }],
                rollback: vec![],
                warnings: vec![],
                transactional: true,
            },
            vec![QualifiedName::new(
                Some("prod"),
                Some("public"),
                "orders_mv",
            )],
            vec![GrantRecord {
                principal: QualifiedName::new(None::<String>, None::<String>, "reporter"),
                target: change.target().clone(),
                privileges: vec!["SELECT".into()],
            }],
            &production_policy(),
        );
        assert!(preview.risk.destructive);
        assert_eq!(preview.dependents.len(), 1);
        assert_eq!(preview.grants.len(), 1);
        assert!(matches!(
            preview.confirmation,
            crate::schema::Confirmation::TypeTarget(_)
        ));
    }

    #[test]
    fn unknown_sql_is_high_risk() {
        let risk = preview_raw_sql("VACUUM FULL mystery()");
        assert!(risk.destructive);
        assert!(!risk.reversible);
    }

    #[test]
    fn principal_password_never_appears_in_preview_sql() {
        let secret = SecretString::from("s3cret-pass");
        let sql = format!("CREATE USER reporter PASSWORD '{}'", secret.expose_secret());
        let redacted = redact_secret(&sql, &secret);
        assert!(!redacted.contains("s3cret-pass"));
        assert!(redacted.contains("***"));
    }
}
