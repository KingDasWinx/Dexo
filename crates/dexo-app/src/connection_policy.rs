use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Local,
    Development,
    Staging,
    Production,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionPolicy {
    pub read_only: bool,
    pub confirm_destructive: bool,
    pub require_verified_tls: bool,
    pub max_rows: u64,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectionPolicyOverrides {
    pub read_only: Option<bool>,
    pub confirm_destructive: Option<bool>,
    pub require_verified_tls: Option<bool>,
    pub max_rows: Option<u64>,
    pub timeout_secs: Option<u64>,
}

impl Environment {
    pub fn parse(value: &str) -> Self {
        Self::known(value).unwrap_or(Self::Local)
    }

    pub fn known(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "production" => Some(Self::Production),
            "staging" => Some(Self::Staging),
            "development" => Some(Self::Development),
            "local" => Some(Self::Local),
            _ => None,
        }
    }

    /// `parse` maps any unknown label to `Local`. Anything that guards a write fails
    /// closed instead, so `prod`, `PRD` or `live` count as production.
    pub fn parse_strict(label: &str) -> Self {
        match Self::known(label) {
            Some(environment) => environment,
            None if label.is_empty() || label.eq_ignore_ascii_case("local") => Self::Local,
            None => Self::Production,
        }
    }
}

impl ConnectionPolicy {
    pub fn for_environment(environment: Environment) -> Self {
        match environment {
            Environment::Production | Environment::Staging => Self {
                read_only: false,
                confirm_destructive: true,
                require_verified_tls: true,
                max_rows: 10_000,
                timeout_secs: 30,
            },
            Environment::Local | Environment::Development => Self {
                read_only: false,
                confirm_destructive: true,
                require_verified_tls: false,
                max_rows: 100_000,
                timeout_secs: 120,
            },
        }
    }

    pub fn resolve(
        environment: &str,
        overrides: &ConnectionPolicyOverrides,
    ) -> Result<Self, AppError> {
        let mut policy = match Environment::known(environment) {
            Some(env) => Self::for_environment(env),
            None => Self {
                read_only: required_override(overrides.read_only, "read_only", environment)?,
                confirm_destructive: required_override(
                    overrides.confirm_destructive,
                    "confirm_destructive",
                    environment,
                )?,
                require_verified_tls: required_override(
                    overrides.require_verified_tls,
                    "require_verified_tls",
                    environment,
                )?,
                max_rows: required_override(overrides.max_rows, "max_rows", environment)?,
                timeout_secs: required_override(
                    overrides.timeout_secs,
                    "timeout_secs",
                    environment,
                )?,
            },
        };
        if let Some(value) = overrides.read_only {
            policy.read_only = value;
        }
        if let Some(value) = overrides.confirm_destructive {
            policy.confirm_destructive = value;
        }
        if let Some(value) = overrides.require_verified_tls {
            policy.require_verified_tls = value;
        }
        if let Some(value) = overrides.max_rows {
            policy.max_rows = value;
        }
        if let Some(value) = overrides.timeout_secs {
            policy.timeout_secs = value;
        }
        Ok(policy)
    }

    pub fn shows_insecure_indicator(&self) -> bool {
        !self.require_verified_tls
    }
}

fn required_override<T>(value: Option<T>, field: &str, environment: &str) -> Result<T, AppError> {
    value.ok_or_else(|| {
        AppError::new(
            ErrorCategory::Configuration,
            format!(
                "the environment '{environment}' is none of local, development, staging or \
                 production, so the connection must set {field} itself: edit it, and either pick \
                 one of those environments or fill in the policy under Advanced options"
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{ConnectionPolicy, ConnectionPolicyOverrides, Environment};
    use crate::error::ErrorCategory;

    #[test]
    fn production_defaults_to_strict_controls() {
        let policy = ConnectionPolicy::for_environment(Environment::Production);
        assert!(policy.confirm_destructive);
        assert!(policy.require_verified_tls);
        assert_eq!(policy.max_rows, 10_000);
    }

    #[test]
    fn local_defaults_keep_destructive_confirmation() {
        let policy = ConnectionPolicy::for_environment(Environment::Local);
        assert!(policy.confirm_destructive);
        assert_eq!(policy.max_rows, 100_000);
        assert_eq!(policy.timeout_secs, 120);
        assert!(policy.shows_insecure_indicator());
    }

    #[test]
    fn custom_environment_does_not_use_parse_fallback() {
        let error = ConnectionPolicy::resolve("pci-lab", &ConnectionPolicyOverrides::default())
            .unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Configuration);
        assert!(error.to_string().contains("pci-lab"), "{error}");
        assert!(error.to_string().contains("read_only"), "{error}");
        assert!(error.to_string().contains("edit it"), "{error}");
    }

    #[test]
    fn custom_environment_uses_persisted_policy() {
        let policy = ConnectionPolicy::resolve(
            "pci-lab",
            &ConnectionPolicyOverrides {
                read_only: Some(true),
                confirm_destructive: Some(true),
                require_verified_tls: Some(true),
                max_rows: Some(50),
                timeout_secs: Some(5),
            },
        )
        .unwrap();
        assert!(policy.read_only);
        assert_eq!(policy.max_rows, 50);
        assert_eq!(policy.timeout_secs, 5);
    }

    #[test]
    fn unknown_environment_labels_fail_closed() {
        assert_eq!(Environment::parse_strict("prod"), Environment::Production);
        assert_eq!(Environment::parse_strict("PRD"), Environment::Production);
        assert_eq!(Environment::parse_strict(""), Environment::Local);
        assert_eq!(Environment::parse_strict("local"), Environment::Local);
        assert_eq!(Environment::parse_strict("staging"), Environment::Staging);
    }
}
